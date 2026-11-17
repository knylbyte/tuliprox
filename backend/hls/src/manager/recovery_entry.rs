use super::revision_recovery::{local_file_has_length, local_segment_dependencies_ready};
use crate::{
    lease::{
        HlsAccessLeaseStore, HlsLeaseManifestPublicationGuard, HlsLeaseManifestPublicationOutcome as Outcome,
        HlsLeaseManifestPublicationRejectReason as Reject,
    },
    manifest_acceptance::{
        manifest_acceptance_episode_status, HlsManifestAcceptanceEpisodeStatus, HlsManifestAcceptanceGeneration,
        HlsManifestAcceptanceState,
    },
    manifest_commit::{HlsCachedEntryState, HlsCachedManifestOptions},
    media_reserve::{evaluate_lease_reserve, HlsLeaseManifestSnapshot, HlsLeaseReserveInput, HlsManifestDeliveryMode},
    origin_progress::{awaiting_media_progress, HlsOriginPathCondition, HlsOriginProgressPhase},
    recovery_timing::HlsRecoveryTriggerBudgetMs,
    session::HlsRecoveryEntryLeases,
    terminal_tail::HlsLeasePlaybackMode,
    HlsAccessLease, HlsAccessLeaseId, HlsAccessLeaseState, HlsProxyManager, HlsPublishedTransientResourceIds,
    HlsSession, HlsSessionHandle, HlsSessionMode, ProxySessionId, SegmentCacheStatus, SegmentEntry,
};
use std::{cmp::Ordering, sync::Arc};

pub enum HlsCachedEntryCommit {
    Publication(Outcome),
    Unavailable(&'static str),
}

impl HlsSession {
    pub(crate) fn recovery_entry_context(&self, now_ms: u64) -> Option<(HlsManifestAcceptanceGeneration, bool)> {
        if !matches!(self.mode, HlsSessionMode::NormalCacheTimeline)
            || self.origin_playback_kind() != tuliprox_core::model::PlaybackKind::LiveHls
            || self.published_live_origin_baseline.is_none()
            || self.is_gc_marked_for_removal()
        {
            return None;
        }
        let episode = self.origin_control.acceptance_episode.as_ref()?;
        let generation = self.origin_control.acceptance_generation;
        if episode.generation != generation
            || episode.state == HlsManifestAcceptanceState::Completed
            || episode.deterministic_conflict_receipt().is_some()
        {
            return None;
        }
        let qualification = &self.origin_control.qualification;
        if qualification.generation() != Some(generation)
            || !qualification.pending_is_current(&crate::short_manifest::HlsOriginQualificationBasis::capture(self))
        {
            return None;
        }
        let retry_evidence = episode.next_retry_at_ms.is_some();
        if self.origin_refresh.in_flight {
            let recovering = self.origin_refresh.attempt.as_ref().is_some_and(|attempt| attempt.recovery_in_progress());
            let pending = matches!(
                manifest_acceptance_episode_status(Some(episode), generation, now_ms),
                HlsManifestAcceptanceEpisodeStatus::InFlight { .. }
            );
            return (recovering && (pending || retry_evidence)).then_some((generation, false));
        }
        // RetryPending is the same binding and generation evidence used to
        // resume qualification after a shared refresh finishes and backs off.
        (retry_evidence
            && episode.state == HlsManifestAcceptanceState::Holding
            && self.origin_refresh.last_error_at_ms.is_some()
            && self.origin_qualification_retry_target().is_some())
        .then_some((generation, true))
    }

