//! The acceptance episode: one bounded attempt to get a manifest accepted.
//!
//! An episode opens when recovery starts, accumulates candidate observations and
//! the evidence the committed resources give, and closes either committed, held,
//! or exhausted. Everything here reads and writes that episode's state on the
//! session.

use super::{
    current_time_millis,
    fingerprint::{build_manifest_timeline_fingerprint, is_same_host_reset_candidate, HlsManifestCandidateFingerprint},
    recovery::{
        acceptance_attempt_may_start, HlsManifestRecoveryAttemptPlan, HlsManifestRecoveryCandidate,
        HlsManifestRequalificationWindow,
    },
    HlsManifestOriginRelation, HlsManifestSequenceRelation, HlsOriginManifestFetchContext,
};
use crate::{
    hls_origin_log_value,
    manifest_acceptance::{
        classify_host_local_sequence, HlsAlternativeOriginCohort, HlsCandidateHostRelation,
        HlsCommittedContentAnchorEvidence, HlsCrossHostAcceptanceEvidence, HlsEmergencyLiveHandoffCompatibility,
        HlsHostLocalSequenceRelation, HlsManifestAcceptanceExhaustionReason, HlsManifestAcceptanceGeneration,
        HlsManifestAcceptanceLandscape, HlsManifestAcceptanceState, HlsManifestAcceptanceTrigger,
        HlsManifestCandidateObservation, HlsResourceTimelineEvidence, HlsSwitchSegmentReadiness,
    },
    recovery_timing::{
        HlsAcceptanceEpisodeTiming, HlsAcceptanceEpisodeTimingInput, HlsAcceptanceEpisodeTimingSeed,
        HlsRecoveryWorkloadEnvelope, HlsTerminalMediaPreparationState, HlsTransitionMarginMs,
    },
    resource_identity::{HlsMediaResourceIdentity, HlsSegmentOccurrenceIdentity, HlsVerifiedOccurrenceBinding},
};
use log::debug;
use shared::model::HlsManifestRecoveryBurstPlan;

pub(super) fn manifest_acceptance_generation_is_current(
    session: &crate::HlsSession,
    generation: HlsManifestAcceptanceGeneration,
) -> bool {
    session.origin_control.acceptance_generation == generation
        && session.origin_control.acceptance_episode.as_ref().is_some_and(|episode| episode.generation == generation)
}

pub(super) async fn begin_manifest_acceptance_episode(
    context: &HlsOriginManifestFetchContext,
    plan: &HlsManifestRecoveryAttemptPlan<'_>,
    burst_plan: HlsManifestRecoveryBurstPlan,
    expected_generation: Option<HlsManifestAcceptanceGeneration>,
) -> Option<HlsManifestAcceptanceGeneration> {
    let runtime = context.availability_runtime.as_ref().and_then(crate::hls_ctx::WeakHlsCtx::upgrade);
    let mut leases = if let Some(runtime) = runtime.as_ref().filter(|_| plan.attempt_index == 0) {
        Some(runtime.hls_proxy.access_leases().write().await)
    } else {
        None
    };
    let mut session = context.session.write().await;
    // Keep ownership validation atomic with the following episode/qualification changes.
    if expected_generation.is_some_and(|generation| !manifest_acceptance_generation_is_current(&session, generation)) {
        return None;
    }
    let basis = crate::short_manifest::HlsOriginQualificationBasis::capture(&session);
    if !session.origin_control.qualification.pending_is_current(&basis) {
        return None;
    }
    if plan.attempt_index != 0 {
        let episode = session.origin_control.acceptance_episode.as_mut()?;
        if episode.trigger() != plan.trigger || episode.state == HlsManifestAcceptanceState::Completed {
            return None;
        }
        episode.state = HlsManifestAcceptanceState::Collecting;
        let generation = episode.generation;
        session.origin_control.qualification =
            crate::short_manifest::HlsOriginQualification::Checking { basis, generation, target: plan.binding.clone() };
        return Some(generation);
    }
    // A recovery that starts after an earlier qualification is itself the next
    // shared check. Reuse its burst for duration selection, including ordinary
    // redirect/transport recovery, before adopting a changed origin foundation.
    if let Some(previous) =
        session.origin_control.target_duration_qualification.as_ref().filter(|check| check.result.is_some())
    {
        let config = context.app_config.config.load();
        let minimum = config
            .reverse_proxy
            .as_ref()
            .and_then(|proxy| proxy.hls_cache.as_ref())
            .map_or(previous.minimum, |hls| hls.min_origin_target_duration_secs);
        session.origin_control.target_duration_qualification =
            Some(crate::short_manifest::HlsTargetDurationQualification::new(&session, minimum));
    }
    let started_at_ms = current_time_millis();
    let timing = acceptance_episode_timing(context, &session, started_at_ms, burst_plan);
    if let (Some(runtime), Some(leases)) = (&runtime, &mut leases) {
        session.capture_recovery_entry_leases(leases, runtime.hls_proxy.segment_cache(), runtime.hls_proxy.now_ms());
    }
    let generation = session.origin_control.begin_acceptance_episode(started_at_ms, burst_plan, plan.trigger, &timing);
    session.origin_control.qualification =
        crate::short_manifest::HlsOriginQualification::Checking { basis, generation, target: plan.binding.clone() };
    if let Some(episode) = session.origin_control.acceptance_episode.as_mut() {
        episode.state = HlsManifestAcceptanceState::Collecting;
        debug!(
            "HLS manifest acceptance full burst started: generation={} candidates={} max_stagger_ms={} binding_scheme={} binding_host={} provider_url_index={}",
            generation.0,
            episode.required_candidates(),
            episode.burst_max_stagger_ms(),
            plan.binding.request_url().scheme(),
            plan.binding
                .request_url()
                .host_str()
                .map_or_else(|| "none".to_string(), hls_origin_log_value),
            plan.binding
                .provider_url_index()
                .map_or_else(|| "none".to_string(), |index| index.to_string())
        );
    }
    Some(generation)
}

