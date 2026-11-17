use super::recovery_entry::recovery_entry_segment_ready;
use crate::{
    manifest_commit::{
        hls_cached_manifest_options_for_requirement, hls_committed_manifest_body_for_request,
        hls_live_window_origin_allowed_for_request, manifest_commit_requirement, HlsCommittedManifestBody,
    },
    media_reserve::{evaluate_lease_reserve, HlsLeaseManifestSnapshot, HlsLeaseReserveInput, HlsManifestDeliveryMode},
    playback::HlsAccessContext,
    session_store::{HlsCurrentProxySessionGuard, HlsSessionIncarnation},
    HlsAccessLease, HlsAccessLeaseId, HlsAccessLeaseState, HlsFreshManifestRequiredReason, HlsManifestRenderer,
    HlsMediaLeaseIdentity, HlsProxyManager, HlsSession, HlsSessionHandle, HlsSessionMode, HlsSessionStoreOutcome,
    ProxySessionId,
};
use std::{sync::Arc, time::Duration};

pub enum HlsRecoveryPoll {
    Regular,
    Unavailable,
    Terminal(Box<HlsAccessLease>),
    Retained { body: Arc<str>, origin: Option<HlsRetainedPollOrigin> },
}

/// Authorization to start recovery work requested by a retained manifest poll.
/// Once started, the refresh belongs to the shared session.
#[derive(Clone)]
pub struct HlsRetainedPollOrigin {
    session: HlsSessionIncarnation,
    proxy_id: ProxySessionId,
    lease_id: HlsAccessLeaseId,
    lease_identity: HlsMediaLeaseIdentity,
    recovery_from: HlsRetainedPollReason,
    origin_epoch: u64,
    progress_generation: u64,
}

#[derive(Clone, Copy)]
enum HlsRetainedPollReason {
    Entry407(u64),
    MemberHandoff(u64),
}

pub struct HlsRetainedPollOriginGuard<'a> {
    _current: HlsCurrentProxySessionGuard<'a>,
    leases: tokio::sync::RwLockReadGuard<'a, crate::HlsAccessLeaseStore>,
    origin: &'a HlsRetainedPollOrigin,
}

impl HlsRetainedPollOrigin {
    pub fn lease_id(&self) -> &HlsAccessLeaseId { &self.lease_id }
    pub fn proxy_id(&self) -> &ProxySessionId { &self.proxy_id }

    pub(crate) fn allows_start(&self, leases: &crate::HlsAccessLeaseStore, state: &HlsSession, now_ms: u64) -> bool {
        state.proxy_session_id == self.proxy_id
            && !state.is_gc_marked_for_removal()
            && match self.recovery_from {
                HlsRetainedPollReason::Entry407(from) => state.origin_control.entry_407_recovery_from == Some(from),
                HlsRetainedPollReason::MemberHandoff(start) => {
                    state.group_member_start_proxy_seq == Some(start) && !state.group_member_handoff_pending()
                }
            }
            && state.origin_epoch == self.origin_epoch
            && state.origin_control.progress_generation == self.progress_generation
            && state.origin_refresh.is_due(now_ms)
            && leases.current_response_lease(&self.lease_id, &self.proxy_id, now_ms).is_some_and(|lease| {
                lease.state == HlsAccessLeaseState::Activated
                    && lease.active_until_ms.is_some_and(|until| until > now_ms)
                    && lease.runtime_policy_revocation.is_none()
                    && lease.playback_mode == crate::terminal_tail::HlsLeasePlaybackMode::Live
                    && lease.media_identity() == Some(self.lease_identity)
                    && (matches!(self.recovery_from, HlsRetainedPollReason::Entry407(_))
                        || lease
                            .last_manifest_snapshot
                            .as_ref()
                            .is_some_and(|snapshot| state.retains_prior_group_member(snapshot)))
            })
    }

    pub(crate) fn matches_session(&self, manager: &HlsProxyManager, session: &HlsSessionHandle) -> bool {
        manager.sessions.session_incarnation(session) == Some(self.session)
    }
}

impl HlsRetainedPollOriginGuard<'_> {
    pub fn allows_start(&self, state: &HlsSession, now_ms: u64) -> bool {
        self.origin.allows_start(&self.leases, state, now_ms)
    }

    pub(crate) fn has_session_demand(&self, now_ms: u64) -> bool {
        self.leases.has_current_origin_work_lease(&self.origin.proxy_id, now_ms)
    }
}

impl HlsProxyManager {
    pub(crate) async fn retained_poll_has_session_demand(
        &self,
        session: &HlsSessionHandle,
        origin: &HlsRetainedPollOrigin,
    ) -> bool {
        let Some(guard) = self.lock_retained_poll_origin(session, origin).await else { return false };
        guard.has_session_demand(self.now_ms())
    }