    pub(crate) fn awaiting_media_progress_generation(&self, now_ms: u64) -> Option<u64> {
        let control = &self.origin_control;
        let last_progress_at_ms = control.last_media_progress_at_ms?;
        let target_duration_ms = control
            .target_duration_snapshot_ms
            .or_else(|| self.target_duration.map(|seconds| u64::from(seconds) * 1_000))?;
        (matches!(self.mode, HlsSessionMode::NormalCacheTimeline)
            && self.origin_playback_kind() == tuliprox_core::model::PlaybackKind::LiveHls
            && self.published_live_origin_baseline.is_some()
            && !self.is_gc_marked_for_removal()
            && matches!(
                control.progress_phase,
                HlsOriginProgressPhase::Fresh | HlsOriginProgressPhase::AwaitingMediaProgress
            )
            && matches!(
                control.path_condition,
                HlsOriginPathCondition::ProgressExpected | HlsOriginPathCondition::RetryableFetchFailure
            )
            && awaiting_media_progress(now_ms.saturating_sub(last_progress_at_ms), target_duration_ms))
        .then_some(control.progress_generation)
    }

    pub fn cached_entry_is_current(
        &self,
        lease: &HlsAccessLease,
        options: HlsCachedManifestOptions,
        now_ms: u64,
    ) -> bool {
        self.recovery_entry_boundaries_allow(lease)
            && options.cached_entry.is_some_and(|context| match context.state {
                HlsCachedEntryState::Recovery { generation, .. } => {
                    self.recovery_entry_context(now_ms).is_some_and(|(id, _)| id == generation)
                }
                HlsCachedEntryState::AwaitingMediaProgress { progress_generation } => {
                    !self.idle_for_origin_reentry(now_ms)
                        && self.awaiting_media_progress_generation(now_ms) == Some(progress_generation)
                }
            })
    }

    pub(crate) fn recovery_entry_boundaries_allow(&self, lease: &HlsAccessLease) -> bool {
        lease.state == HlsAccessLeaseState::Pending
            && lease.last_manifest_snapshot.is_none()
            && lease.playback_mode == HlsLeasePlaybackMode::Live
            && lease.runtime_policy_revocation.is_none()
            && self.live_entry_allowed_for_lease(lease)
            && self.fresh_manifest_commit_required.is_none()
            && self.origin_account_manifest_pending.is_none()
            && self.pending_handoff_discontinuity_sequence.is_none()
            && !matches!(
                self.origin_control.progress_phase,
                HlsOriginProgressPhase::Terminal | HlsOriginProgressPhase::TerminalPartial
            )
    }

    pub(crate) fn capture_recovery_entry_leases(
        &mut self,
        leases: &mut HlsAccessLeaseStore,
        cache: &crate::HlsSegmentCache,
        now_ms: u64,
    ) {
        self.confirm_recovery_entry_progress(cache, now_ms);
        if self.recovery_entry_leases.is_some()
            || self.published_live_origin_baseline.is_none()
            || !matches!(self.mode, HlsSessionMode::NormalCacheTimeline)
            || self.origin_playback_kind() != tuliprox_core::model::PlaybackKind::LiveHls
        {
            return;
        }
        let Some(first_new_proxy_seq) = self.proxy_next_seq else { return };
        let established = leases
            .active_live_playback_snapshots_for_session(&self.proxy_session_id, now_ms)
            .into_iter()
            .map(|lease| (lease.lease_id, lease.issued_at_ms))
            .collect();
        self.recovery_entry_leases = Some(HlsRecoveryEntryLeases { first_new_proxy_seq, established, closed: None });
    }