fn acceptance_episode_timing(
    context: &HlsOriginManifestFetchContext,
    session: &crate::HlsSession,
    started_at_ms: u64,
    burst_plan: HlsManifestRecoveryBurstPlan,
) -> HlsAcceptanceEpisodeTiming {
    let fallback_target_duration_ms = session
        .origin_control
        .target_duration_snapshot_ms
        .or_else(|| session.target_duration.map(|seconds| u64::from(seconds).saturating_mul(1_000)))
        .unwrap_or_else(|| context.acceptance_timing_seed.map_or(15_000, |seed| seed.target_duration_ms));
    let seed = context.acceptance_timing_seed.unwrap_or(HlsAcceptanceEpisodeTimingSeed {
        target_duration_ms: fallback_target_duration_ms,
        transition_margin: HlsTransitionMarginMs::from_millis(fallback_target_duration_ms),
        workload: HlsRecoveryWorkloadEnvelope::acceptance_policy().ceiling(),
        required_terminal_media_key: None,
        terminal_media_preparation: HlsTerminalMediaPreparationState::Failed { key: None },
    });
    HlsAcceptanceEpisodeTiming::from_input(&HlsAcceptanceEpisodeTimingInput {
        started_at_ms,
        burst_plan,
        target_duration_ms: fallback_target_duration_ms,
        transition_margin: seed.transition_margin,
        workload: seed.workload,
        observed_latency: session.origin_control.recovery_samples.latency_snapshot(),
        required_terminal_media_key: seed.required_terminal_media_key,
        terminal_media_preparation: seed.terminal_media_preparation,
        policy: context.recovery_timing_policy,
    })
}

#[derive(Debug, Clone)]
pub(super) struct HlsManifestAcceptanceEpisodeSnapshot {
    pub(super) trigger: HlsManifestAcceptanceTrigger,
    pub(super) full_burst_completed: bool,
    pub(super) held_alternative: Option<HlsAlternativeOriginCohort>,
    pub(super) observed_landscape: Option<HlsManifestAcceptanceLandscape>,
}

pub(super) async fn manifest_acceptance_episode_snapshot(
    context: &HlsOriginManifestFetchContext,
    generation: Option<HlsManifestAcceptanceGeneration>,
) -> Option<HlsManifestAcceptanceEpisodeSnapshot> {
    let generation = generation?;
    let session = context.session.read().await;
    session.origin_control.acceptance_episode.as_ref().filter(|episode| episode.generation == generation).map(
        |episode| HlsManifestAcceptanceEpisodeSnapshot {
            trigger: episode.trigger(),
            full_burst_completed: episode.full_burst_completed,
            held_alternative: episode.held_alternative.clone(),
            observed_landscape: episode.observed_landscape.clone(),
        },
    )
}