    pub async fn lock_retained_poll_origin<'a>(
        &'a self,
        session: &HlsSessionHandle,
        origin: &'a HlsRetainedPollOrigin,
    ) -> Option<HlsRetainedPollOriginGuard<'a>> {
        let current = self.sessions.hold_current_proxy_session(&origin.proxy_id).await?;
        if !Arc::ptr_eq(current.session(), session) || !origin.matches_session(self, session) {
            return None;
        }
        Some(HlsRetainedPollOriginGuard { _current: current, leases: self.access_leases.read().await, origin })
    }

    pub async fn release_unused_retained_poll_origin(
        &self,
        session: &HlsSessionHandle,
        origin: &HlsRetainedPollOrigin,
    ) {
        let Some(current) = self.sessions.hold_current_proxy_session(&origin.proxy_id).await else { return };
        if !Arc::ptr_eq(current.session(), session) || !origin.matches_session(self, session) {
            return;
        }
        let mut leases = self.access_leases.write().await;
        let mut state = session.write().await;
        crate::availability::binding_protection::release_unused_origin(&mut leases, &mut state, self.now_ms());
    }

    /// Reads the last published bytes under the session and lease locks.
    /// A poll here has no effect on the publication, cursor or entry fallback.
    pub async fn retained_recovery_poll(
        &self,
        session: &HlsSessionHandle,
        context: &HlsAccessContext,
        expected: &HlsAccessLease,
    ) -> HlsRecoveryPoll {
        let Some(current) = self.sessions.hold_current_proxy_session(&context.proxy_session_id).await else {
            return HlsRecoveryPoll::Unavailable;
        };
        if !Arc::ptr_eq(current.session(), session) {
            return HlsRecoveryPoll::Unavailable;
        }
        let leases = self.access_leases.read().await;
        let state = session.read().await;
        let now_ms = self.now_ms();
        let Some(lease) =
            leases.current_response_lease(&context.lease_id, &context.proxy_session_id, now_ms).filter(|lease| {
                lease.matches_manifest_response_authorization(expected)
                    && lease.username == context.username
                    && lease.user_session_token == context.user_session_token
                    && lease.input_id == context.input_id
                    && lease.stream_ref == context.stream_ref
                    && lease.virtual_id == context.virtual_id
                    && lease.family_key == context.family_key
                    && lease.epg_reference_ts == context.epg_reference_ts
                    && lease.archive_origin_url == context.archive_origin_url
            })
        else {
            return HlsRecoveryPoll::Unavailable;
        };
        if lease.playback_mode != crate::terminal_tail::HlsLeasePlaybackMode::Live {
            return HlsRecoveryPoll::Terminal(Box::new(lease.clone()));
        }
        if lease.state != HlsAccessLeaseState::Activated
            || lease.active_until_ms.is_none_or(|until| until <= now_ms)
            || lease.runtime_policy_revocation.is_some()
            || state.is_gc_marked_for_removal()
        {
            return HlsRecoveryPoll::Unavailable;
        }
        self.recovery_poll_body(&state, &leases, lease, now_ms).map_or(HlsRecoveryPoll::Regular, |body| {
            let reason =
                state.origin_control.entry_407_recovery_from.map(HlsRetainedPollReason::Entry407).or_else(|| {
                    state
                        .group_member_start_proxy_seq
                        .filter(|start| {
                            !state.group_member_handoff_pending()
                                && lease
                                    .last_manifest_snapshot
                                    .as_ref()
                                    .is_some_and(|snapshot| snapshot.last_proxy_seq < *start)
                        })
                        .map(HlsRetainedPollReason::MemberHandoff)
                });
            let origin = reason.filter(|_| state.origin_refresh.is_due(now_ms)).and_then(|recovery_from| {
                self.sessions.session_incarnation(session).zip(lease.media_identity()).map(
                    |(session, lease_identity)| HlsRetainedPollOrigin {
                        session,
                        proxy_id: context.proxy_session_id.clone(),
                        lease_id: context.lease_id.clone(),
                        lease_identity,
                        recovery_from,
                        origin_epoch: state.origin_epoch,
                        progress_generation: state.origin_control.progress_generation,
                    },
                )
            });
            HlsRecoveryPoll::Retained { body, origin }
        })
    }

    fn recovery_poll_body(
        &self,
        state: &HlsSession,
        leases: &crate::HlsAccessLeaseStore,
        lease: &HlsAccessLease,
        now_ms: u64,
    ) -> Option<Arc<str>> {
        let snapshot = lease.last_manifest_snapshot.as_ref()?;
        let source = snapshot.cached_source.as_ref()?;
        let group_handoff = state.group_member_handoff_pending() || state.retains_prior_group_member(snapshot);
        if (!group_handoff
            && (state.origin_control.entry_407_recovery_from.is_none()
                || !matches!(
                    state.fresh_manifest_commit_required,
                    None | Some(HlsFreshManifestRequiredReason::PreviousHardManifestFailure)
                )
                || state.pending_handoff_discontinuity_sequence.is_some()
                || state.origin_account_manifest_pending.is_some()
                || source.origin_work_generation != state.activity.origin_work_generation))
            || state.origin_playback_kind() != tuliprox_core::model::PlaybackKind::LiveHls
            || !matches!(state.mode, HlsSessionMode::NormalCacheTimeline)
            || snapshot.delivery_mode != HlsManifestDeliveryMode::NormalCacheTimeline
        {
            return None;
        }

        let publication = snapshot.startup_revisions.as_deref();
        if snapshot.visible_segments.is_empty()
            || snapshot.visible_segments.len() != source.bindings.len()
            || !snapshot.visible_segments.iter().zip(source.bindings.iter()).all(|(unit, binding)| {
                state.segments.get(&unit.proxy_seq).is_some_and(|entry| {
                    unit.duration_ms == entry.duration_ms
                        && binding.is_current(state, entry)
                        && publication.is_none_or(|published| {
                            published.revisions.get(&unit.proxy_seq).is_some_and(|revision| {
                                leases.published_segment_revision(&lease.lease_id, unit.proxy_seq).as_ref()
                                    == Some(revision)
                            })
                        })
                        && recovery_entry_segment_ready(&self.segment_cache, state, entry, publication, now_ms)
                })
            })
        {
            return None;
        }

        let cursor = &lease.playback_cursor;
        let mut timeline =
            state.ready_timeline_snapshot(cursor.ready_timeline_start_proxy_seq(snapshot.first_proxy_seq), now_ms);
        // A response can promise only the media advertised by these bytes.
        let end = timeline.units.partition_point(|unit| unit.proxy_seq <= snapshot.last_proxy_seq);
        if end < timeline.units.len() {
            timeline.units = Arc::from(&timeline.units[..end]);
        }
        let reserve = evaluate_lease_reserve(HlsLeaseReserveInput {
            manifest: snapshot,
            cursor,
            ready_timeline: &timeline,
            now_ms,
            recovery_trigger_budget: crate::recovery_timing::HlsRecoveryTriggerBudgetMs::from_millis(0),
            origin_path_degraded: true,
            recovery_committed: false,
        });
        if !reserve.playback_position_known
            || reserve.guaranteed_reserve_ms == 0
            || newer_playable_window(state, lease, snapshot, now_ms)
        {
            return None;
        }
        Some(Arc::clone(&source.body))
    }
}

