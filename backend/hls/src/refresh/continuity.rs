//! Logical continuity of the shared origin timeline, independent of byte readiness and lease cursors.

use crate::{
    manifest_fetch::{FetchedOriginManifest, HlsManifestRejectLogReason},
    resource_identity::{HlsMediaPlaylistScope, HlsSegmentOccurrenceIdentity, HlsVerifiedOccurrenceBinding},
    HlsSession,
};
use tuliprox_parser::hls::origin_manifest::{
    parse_origin_media_manifest, OriginManifestParseOutcome, ParsedOriginManifest,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HlsCacheOriginAnchor {
    pub(crate) proxy_seq: u64,
    pub(crate) identity: HlsSegmentOccurrenceIdentity,
}

impl HlsCacheOriginAnchor {
    pub(crate) fn capture(session: &HlsSession) -> Option<Self> {
        let floor = session.published_resource_history.restart_proxy_floor();
        session
            .segments
            .range(floor..)
            .rev()
            .find_map(|(&proxy_seq, entry)| entry.occurrence_identity.map(|identity| Self { proxy_seq, identity }))
    }

    pub(crate) fn validate(
        self,
        session: &HlsSession,
        manifest: &ParsedOriginManifest,
    ) -> Result<(), HlsManifestRejectLogReason> {
        self.validate_with_bindings(session, manifest, &[])
    }

    pub(super) fn is_present(self, session: &HlsSession, manifest: &ParsedOriginManifest) -> bool {
        let Some(scope) = HlsMediaPlaylistScope::from_session(&session.playlist_source_key()) else { return false };
        let bindings = super::origin_probe::HlsOriginContentEvidence::verified_bindings(session);
        manifest.segments.iter().any(|segment| {
            let candidate = HlsSegmentOccurrenceIdentity::from_parsed(scope, segment);
            candidate == self.identity
                || bindings.iter().any(|binding| binding.matches(self.proxy_seq, self.identity, candidate))
        })
    }

    pub(super) fn validate_with_bindings(
        self,
        session: &HlsSession,
        manifest: &ParsedOriginManifest,
        additional: &[HlsVerifiedOccurrenceBinding],
    ) -> Result<(), HlsManifestRejectLogReason> {
        if Self::capture(session) != Some(self) {
            return Err(HlsManifestRejectLogReason::StagedSwitchInvalidated);
        }
        let scope = HlsMediaPlaylistScope::from_session(&session.playlist_source_key());
        if scope != Some(self.identity.playlist_scope()) {
            return Err(HlsManifestRejectLogReason::OriginPlaylistScopeChanged { anchor_proxy_seq: self.proxy_seq });
        }
        let scope = self.identity.playlist_scope();
        let bindings = super::origin_probe::HlsOriginContentEvidence::verified_bindings(session);
        let equivalent = |seq, original, candidate| {
            original == candidate
                || bindings.iter().chain(additional).any(|binding| binding.matches(seq, original, candidate))
        };
        let floor = session.published_resource_history.restart_proxy_floor();
        for segment in &manifest.segments {
            let identity = HlsSegmentOccurrenceIdentity::from_parsed(scope, segment);
            let binding = session
                .segments
                .range(floor..)
                .rev()
                .find_map(|(&seq, entry)| {
                    entry.occurrence_identity.filter(|known| known.same_position(identity)).map(|known| (known, seq))
                })
                .or_else(|| session.published_resource_history.binding_for(identity));
            if let Some((known, seq)) = binding.filter(|(known, seq)| !equivalent(*seq, *known, identity)) {
                debug_assert!(known.same_position(identity));
                return Err(HlsManifestRejectLogReason::OriginSequenceResourceConflict {
                    existing_proxy_seq: seq,
                    candidate_origin_seq: identity.origin_msn(),
                });
            }
        }
        let Some(position) = manifest.segments.iter().position(|segment| {
            equivalent(self.proxy_seq, self.identity, HlsSegmentOccurrenceIdentity::from_parsed(scope, segment))
        }) else {
            return Err(HlsManifestRejectLogReason::OriginCacheAnchorMissing {
                anchor_proxy_seq: self.proxy_seq,
                anchor_origin_seq: self.identity.origin_msn(),
            });
        };
        let mut previous = self.identity.origin_msn();
        for segment in &manifest.segments[position + 1..] {
            if previous.checked_add(1) != Some(segment.origin_seq) {
                return Err(HlsManifestRejectLogReason::OriginCacheSuccessorMissing {
                    previous_origin_seq: previous,
                    candidate_origin_seq: segment.origin_seq,
                });
            }
            previous = segment.origin_seq;
        }
        Ok(())
    }
}

pub(super) fn manifest_delivery_host(fetched: &FetchedOriginManifest) -> Option<String> {
    url::Url::parse(&fetched.final_manifest_url).ok().and_then(|url| url.host_str().map(str::to_owned))
}

pub(super) fn confirm_manifest_host(
    session: &mut HlsSession,
    fetched: &FetchedOriginManifest,
    manifest: &ParsedOriginManifest,
    anchor: HlsCacheOriginAnchor,
    now_ms: u64,
) {
    if session.segments.get(&anchor.proxy_seq).is_none_or(|entry| entry.origin_key.origin_epoch != session.origin_epoch)
    {
        return;
    }
    if let Some(host) = manifest_delivery_host(fetched) {
        let duration_ms = manifest.segments.iter().fold(0_u64, |sum, segment| sum.saturating_add(segment.duration_ms));
        session.origin_control.confirm_manifest_host(host, duration_ms, now_ms);
    }
}

pub(super) fn reject_rollover_replay_without_anchor(
    session: &HlsSession,
    manifest: &ParsedOriginManifest,
) -> Result<(), crate::manifest_fetch::HlsManifestCommitError> {
    if manifest.origin_manifest_sequence == 0
        && HlsCacheOriginAnchor::capture(session).is_some_and(|anchor| !anchor.is_present(session, manifest))
    {
        session.check_rollover_replay(manifest).map_err(|error| {
            crate::manifest_fetch::HlsManifestCommitError::TimelineRejected { reason: error.into() }
        })?;
    }
    Ok(())
}

pub(super) fn revoke_conflicting_manifest_host(
    session: &mut HlsSession,
    fetched: &FetchedOriginManifest,
    reason: &HlsManifestRejectLogReason,
) {
    if matches!(reason, HlsManifestRejectLogReason::OriginSequenceResourceConflict { .. }) {
        if let Some(host) = manifest_delivery_host(fetched) {
            session.origin_control.revoke_manifest_host(&host);
        }
    }
}

pub(crate) fn observe_recovery_anchor(
    session: &mut HlsSession,
    fetched: &FetchedOriginManifest,
    origin_epoch: u64,
    now_ms: u64,
) {
    if session.origin_epoch != origin_epoch || !matches!(session.mode, crate::HlsSessionMode::NormalCacheTimeline) {
        return;
    }
    let Some(anchor) = HlsCacheOriginAnchor::capture(session) else { return };
    let OriginManifestParseOutcome::Normal(manifest) =
        parse_origin_media_manifest(&fetched.body, &fetched.final_manifest_url)
    else {
        return;
    };
    match anchor.validate(session, &manifest) {
        Ok(()) => confirm_manifest_host(session, fetched, &manifest, anchor, now_ms),
        Err(reason) => {
            // A rejected zero-window replay does not invalidate the host's proof of the old anchor.
            if anchor.is_present(session, &manifest) || session.check_rollover_replay(&manifest).is_ok() {
                revoke_conflicting_manifest_host(session, fetched, &reason);
            }
        }
    }
}

pub(crate) fn idle_recovery_candidate(
    session: &HlsSession,
    fetched: &FetchedOriginManifest,
    now_ms: u64,
) -> Option<bool> {
    if !session.idle_for_origin_reentry(now_ms) || session.live_entry_revalidation.required {
        return None;
    }
    let anchor = HlsCacheOriginAnchor::capture(session)?;
    let OriginManifestParseOutcome::Normal(manifest) =
        parse_origin_media_manifest(&fetched.body, &fetched.final_manifest_url)
    else {
        return None;
    };
    let scope = HlsMediaPlaylistScope::from_session(&session.playlist_source_key())?;
    if scope != anchor.identity.playlist_scope() {
        return None;
    }
    if anchor.is_present(session, &manifest) {
        anchor.validate(session, &manifest).ok()?;
    } else {
        session.check_rollover_replay(&manifest).ok()?;
    }
    let host = manifest_delivery_host(fetched)?;
    Some(session.origin_control.manifest_host_confirmed(&host, now_ms))
}