pub(super) async fn record_manifest_acceptance_landscape(
    context: &HlsOriginManifestFetchContext,
    generation: Option<HlsManifestAcceptanceGeneration>,
    landscape: HlsManifestAcceptanceLandscape,
) {
    let Some(generation) = generation else {
        return;
    };
    let mut session = context.session.write().await;
    let Some(episode) = session.origin_control.acceptance_episode.as_mut() else {
        return;
    };
    if episode.generation == generation {
        episode.observed_landscape = Some(landscape);
    }
}

pub(super) async fn begin_requalified_manifest_acceptance_episode(
    context: &HlsOriginManifestFetchContext,
    generation: Option<HlsManifestAcceptanceGeneration>,
    trigger: HlsManifestAcceptanceTrigger,
    window: &HlsManifestRequalificationWindow,
) -> Option<HlsManifestAcceptanceGeneration> {
    let generation = generation?;
    let mut session = context.session.write().await;
    let now_ms = current_time_millis();
    match window {
        HlsManifestRequalificationWindow::CurrentEpisode(deadline)
            if !acceptance_attempt_may_start(false, now_ms, 0, *deadline) =>
        {
            return None
        }
        HlsManifestRequalificationWindow::CurrentEpisode(_)
        | HlsManifestRequalificationWindow::ResumedAfterSharedBackoff => {}
    }
    let basis = crate::short_manifest::HlsOriginQualificationBasis::capture(&session);
    if !matches!(
        &session.origin_control.qualification,
        crate::short_manifest::HlsOriginQualification::Checking { basis: current, generation: current_generation, .. }
            if current == &basis && *current_generation == generation
    ) {
        return None;
    }
    let episode = session.origin_control.acceptance_episode.as_mut()?;
    if episode.generation != generation
        || !episode.full_burst_completed
        || episode.state == HlsManifestAcceptanceState::Completed
    {
        return None;
    }
    episode.state = HlsManifestAcceptanceState::Holding;
    let configured_plan = context.manifest_recovery_burst.level.plan();
    let timing = acceptance_episode_timing(context, &session, now_ms, configured_plan);
    let next_generation = session.origin_control.begin_acceptance_episode(now_ms, configured_plan, trigger, &timing);
    if let crate::short_manifest::HlsOriginQualification::Checking { generation, .. } =
        &mut session.origin_control.qualification
    {
        *generation = next_generation;
    }
    debug!(
        "HLS manifest acceptance landscape changed: previous_generation={} next_generation={} candidates={} decision=full-requalification",
        generation.0,
        next_generation.0,
        configured_plan.total_candidates()
    );
    Some(next_generation)
}

pub(super) async fn record_full_manifest_acceptance_burst(
    context: &HlsOriginManifestFetchContext,
    generation: Option<HlsManifestAcceptanceGeneration>,
    current_burst_is_full_plan: bool,
    completed_candidates: usize,
) {
    if !current_burst_is_full_plan {
        return;
    }
    let Some(generation) = generation else {
        return;
    };
    let mut session = context.session.write().await;
    let Some(episode) = session.origin_control.acceptance_episode.as_mut() else {
        return;
    };
    if episode.generation == generation {
        episode.record_full_burst_candidates(completed_candidates);
    }
}

pub(super) async fn update_manifest_acceptance_episode_state(
    context: &HlsOriginManifestFetchContext,
    generation: Option<HlsManifestAcceptanceGeneration>,
    state: HlsManifestAcceptanceState,
) {
    let Some(generation) = generation else {
        return;
    };
    let mut session = context.session.write().await;
    let Some(episode) = session.origin_control.acceptance_episode.as_mut() else {
        return;
    };
    if episode.generation == generation {
        episode.state = state;
    }
}

pub(super) async fn hold_uncommitted_manifest_acceptance_episode(
    context: &HlsOriginManifestFetchContext,
    generation: Option<HlsManifestAcceptanceGeneration>,
    held_alternative: Option<HlsAlternativeOriginCohort>,
    next_retry_at_ms: u64,
) {
    let Some(generation) = generation else {
        return;
    };
    let mut session = context.session.write().await;
    let Some(episode) = session.origin_control.acceptance_episode.as_mut() else {
        return;
    };
    if episode.generation == generation {
        episode.hold_after_uncommitted_burst(held_alternative, Some(next_retry_at_ms));
        session.origin_control.path_condition = crate::origin_progress::HlsOriginPathCondition::AcceptanceConflict;
    }
}

