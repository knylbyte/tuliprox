use super::*;
use crate::{refresh::continuity::HlsCacheOriginAnchor, HlsSession, SegmentCacheStatus, SegmentFetchPriority};
use std::{
    fmt::Write as _,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tokio::sync::oneshot;
use tuliprox_parser::hls::origin_manifest::{
    parse_origin_media_manifest, OriginManifestParseOutcome, ParsedOriginManifest,
};

mod leases;
mod transport;

fn manifest(base: &str, first: u64, last: u64) -> FetchedOriginManifest {
    let mut body = format!("#EXTM3U\n#EXT-X-VERSION:4\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:{first}\n");
    for msn in first..=last {
        writeln!(body, "#EXTINF:4,\n/media/{msn}.ts?token=a").unwrap();
    }
    let mut fetched = fetched_manifest(&body);
    fetched.final_manifest_url = format!("{base}/index.m3u8");
    fetched.resolved_request_url.clone_from(&fetched.final_manifest_url);
    fetched.redirect_host = Some(url::Url::parse(base).unwrap().host_str().unwrap().to_owned());
    fetched
}

fn parsed(fetched: &FetchedOriginManifest) -> ParsedOriginManifest {
    let OriginManifestParseOutcome::Normal(parsed) =
        parse_origin_media_manifest(&fetched.body, &fetched.final_manifest_url)
    else {
        panic!("normal fixture");
    };
    parsed
}

async fn established(base: &str) -> OriginRefreshRequest {
    let session = test_session();
    let mut request = test_origin_refresh_request(Arc::clone(&session));
    request.now_ms = tuliprox_core::utils::current_time_millis();
    request.origin_entry = LiveHlsOriginEntry::parse(&format!("{base}/index.m3u8")).unwrap();
    let mut state = session.write().await;
    commit_fetched_manifest(&mut state, &manifest(base, 101, 104), &request, request.now_ms).unwrap();
    for seq in 0..3 {
        state.segments.get_mut(&seq).unwrap().status = ready();
    }
    state.render_and_store_manifest(request.now_ms).unwrap();
    state.segments.get_mut(&2).unwrap().status = fetching();
    state.segments.get_mut(&3).unwrap().status = SegmentCacheStatus::Discovered;
    state.mark_authorized_media_access(request.now_ms);
    drop(state);
    request
}

fn ready() -> SegmentCacheStatus { SegmentCacheStatus::Ready { content_length: 1, ready_at_ms: 100 } }
fn fetching() -> SegmentCacheStatus {
    SegmentCacheStatus::Fetching { priority: SegmentFetchPriority::Demand, started_at_ms: 100 }
}

#[tokio::test]
async fn newest_known_anchor_uses_saved_identity_in_every_download_state() {
    let request = established("https://a.example").await;
    let mut state = request.session.write().await;
    let expected = state.segments[&3].occurrence_identity.unwrap();
    for status in [SegmentCacheStatus::Discovered, fetching(), ready()] {
        state.segments.get_mut(&3).unwrap().status = status;
        state.segments.get_mut(&0).unwrap().status = ready();
        state.segments.get_mut(&3).unwrap().origin_fetch_ref.as_mut().unwrap().resolved_origin_url =
            "https://rebound.example/unrelated.ts?changed=1".into();
        state.segments.get_mut(&3).unwrap().origin_fetch_ref.as_mut().unwrap().byte_range =
            Some(tuliprox_parser::hls::origin_manifest::ParsedByteRange { offset: 999, length: 100 });
        let anchor = HlsCacheOriginAnchor::capture(&state).unwrap();
        assert_eq!((anchor.proxy_seq, anchor.identity), (3, expected));
        assert!(anchor.validate(&state, &parsed(&manifest("https://b.example", 103, 104))).is_ok());
    }
    state.apply_origin_manifest(&parsed(&manifest("https://a.example", 104, 105))).unwrap();
    for status in [SegmentCacheStatus::Discovered, fetching(), ready()] {
        state.segments.get_mut(&4).unwrap().status = status;
        let anchor = HlsCacheOriginAnchor::capture(&state).unwrap();
        assert_eq!(anchor.proxy_seq, 4);
        assert_eq!(anchor.identity, state.segments[&4].occurrence_identity.unwrap());
        assert_eq!(anchor.identity.origin_msn(), 105);
    }
}

#[tokio::test]
async fn missing_successor_is_distinct_from_missing_anchor_and_conflicting_binding() {
    let request = established("https://a.example").await;
    let state = request.session.read().await;
    let anchor = HlsCacheOriginAnchor::capture(&state).unwrap();
    let mut candidate = parsed(&manifest("https://a.example", 103, 106));
    candidate.segments.last_mut().unwrap().origin_seq = 108;
    let reason = anchor.validate(&state, &candidate).unwrap_err();
    assert_eq!(
        reason,
        HlsManifestRejectLogReason::OriginCacheSuccessorMissing { previous_origin_seq: 105, candidate_origin_seq: 108 }
    );
    assert!(reason.status_label().starts_with("origin-cache-successor-missing"));
}

#[tokio::test]
async fn cache_anchor_uses_proxy_order_and_excludes_local_objects_and_old_restart_namespace() {
    let request = established("https://a.example").await;
    let mut state = request.session.write().await;
    let previous = HlsCacheOriginAnchor::capture(&state).unwrap();
    let mut local = state.segments[&3].clone();
    local.proxy_seq = 100;
    local.occurrence_identity = None;
    local.origin_fetch_ref = None;
    local.retained_resource_identity = None;
    state.segments.insert(100, local);
    assert_eq!(HlsCacheOriginAnchor::capture(&state).unwrap().proxy_seq, 3);
    state.segments.remove(&100);
    let restart = parsed(&manifest("https://a.example", 0, 2));
    let host = crate::timeline::effective_origin_host_id("a.example");
    let preview = state.preview_verified_restart(&restart, host).unwrap();
    state.apply_verified_restart(&restart, host, &preview).unwrap();
    assert_eq!(previous.validate(&state, &restart), Err(HlsManifestRejectLogReason::StagedSwitchInvalidated));
    let anchor = HlsCacheOriginAnchor::capture(&state).unwrap();
    assert_eq!((anchor.proxy_seq, anchor.identity.origin_msn()), (6, 2));
    state.segments.get_mut(&3).unwrap().status = ready();
    assert_eq!(HlsCacheOriginAnchor::capture(&state), Some(anchor));
}

#[tokio::test]
async fn discovered_anchor_accepts_successors_and_unchanged_tail_without_ready_proof() {
    for (last, absolute) in [(104, false), (104, true), (106, false), (106, true)] {
        let request = established("https://a.example").await;
        {
            let mut state = request.session.write().await;
            state.segments.remove(&0);
            state.origin_to_proxy.retain(|_, seq| *seq != 0);
        }
        let epoch = request.session.read().await.origin_epoch;
        let mut candidate = manifest("https://b.example", 103, last);
        candidate.final_manifest_url = "https://b.example/redirect/new.m3u8?manifest=2".into();
        if absolute {
            candidate.body = candidate.body.replace("/media/", "http://media-b.example:8443/media/");
        }
        let result = super::super::origin_probe::commit_pinned_manifest(
            &request,
            &candidate,
            HlsManifestCommitAcceptanceMode::StrictPinnedHost,
        )
        .await;
        assert!(result.is_ok(), "{:?}", result.err());
        let state = request.session.read().await;
        assert_eq!(state.proxy_next_seq, Some(last - 100));
        assert_eq!(state.segments[&3].origin_key.host_local_sequence, 104);
        assert!(!matches!(state.segments[&3].status, SegmentCacheStatus::Ready { .. }));
        assert_eq!(state.origin_epoch, epoch);
        assert!(state.origin_control.acceptance_episode.is_none());
        assert!(state.segments.values().all(|entry| !entry.discontinuity_before));
    }
}

#[tokio::test]
async fn missing_latest_anchor_and_forward_jump_reach_existing_recovery_owner() {
    for (first, last, unpublished) in [(101, 103, false), (105, 108, false), (101, 103, true), (105, 108, true)] {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        let body = manifest("http://fixture.example", first, last).body;
        let origin = spawn_test_origin(Arc::new(move |_| {
            if counter.fetch_add(1, Ordering::AcqRel) == 0 {
                (200, Vec::new(), body.clone())
            } else {
                (503, Vec::new(), String::new())
            }
        }))
        .await;
        let mut request = established(&origin.base_url).await;
        if unpublished {
            let mut state = request.session.write().await;
            state.last_rendered_manifest = None;
            state.published_live_origin_baseline = None;
            state.published_resource_history = crate::resource_identity::HlsPublishedResourceHistory::default();
            for entry in state.segments.values_mut() {
                entry.status = SegmentCacheStatus::Discovered;
            }
            assert!(state.established_manifest_recovery_binding().is_none());
        }
        let before = request.session.read().await.proxy_next_seq;
        let anchor = HlsCacheOriginAnchor::capture(&*request.session.read().await).unwrap();
        assert!(matches!(
            anchor.validate(&*request.session.read().await, &parsed(&manifest(&origin.base_url, first, last))),
            Err(HlsManifestRejectLogReason::OriginCacheAnchorMissing { anchor_proxy_seq: 3, anchor_origin_seq: 104 })
        ));
        assert!(fetch_and_commit_manifest_with_policy(&mut request).await.is_err());
        let state = request.session.read().await;
        assert_eq!(state.proxy_next_seq, before, "no silent forward commit");
        let episode = state.origin_control.acceptance_episode.as_ref().expect("existing recovery owner");
        assert_eq!(episode.full_bursts_completed, 1);
        assert!(calls.load(Ordering::Acquire) > 1);
    }
}

#[tokio::test]
async fn expired_reentry_keeps_the_existing_byte_verification_gate_before_normal_commit() {
    let request = established("https://a.example").await;
    let mut request = request;
    request.manifest_commit_requirement = HlsManifestCommitRequirement::FreshCommitRequired {
        reason: crate::HlsFreshManifestRequiredReason::StaleEntryWindow,
    };
    {
        let mut state = request.session.write().await;
        state.activity.last_authorized_media_at_ms = Some(request.now_ms.saturating_sub(300_000));
        assert!(matches!(
            state.account_binding_protection(request.now_ms),
            crate::HlsAccountBindingProtection::Expired
        ));
    }
    let result = crate::refresh::origin_probe::commit_pinned_manifest(
        &request,
        &manifest("https://a.example", 101, 104),
        HlsManifestCommitAcceptanceMode::StrictPinnedHost,
    )
    .await;
    assert!(matches!(
        result,
        Err(HlsManifestCommitError::TimelineRejected {
            reason: HlsManifestRejectLogReason::OriginCacheRevalidationRequired { anchor_proxy_seq: 3 }
        })
    ));
    assert_eq!(request.session.read().await.proxy_next_seq, Some(4));
}

#[tokio::test]
async fn ordinary_query_path_and_range_conflicts_reject_before_any_probe_or_timeline_change() {
    let origin = spawn_test_origin(Arc::new(|_| (500, Vec::new(), String::new()))).await;
    for kind in ["path", "query", "earlier", "offset", "length", "presence"] {
        let request = established(&origin.base_url).await;
        let mut baseline = manifest(&origin.base_url, 101, 104);
        if matches!(kind, "offset" | "length" | "presence") {
            baseline.body = baseline.body.replace("#EXTINF:4,", "#EXTINF:4,\n#EXT-X-BYTERANGE:188@0");
            let mut state = request.session.write().await;
            *state = HlsSession::new(state.key.clone(), b"secret", request.now_ms);
            commit_fetched_manifest(&mut state, &baseline, &request, request.now_ms).unwrap();
            state.segments.get_mut(&3).unwrap().status = SegmentCacheStatus::Discovered;
        }
        let mut candidate = baseline.clone();
        candidate.final_manifest_url = format!("{}/redirect/rotated.m3u8?manifest=next", origin.base_url);
        match kind {
            "path" => candidate.body = candidate.body.replace("/media/104.ts", "/different/104.ts"),
            "query" => candidate.body = candidate.body.replace("104.ts?token=a", "104.ts?token=b"),
            "earlier" => candidate.body = candidate.body.replace("101.ts?token=a", "101.ts?token=b"),
            "offset" => candidate.body = candidate.body.replace("188@0\n/media/104.ts", "188@1\n/media/104.ts"),
            "length" => candidate.body = candidate.body.replace("188@0\n/media/104.ts", "187@0\n/media/104.ts"),
            "presence" => {
                candidate.body = candidate.body.replace("#EXT-X-BYTERANGE:188@0\n/media/104.ts", "/media/104.ts");
            }
            _ => unreachable!(),
        }
        let before = {
            let state = request.session.read().await;
            (
                state.segments.clone(),
                state.origin_to_proxy.clone(),
                state.published_resource_history.clone(),
                state.next_media_start_ms,
                state.origin_manifest_commits(),
            )
        };
        let result = super::super::origin_probe::commit_pinned_manifest(
            &request,
            &candidate,
            HlsManifestCommitAcceptanceMode::StrictPinnedHost,
        )
        .await;
        let Err(HlsManifestCommitError::TimelineRejected { reason }) = result else { panic!("conflict {kind}") };
        assert!(matches!(reason, HlsManifestRejectLogReason::OriginSequenceResourceConflict { .. }));
        let state = request.session.read().await;
        assert_eq!(
            (
                state.segments.clone(),
                state.origin_to_proxy.clone(),
                state.published_resource_history.clone(),
                state.next_media_start_ms,
                state.origin_manifest_commits()
            ),
            before
        );
    }
    assert!(origin.requests.lock().await.is_empty(), "normal continuity never performs equality downloads");
}

#[tokio::test]
async fn captured_anchor_is_rechecked_after_newer_discovery_but_not_invalidated_by_readiness() {
    let request = established("https://a.example").await;
    let shared = Arc::clone(&request.session);
    let anchor = HlsCacheOriginAnchor::capture(&*request.session.read().await).unwrap();
    let candidate = manifest("https://b.example", 103, 104);
    for status in [fetching(), ready()] {
        let mut state = shared.write().await;
        state.segments.get_mut(&3).unwrap().status = status;
        assert_eq!(HlsCacheOriginAnchor::capture(&state), Some(anchor));
        assert!(anchor.validate(&state, &parsed(&candidate)).is_ok());
    }
    let (checked_tx, checked_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let session = Arc::clone(&request.session);
    let task = tokio::spawn(async move {
        assert!(anchor.validate(&*session.read().await, &parsed(&candidate)).is_ok());
        checked_tx.send(()).unwrap();
        release_rx.await.unwrap();
        let mut state = session.write().await;
        assert_eq!(
            anchor.validate(&state, &parsed(&candidate)),
            Err(HlsManifestRejectLogReason::StagedSwitchInvalidated)
        );
        commit_fetched_manifest(&mut state, &candidate, &request, request.now_ms)
    });
    checked_rx.await.unwrap();
    shared.write().await.apply_origin_manifest(&parsed(&manifest("https://a.example", 104, 105))).unwrap();
    release_tx.send(()).unwrap();
    assert!(matches!(
        task.await.unwrap(),
        Err(HlsManifestCommitError::TimelineRejected {
            reason: HlsManifestRejectLogReason::OriginCacheAnchorMissing {
                anchor_proxy_seq: 4,
                anchor_origin_seq: 105
            }
        })
    ));
}

mod rollover;