    pub(crate) fn confirm_recovery_entry_progress(&mut self, cache: &crate::HlsSegmentCache, now_ms: u64) {
        let entry_from = self.recovery_entry_leases.as_ref().map(|captured| captured.first_new_proxy_seq);
        let poll_from = self.origin_control.entry_407_recovery_from;
        let Some(first_new_proxy_seq) = entry_from.into_iter().chain(poll_from).min() else { return };
        let Some(source) = self
            .last_rendered_manifest
            .as_ref()
            .filter(|source| source.valid_until_ms >= now_ms && source.last_proxy_seq >= first_new_proxy_seq)
        else {
            return;
        };
        // Progress must be reachable from the committed window's head. A READY
        // origin segment beyond a gap may still be unusable in this publication.
        let mut previous = None;
        for seq in &source.segment_proxy_seqs {
            let Some(entry) = self.segments.get(seq) else { break };
            if !recovery_entry_contiguous(previous, entry)
                || !recovery_entry_segment_ready(cache, self, entry, None, now_ms)
            {
                break;
            }
            if self.live_origin_sequences().contains(seq) {
                if entry_from.is_some_and(|first| *seq >= first) {
                    self.recovery_entry_leases = None;
                }
                if poll_from.is_some_and(|first| *seq >= first) {
                    self.origin_control.entry_407_recovery_from = None;
                }
            }
            if self.recovery_entry_leases.is_none() && self.origin_control.entry_407_recovery_from.is_none() {
                return;
            }
            previous = Some(entry);
        }
    }
}

fn recovery_entry_contiguous(previous: Option<&SegmentEntry>, entry: &SegmentEntry) -> bool {
    previous.is_none_or(|prior| {
        prior.proxy_seq.checked_add(1) == Some(entry.proxy_seq)
            && prior.media_start_ms.checked_add(prior.duration_ms) == Some(entry.media_start_ms)
    })
}

pub(super) fn recovery_entry_segment_ready(
    cache: &crate::HlsSegmentCache,
    session: &HlsSession,
    entry: &SegmentEntry,
    publication: Option<&crate::HlsManifestRevisions>,
    now_ms: u64,
) -> bool {
    let SegmentCacheStatus::Ready { content_length, .. } = entry.status else { return false };
    if crate::is_hls_provisioning_segment(entry) || crate::timeline::is_hls_provisioning_gap_segment(entry) {
        return false;
    }
    let media_ready = if let Some(startup) = session.startup.as_ref() {
        startup.revisions.get(&entry.proxy_seq).is_some_and(|revision| {
            publication.is_none_or(|snapshot| snapshot.revisions.get(&entry.proxy_seq) == Some(revision))
                && matches!(revision.revision().state(), crate::SegmentRevisionState::Complete(metadata)
                    if local_file_has_length(&metadata.path, metadata.size))
        })
    } else {
        local_file_has_length(&cache.object_path(&entry.cache_key), content_length)
    };
    media_ready
        && entry.encryption.as_ref().is_none_or(|encryption| {
            session
                .transient
                .ready_key_object_valid_until_ms(
                    &session.proxy_session_id,
                    &encryption.resource_id,
                    &encryption.resource_extension,
                    now_ms,
                )
                .is_some()
        })
        && local_segment_dependencies_ready(cache, session, entry)
}

impl HlsCachedManifestOptions {
    pub fn cached_entry_requested(self) -> bool { self.cached_entry.is_some() }

    pub fn permits_cached_entry(self) -> bool { self.cached_entry.is_some_and(|context| context.permits_fallback) }

    pub fn awaiting_media_progress_entry(self) -> bool {
        self.cached_entry.is_some_and(|entry| matches!(entry.state, HlsCachedEntryState::AwaitingMediaProgress { .. }))
    }

    pub fn cached_entry_phase(self) -> &'static str {
        match self.cached_entry.map(|entry| entry.state) {
            Some(HlsCachedEntryState::AwaitingMediaProgress { .. }) => "awaiting_media_progress",
            Some(HlsCachedEntryState::Recovery { between_attempts: true, .. }) => "between_attempts",
            _ => "in_flight",
        }
    }
}