pub(super) async fn complete_manifest_acceptance_episode(
    context: &HlsOriginManifestFetchContext,
    generation: Option<HlsManifestAcceptanceGeneration>,
) {
    let Some(generation) = generation else {
        return;
    };
    let mut session = context.session.write().await;
    let Some(episode) = session.origin_control.acceptance_episode.as_mut() else {
        return;
    };
    if episode.generation == generation {
        episode.complete();
    }
}

pub(super) fn manifest_acceptance_exhaustion_reason(
    observations: &[HlsManifestCandidateObservation],
) -> HlsManifestAcceptanceExhaustionReason {
    if observations.is_empty() {
        return HlsManifestAcceptanceExhaustionReason::AllFailed;
    }
    if observations.iter().all(|candidate| {
        candidate.host_relation == HlsCandidateHostRelation::PinnedHost
            && matches!(
                candidate.local_sequence_relation,
                Some(HlsHostLocalSequenceRelation::Same | HlsHostLocalSequenceRelation::Backward)
            )
    }) {
        HlsManifestAcceptanceExhaustionReason::NoProgress
    } else {
        HlsManifestAcceptanceExhaustionReason::NoCommittableCandidate
    }
}

pub(super) async fn record_manifest_acceptance_exhaustion(
    context: &HlsOriginManifestFetchContext,
    generation: Option<HlsManifestAcceptanceGeneration>,
    reason: HlsManifestAcceptanceExhaustionReason,
) {
    let Some(generation) = generation else {
        return;
    };
    let mut session = context.session.write().await;
    let Some(episode) = session.origin_control.acceptance_episode.as_mut() else {
        return;
    };
    if episode.generation == generation
        && episode.exhaustion_reason().is_none()
        && episode.deterministic_conflict_receipt().is_none()
    {
        episode.record_exhaustion(reason);
    }
}