impl HlsSession {
    pub fn has_retained_recovery_poll(&self) -> bool {
        self.origin_control.entry_407_recovery_from.is_some()
            || self.group_member_start_proxy_seq.is_some()
            || self.group_member_handoff_pending()
    }

    pub(crate) fn group_member_handoff_pending(&self) -> bool {
        self.key.is_group_key()
            && self.origin_account_runtime.as_ref().is_some_and(|runtime| runtime.member_handoff_pending())
    }

    pub(crate) fn retains_prior_group_member(&self, snapshot: &HlsLeaseManifestSnapshot) -> bool {
        self.group_member_start_proxy_seq.is_some_and(|start| snapshot.last_proxy_seq < start)
    }
}

fn newer_playable_window(
    session: &HlsSession,
    lease: &crate::HlsAccessLease,
    snapshot: &HlsLeaseManifestSnapshot,
    now_ms: u64,
) -> bool {
    let requirement = manifest_commit_requirement(session, HlsSessionStoreOutcome::Reused, None, false, false, now_ms);
    let options = hls_cached_manifest_options_for_requirement(
        Duration::ZERO,
        requirement,
        session.latest_manifest_rendered_at_ms(),
        session.origin_manifest_commits(),
    );
    if session.uses_fast_start_live_window() {
        return hls_live_window_origin_allowed_for_request(session, options, now_ms, now_ms)
            && session.plan_live_window_for_lease_snapshot(lease).is_some_and(|plan| {
                let Some(first) = plan.visible.first() else { return false };
                let Some(last) = plan.visible.last() else { return false };
                *first >= snapshot.first_proxy_seq
                    && *last >= snapshot.last_proxy_seq
                    && (*first > snapshot.first_proxy_seq || *last > snapshot.last_proxy_seq)
                    && plan.is_ready(session)
                    && HlsManifestRenderer::render_live_window(session, &plan, now_ms).is_ok()
            });
    }
    session.last_rendered_manifest.as_ref().is_some_and(|rendered| {
        rendered.first_proxy_seq >= snapshot.first_proxy_seq
            && rendered.last_proxy_seq >= snapshot.last_proxy_seq
            && (rendered.first_proxy_seq > snapshot.first_proxy_seq
                || rendered.last_proxy_seq > snapshot.last_proxy_seq)
            && matches!(
                hls_committed_manifest_body_for_request(session, options, now_ms, now_ms),
                Some(HlsCommittedManifestBody::Normal(_))
            )
    })
}