impl HlsProxyManager {
    pub(super) fn validate_recovery_entry_continuation(
        &self,
        leases: &HlsAccessLeaseStore,
        session: &HlsSession,
        lease: &HlsAccessLease,
        expected: HlsLeaseManifestPublicationGuard,
        candidate: &HlsLeaseManifestSnapshot,
        now_ms: u64,
    ) -> Result<(), Reject> {
        if candidate.delivery_mode != HlsManifestDeliveryMode::NormalCacheTimeline || candidate.is_recovery_entry() {
            return Ok(());
        }
        let current = lease.last_manifest_snapshot.as_ref().filter(|snapshot| snapshot.is_recovery_entry());
        if expected.recovery_entry_snapshot_generation() != current.map(|snapshot| snapshot.snapshot_generation) {
            return Err(Reject::SnapshotChanged);
        }
        let Some(current) = current else { return Ok(()) };
        let order = current.recovery_entry_origin_order(candidate).ok_or(Reject::ManifestGenerationUnavailable)?;
        if order == Ordering::Less
            || candidate.first_proxy_seq < current.first_proxy_seq
            || candidate.last_proxy_seq < current.last_proxy_seq
        {
            return Err(Reject::SourceRegressive);
        }
        if order == Ordering::Greater {
            return Ok(());
        }
        let (Some(previous_source), Some(source)) = (&current.cached_source, &candidate.cached_source) else {
            return Err(Reject::ManifestGenerationUnavailable);
        };
        let revisions = candidate.startup_revisions.as_deref();
        let regular_window = revisions.and_then(|publication| publication.window_plan.as_ref());
        let current_source = regular_window
            .map_or_else(|| session.last_normal_manifest_commit_identity(), |_| session.live_window_source_identity());
        if !expected.has_source_binding()
            || source.origin_commits != session.origin_manifest_commits()
            || current_source != Some(candidate.source_commit_identity)
            || session.fresh_manifest_commit_required.is_some()
        {
            return Err(Reject::ManifestGenerationUnavailable);
        }
        let previous_units = &current.visible_segments;
        let units = &candidate.visible_segments;
        if candidate.first_proxy_seq > current.last_proxy_seq.saturating_add(1)
            || units.first().map(|unit| unit.proxy_seq) != Some(candidate.first_proxy_seq)
            || units.last().map(|unit| unit.proxy_seq) != Some(candidate.last_proxy_seq)
            || previous_units.len() != previous_source.bindings.len()
            || units.len() != source.bindings.len()
        {
            return Err(Reject::RevisionConflict);
        }
        let overlap_start = previous_units.partition_point(|unit| unit.proxy_seq < candidate.first_proxy_seq);
        let hidden_discontinuities =
            u64::try_from(previous_units[..overlap_start].iter().filter(|unit| unit.discontinuity_before).count())
                .unwrap_or(u64::MAX);
        if current.discontinuity_sequence.checked_add(hidden_discontinuities) != Some(candidate.discontinuity_sequence)
            || units.len() < previous_units.len() - overlap_start
        {
            return Err(Reject::RevisionConflict);
        }
        let previous_revisions = current.startup_revisions.as_ref();
        let mut predecessor = (overlap_start == previous_units.len())
            .then(|| previous_units.last().zip(previous_source.bindings.last()))
            .flatten();
        for (index, (unit, binding)) in units.iter().zip(source.bindings.iter()).enumerate() {
            let seq = unit.proxy_seq;
            let entry = session.segments.get(&seq).ok_or(Reject::RevisionConflict)?;
            if !binding.is_current(session, entry)
                || entry.duration_ms != unit.duration_ms
                || (seq != candidate.first_proxy_seq && entry.discontinuity_before != unit.discontinuity_before)
                || predecessor.is_some_and(|(prior, prior_binding)| {
                    prior.proxy_seq.checked_add(1) != Some(seq)
                        || prior_binding.media_start_ms.checked_add(prior.duration_ms) != Some(binding.media_start_ms)
                })
            {
                return Err(Reject::RevisionConflict);
            }
            let previous_index = overlap_start + index;
            if let Some(prior) = previous_units.get(previous_index) {
                let prior_revision = previous_revisions.and_then(|publication| publication.revisions.get(&seq));
                let candidate_revision = revisions.and_then(|publication| publication.revisions.get(&seq));
                if prior != unit
                    || previous_source.bindings.get(previous_index) != Some(binding)
                    || prior_revision != candidate_revision
                    || leases.published_segment_revision(&lease.lease_id, seq).as_ref() != prior_revision
                {
                    return Err(Reject::RevisionConflict);
                }
            } else if !session.live_origin_sequences().contains(&seq)
                || !recovery_entry_segment_ready(&self.segment_cache, session, entry, revisions, now_ms)
            {
                return Err(Reject::RevisionConflict);
            }
            predecessor = Some((unit, binding));
        }
        Ok(())
    }