pub(super) fn manifest_candidate_observation(
    candidate: &HlsManifestRecoveryCandidate,
    burst_plan: HlsManifestRecoveryBurstPlan,
    resources: &HlsCommittedResourceEvidence,
) -> HlsManifestCandidateObservation {
    let session_key = &resources.session_key;
    let host_relation = match candidate.report.quality.host_relation {
        HlsManifestOriginRelation::SameRedirectHost => HlsCandidateHostRelation::PinnedHost,
        HlsManifestOriginRelation::OtherRedirectHost => HlsCandidateHostRelation::OtherHost,
        HlsManifestOriginRelation::Initial => HlsCandidateHostRelation::InitialBaseline,
        HlsManifestOriginRelation::UnknownHost => HlsCandidateHostRelation::Unknown,
    };
    let HlsManifestCandidateFingerprint {
        timeline: timeline_fingerprint,
        has_switch_segment,
        emergency_evidence,
        target_duration_secs,
    } = build_manifest_timeline_fingerprint(
        session_key,
        &candidate.fetched.body,
        &candidate.fetched.final_manifest_url,
    );
    let mut resource_timeline_evidence = manifest_occurrence_timeline_evidence(
        session_key,
        &candidate.fetched.body,
        &candidate.fetched.final_manifest_url,
        resources,
    );
    if is_same_host_reset_candidate(&candidate.report.quality)
        && matches!(
            resource_timeline_evidence,
            HlsResourceTimelineEvidence::ReplayOnly | HlsResourceTimelineEvidence::ContradictoryOrder
        )
        && has_switch_segment
    {
        // A permitted re-entry must reach content staging even when the origin
        // reused every URI. Only the subsequent fresh byte probe can distinguish
        // a restart from old media; this is no media-progress evidence itself.
        resource_timeline_evidence = HlsResourceTimelineEvidence::Eligible;
    }
    let committed_content_anchor = resources
        .anchor
        .as_ref()
        .filter(|anchor| {
            let tuliprox_parser::hls::origin_manifest::OriginManifestParseOutcome::Normal(manifest) =
                tuliprox_parser::hls::origin_manifest::parse_origin_media_manifest(
                    &candidate.fetched.body,
                    &candidate.fetched.final_manifest_url,
                )
            else {
                return false;
            };
            manifest
                .segments
                .iter()
                .find(|segment| segment.origin_seq == anchor.origin_key.host_local_sequence)
                .is_some_and(|segment| {
                    segment.map_ref.is_none()
                        && segment.encryption.is_none()
                        && anchor.identity.same_path_and_range(HlsMediaResourceIdentity::from_url(
                            &segment.resource_uri,
                            segment.origin_byte_range,
                        ))
                })
        })
        .filter(|_| {
            emergency_evidence.live_handoff == HlsEmergencyLiveHandoffCompatibility::RequiresStagedTrackVerification
        })
        .map_or(HlsCommittedContentAnchorEvidence::Unavailable, |_| {
            HlsCommittedContentAnchorEvidence::RequiresStagedByteVerification
        });
    HlsManifestCandidateObservation {
        candidate_index: candidate.candidate_index,
        candidate_slot: burst_plan.slot_for_candidate(candidate.candidate_index),
        effective_host: candidate.report.quality.effective_host.clone(),
        host_relation,
        host_local_media_sequence: candidate.report.media_sequence,
        host_local_highwater: candidate.report.quality.origin_highwater,
        local_sequence_relation: matches!(
            host_relation,
            HlsCandidateHostRelation::PinnedHost | HlsCandidateHostRelation::InitialBaseline
        )
        .then(|| {
            classify_host_local_sequence(
                candidate.report.quality.previous_highwater,
                candidate.report.quality.origin_highwater,
                candidate.report.quality.allowed_forward_window.unwrap_or(1),
                candidate.report.quality.sequence_relation == HlsManifestSequenceRelation::Rebase,
            )
        })
        .flatten(),
        resource_timeline_evidence,
        timeline_fingerprint,
        target_duration_secs,
        manifest_fetch_elapsed_ms: candidate.fetch_elapsed_ms,
        switch_segment_readiness: if has_switch_segment {
            HlsSwitchSegmentReadiness::RequiresStaging
        } else {
            HlsSwitchSegmentReadiness::Unavailable
        },
        committed_content_anchor,
        emergency_evidence,
        evidence: HlsCrossHostAcceptanceEvidence::Insufficient,
    }
}

pub(super) struct HlsCommittedResourceEvidence {
    pub(super) session_key: crate::HlsSessionKey,
    pub(super) anchor: Option<crate::refresh::origin_probe::HlsOriginContentAnchor>,
    pub(super) known_identities: Vec<HlsSegmentOccurrenceIdentity>,
    pub(super) known_entries: Vec<(HlsSegmentOccurrenceIdentity, u64)>,
    verified_bindings: Vec<HlsVerifiedOccurrenceBinding>,
    verifiable_identities: Vec<HlsSegmentOccurrenceIdentity>,
    pub(super) previous_proxy_tail: Option<u64>,
    pub(super) origin_progress_generation: u64,
    pub(super) published_resource_history_generation: u64,
    pub(super) pinned_host_generation: u64,
}

pub(super) async fn committed_resource_evidence(
    context: &HlsOriginManifestFetchContext,
) -> HlsCommittedResourceEvidence {
    HlsCommittedResourceEvidence::capture(&*context.session.read().await)
}

impl HlsCommittedResourceEvidence {
    pub(super) fn capture(session: &crate::HlsSession) -> Self {
        let anchor = crate::refresh::origin_probe::HlsOriginContentEvidence::latest_ready(session);
        let floor = session.published_resource_history.restart_proxy_floor();
        let mut known_entries = session
            .segments
            .values()
            .filter(|entry| entry.proxy_seq >= floor)
            .filter_map(|entry| entry.occurrence_identity.map(|identity| (identity, entry.proxy_seq)))
            .collect::<Vec<_>>();
        for (identity, seq) in session.published_resource_history.recent_entries(usize::MAX) {
            if !known_entries.iter().any(|(known, _)| known.same_position(identity)) {
                known_entries.push((identity, seq));
            }
        }
        let known_identities = known_entries.iter().map(|(identity, _)| *identity).collect();
        let verifiable_identities = session
            .segments
            .values()
            .filter(|entry| {
                entry.proxy_seq >= floor
                    && crate::refresh::origin_probe::HlsOriginContentEvidence::has_complete_reference(session, entry)
            })
            .filter_map(|entry| entry.occurrence_identity)
            .collect();
        Self {
            session_key: session.playlist_source_key().into_owned(),
            anchor,
            known_identities,
            known_entries,
            verified_bindings: crate::refresh::origin_probe::HlsOriginContentEvidence::verified_bindings(session),
            verifiable_identities,
            previous_proxy_tail: session.proxy_next_seq.and_then(|next| next.checked_sub(1)),
            origin_progress_generation: session.origin_control.progress_generation,
            published_resource_history_generation: session.published_resource_history.generation(),
            pinned_host_generation: session.origin_epoch,
        }
    }