    pub async fn check_recovery_entry_reserve(
        &self,
        session: &HlsSessionHandle,
        lease: &HlsAccessLease,
        options: HlsCachedManifestOptions,
    ) -> Result<(), &'static str> {
        let leases = self.access_leases.read().await;
        let mut session = session.write().await;
        let now_ms = self.now_ms();
        if !session.cached_entry_is_current(lease, options, now_ms) {
            return Err("recovery_context_changed");
        }
        self.require_established_live_reserve(&leases, &mut session, now_ms)
    }

    fn require_established_live_reserve(
        &self,
        leases: &HlsAccessLeaseStore,
        session: &mut HlsSession,
        now_ms: u64,
    ) -> Result<(), &'static str> {
        session.confirm_recovery_entry_progress(&self.segment_cache, now_ms);
        let Some(captured) = session.recovery_entry_leases.as_ref() else { return Err("no_established_live_reserve") };
        if let Some(reason) = captured.closed {
            return Err(reason);
        }
        let mut known_position = false;
        for (id, issued_at_ms) in &captured.established {
            let Some(lease) = leases.live_playback_lease(id, &session.proxy_session_id, now_ms).filter(|lease| {
                lease.issued_at_ms == *issued_at_ms
                    && lease.state == HlsAccessLeaseState::Activated
                    && lease.active_until_ms.is_some_and(|until| until > now_ms)
                    && lease.runtime_policy_revocation.is_none()
            }) else {
                continue;
            };
            let Some(manifest) = lease
                .last_manifest_snapshot
                .as_ref()
                .filter(|manifest| manifest.delivery_mode == HlsManifestDeliveryMode::NormalCacheTimeline)
            else {
                continue;
            };
            let cursor = &lease.playback_cursor;
            if cursor.first_segment_started_proxy_seq.and_then(|seq| session.segments.get(&seq)).is_some_and(|entry| {
                crate::is_hls_provisioning_segment(entry) || crate::timeline::is_hls_provisioning_gap_segment(entry)
            }) {
                continue;
            }
            let mut timeline = session
                .ready_timeline_snapshot(cursor.ready_timeline_start_proxy_seq(manifest.first_proxy_seq), now_ms);
            // Use the same contiguous timeline as normal playback, but only
            // count media that this session can still deliver from local storage.
            for unit in Arc::make_mut(&mut timeline.units) {
                if !session.segments.get(&unit.proxy_seq).is_some_and(|entry| {
                    let publication = manifest
                        .startup_revisions
                        .as_deref()
                        .filter(|_| (manifest.first_proxy_seq..=manifest.last_proxy_seq).contains(&unit.proxy_seq));
                    recovery_entry_segment_ready(&self.segment_cache, session, entry, publication, now_ms)
                }) {
                    unit.state = crate::media_reserve::HlsReadyMediaState::NotReady;
                    break;
                }
            }
            let reserve = evaluate_lease_reserve(HlsLeaseReserveInput {
                manifest,
                cursor,
                ready_timeline: &timeline,
                now_ms,
                recovery_trigger_budget: HlsRecoveryTriggerBudgetMs::from_millis(0),
                origin_path_degraded: false,
                recovery_committed: false,
            });
            known_position |= reserve.playback_position_known;
            if reserve.playback_position_known && reserve.guaranteed_reserve_ms > 0 {
                return Ok(());
            }
        }
        let reason = if known_position { "established_live_reserve_exhausted" } else { "no_established_live_reserve" };
        if let Some(captured) = &mut session.recovery_entry_leases {
            captured.closed = Some(reason);
        }
        Err(reason)
    }

    pub fn recovery_entry_bindings_ready(&self, session: &HlsSession, snapshot: &HlsLeaseManifestSnapshot) -> bool {
        let now_ms = self.now_ms();
        let mut previous: Option<&SegmentEntry> = None;
        snapshot.visible_proxy_seqs().all(|seq| {
            let Some(entry) = session.segments.get(&seq) else { return false };
            let contiguous = recovery_entry_contiguous(previous, entry);
            previous = Some(entry);
            contiguous
                && (session.startup.is_none() || snapshot.startup_revisions.is_some())
                && recovery_entry_segment_ready(
                    &self.segment_cache,
                    session,
                    entry,
                    snapshot.startup_revisions.as_deref(),
                    now_ms,
                )
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn commit_cached_entry_manifest(
        &self,
        lease_id: &HlsAccessLeaseId,
        session_id: &ProxySessionId,
        guard: HlsLeaseManifestPublicationGuard,
        mut snapshot: HlsLeaseManifestSnapshot,
        resources: HlsPublishedTransientResourceIds,
        options: HlsCachedManifestOptions,
    ) -> HlsCachedEntryCommit {
        let rejected = |reason| HlsCachedEntryCommit::Publication(Outcome::Rejected(reason));
        let Some(current) = self.sessions.hold_current_proxy_session(session_id).await else {
            return rejected(Reject::SessionMismatch);
        };
        let mut leases = self.access_leases.write().await;
        let mut session = current.session().write().await;
        let now_ms = self.now_ms();
        let Some(lease) = leases.response_snapshot(lease_id, session_id, now_ms) else {
            return rejected(Reject::LeaseUnavailable);
        };
        let Some(source) = session.last_rendered_manifest.as_ref() else {
            return rejected(Reject::SnapshotChanged);
        };
        let count = snapshot.visible_segments.len();
        let from_committed_origin = snapshot.cached_source.as_ref().is_some_and(|source| {
            source.recovery_entry && source.origin_commits == session.last_normal_manifest_origin_commits()
        });
        if !guard.has_source_binding()
            || guard.entry_strip_policy().is_none()
            || !options.permits_cached_entry()
            || !session.cached_entry_is_current(&lease, options, now_ms)
            || !session.live_entry_snapshot_allowed_for_lease(&lease, Some(&snapshot))
            || now_ms > source.valid_until_ms
            || snapshot.delivery_mode != crate::media_reserve::HlsManifestDeliveryMode::NormalCacheTimeline
            || !from_committed_origin
            || count == 0
            || !source
                .segment_proxy_seqs
                .windows(count)
                .any(|window| window.iter().copied().eq(snapshot.visible_proxy_seqs()))
        {
            return rejected(Reject::SnapshotChanged);
        }
        let guard = match self.validate_entry_manifest_publication(
            guard,
            &snapshot,
            &session,
            Arc::as_ptr(current.session()) as usize,
        ) {
            Ok(guard) => guard,
            Err(Reject::ManifestGenerationUnavailable) => return rejected(Reject::SnapshotChanged),
            Err(reason) => return rejected(reason),
        };
        if !options.awaiting_media_progress_entry() {
            if let Err(reason) = self.require_established_live_reserve(&leases, &mut session, now_ms) {
                return HlsCachedEntryCommit::Unavailable(reason);
            }
        }
        if !self.recovery_entry_bindings_ready(&session, &snapshot) {
            return HlsCachedEntryCommit::Unavailable("missing_window_bindings");
        }
        snapshot.delivered_at_ms = now_ms;
        let outcome = leases.commit_manifest_publication_with_resources(
            lease_id,
            session_id,
            guard,
            snapshot.clone(),
            resources.clone(),
            now_ms,
        );
        if outcome.is_committed() {
            if options.awaiting_media_progress_entry() {
                session.origin_control.progress_phase = HlsOriginProgressPhase::AwaitingMediaProgress;
            }
            session.record_live_window_publication_with_resources(
                lease_id,
                guard.lease_issued_at_ms(),
                &snapshot,
                resources,
            );
        }
        HlsCachedEntryCommit::Publication(outcome)
    }
}