    pub(super) fn binding_matches(
        &self,
        proxy_seq: u64,
        original: HlsSegmentOccurrenceIdentity,
        candidate: HlsSegmentOccurrenceIdentity,
    ) -> bool {
        original == candidate
            || self.verified_bindings.iter().any(|binding| binding.matches(proxy_seq, original, candidate))
    }

    fn timeline_evidence(
        &self,
        identities: impl IntoIterator<Item = HlsSegmentOccurrenceIdentity>,
    ) -> HlsResourceTimelineEvidence {
        let mut requires_verification = false;
        let canonical = identities.into_iter().map(|identity| {
            let Some(&(original, seq)) = self.known_entries.iter().find(|(known, _)| known.same_position(identity))
            else {
                return identity;
            };
            if self.binding_matches(seq, original, identity) {
                return original;
            }
            if self.verifiable_identities.contains(&original) {
                // This only permits the explicit recovery verifier to run. No
                // equality or continuity is established until its full hash commit.
                requires_verification = true;
                return original;
            }
            identity
        });
        let evidence = occurrence_timeline_evidence(canonical, &self.known_identities);
        if requires_verification && evidence == HlsResourceTimelineEvidence::ReplayOnly {
            HlsResourceTimelineEvidence::Eligible
        } else {
            evidence
        }
    }
}

#[cfg(test)]
pub(super) fn candidate_resource_timeline_evidence(
    fingerprint: &crate::manifest_acceptance::HlsManifestTimelineFingerprint,
    published: &[HlsSegmentOccurrenceIdentity],
) -> HlsResourceTimelineEvidence {
    occurrence_timeline_evidence(
        fingerprint.segment_samples.iter().filter_map(|segment| segment.occurrence_identity),
        published,
    )
}

pub(super) fn occurrence_timeline_evidence(
    identities: impl IntoIterator<Item = HlsSegmentOccurrenceIdentity>,
    published: &[HlsSegmentOccurrenceIdentity],
) -> HlsResourceTimelineEvidence {
    let mut saw_published = false;
    let mut saw_new = false;
    for identity in identities {
        let existing = published.iter().find(|existing| existing.same_position(identity));
        if existing.is_some_and(|existing| *existing != identity) {
            return HlsResourceTimelineEvidence::ContradictoryOrder;
        }
        let was_published = existing.is_some();
        if was_published && saw_new {
            return HlsResourceTimelineEvidence::ContradictoryOrder;
        }
        saw_published |= was_published;
        saw_new |= !was_published;
    }
    if saw_published && !saw_new {
        HlsResourceTimelineEvidence::ReplayOnly
    } else {
        HlsResourceTimelineEvidence::Eligible
    }
}

fn manifest_occurrence_timeline_evidence(
    session_key: &crate::HlsSessionKey,
    body: &str,
    final_manifest_url: &str,
    resources: &HlsCommittedResourceEvidence,
) -> HlsResourceTimelineEvidence {
    match tuliprox_parser::hls::origin_manifest::parse_origin_media_manifest(body, final_manifest_url) {
        tuliprox_parser::hls::origin_manifest::OriginManifestParseOutcome::Normal(manifest) => {
            let scope = crate::resource_identity::HlsMediaPlaylistScope::from_session(session_key);
            resources.timeline_evidence(
                manifest
                    .segments
                    .iter()
                    .filter_map(|segment| scope.map(|scope| HlsSegmentOccurrenceIdentity::from_parsed(scope, segment))),
            )
        }
        tuliprox_parser::hls::origin_manifest::OriginManifestParseOutcome::TransientPassthrough { .. } => {
            HlsResourceTimelineEvidence::Eligible
        }
    }
}
