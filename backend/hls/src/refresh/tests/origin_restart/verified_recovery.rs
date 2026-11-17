use super::*;
use crate::{
    refresh::{
        continuity::HlsCacheOriginAnchor,
        origin_probe::{HlsOriginContentEvidence, OriginContentDigest},
    },
    SegmentCacheStatus,
};
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

mod hashes;
mod limits;

#[tokio::test]
async fn changed_path_and_transport_qualify_only_the_concrete_occurrence() {
    let origin = Origin::start().await;
    let request = ready_window(&origin).await;
    let original = request.session.read().await.segments[&3].clone();
    let revision = request.session.read().await.startup.as_ref().unwrap().revisions[&3].clone();
    let mut fetched = rotate_tail(window(&origin, 101, 4, "old"), "new");
    fetched.body = fetched.body.replace(
        &format!("{}/0123456789abcdef/104.ts?token=new", origin.base),
        &format!("{}/replacement/104.ts?token=new", origin.base.replace("127.0.0.1", "localhost")),
    );
    fetched.final_manifest_url = fetched.final_manifest_url.replace("127.0.0.1", "localhost");
    fetched.resolved_request_url.clone_from(&fetched.final_manifest_url);
    fetched.redirect_host = Some("localhost".into());
    origin.state.requests.lock().await.clear();
    let result = super::super::super::commit_manifest_recovery_candidate(
        &request,
        fetched.clone(),
        HlsManifestCommitAcceptanceMode::AllowHeldHostSwitchCandidate,
    )
    .await;
    assert!(result.is_ok(), "{:?}", result.err());
    assert_eq!(*origin.state.requests.lock().await, ["/replacement/104.ts?token=new"]);
    assert!(crate::refresh::origin_probe::commit_pinned_manifest(
        &request,
        &fetched,
        HlsManifestCommitAcceptanceMode::StrictPinnedHost
    )
    .await
    .is_ok());
    let state = request.session.read().await;
    assert_eq!(state.origin_epoch, 0);
    assert_eq!(state.segments[&3].origin_fetch_ref, original.origin_fetch_ref);
    assert_eq!(state.segments[&3].occurrence_identity, original.occurrence_identity);
    assert_eq!(state.startup.as_ref().unwrap().revisions[&3], revision);
    assert!(revision.revision().origin_binding_is_current(&state));
    assert_eq!(origin.state.requests.lock().await.len(), 1);
}

#[tokio::test]
async fn confirmed_binding_survives_collected_prefix_without_aliasing_another_msn() {
    let origin = Origin::start().await;
    let request = ready_window(&origin).await;
    let confirmed = rotate_tail(window(&origin, 101, 4, "old"), "new");
    assert!(candidate(&request, confirmed).await.is_ok());
    let advanced = rotate_tail(window(&origin, 101, 5, "old"), "new");
    crate::refresh::origin_probe::commit_pinned_manifest(
        &request,
        &advanced,
        HlsManifestCommitAcceptanceMode::StrictPinnedHost,
    )
    .await
    .unwrap();
    fetch(&request, 4).await;
    {
        let mut state = request.session.write().await;
        state.render_and_store_manifest(request.now_ms).unwrap();
        state.segments.retain(|seq, _| *seq >= 4);
        state.origin_to_proxy.retain(|_, seq| *seq >= 4);
    }
    let requests = origin.state.requests.lock().await.len();
    let repeated = rotate_tail(window(&origin, 104, 2, "old"), "new");
    crate::refresh::origin_probe::commit_pinned_manifest(
        &request,
        &repeated,
        HlsManifestCommitAcceptanceMode::StrictPinnedHost,
    )
    .await
    .unwrap();
    assert_eq!(origin.state.requests.lock().await.len(), requests);
    let mut next = window(&origin, 105, 2, "old");
    next.body = next.body.replace("106.ts?token=old", "104.ts?token=new");
    crate::refresh::origin_probe::commit_pinned_manifest(
        &request,
        &next,
        HlsManifestCommitAcceptanceMode::StrictPinnedHost,
    )
    .await
    .unwrap();
    let state = request.session.read().await;
    assert_eq!(state.proxy_next_seq, Some(6));
    assert_eq!(state.segments[&5].occurrence_identity.unwrap().origin_msn(), 106);
    assert!(!matches!(state.segments[&5].status, SegmentCacheStatus::Ready { .. }));
}
mod races;
mod selection;

#[tokio::test]
async fn verified_cross_host_reset_uses_its_complete_staging_and_rejects_replay() {
    for mode in [
        HlsManifestCommitAcceptanceMode::FreshPinnedRevalidation,
        HlsManifestCommitAcceptanceMode::AllowHeldHostSwitchCandidate,
    ] {
        for fresh in [false, true] {
            let origin = Origin::start().await;
            let mut request = ready_window(&origin).await;
            let old = request.session.read().await.segments[&3].clone();
            authorize_reentry(&mut request).await;
            {
                let mut state = request.session.write().await;
                let plan = HlsManifestRecoveryBurstLevel::Friendly.plan();
                state.origin_control.begin_acceptance_episode(
                    request.now_ms,
                    plan,
                    HlsManifestAcceptanceTrigger::RecoveryRequired,
                    &test_acceptance_episode_timing(request.now_ms, plan),
                );
                state.origin_control.acceptance_episode.as_mut().unwrap().record_full_burst();
            }
            *origin.state.new_namespace_bytes.write().await = Some(media(if fresh { 7 } else { 1 }));
            let mut fetched = playlist(&origin.base, "fedcba9876543210", 4, "?token=reset");
            fetched.final_manifest_url = fetched.final_manifest_url.replace("127.0.0.1", "localhost");
            fetched.resolved_request_url.clone_from(&fetched.final_manifest_url);
            fetched.redirect_host = Some("localhost".into());
            origin.state.requests.lock().await.clear();
            let result = super::super::super::commit_manifest_recovery_candidate(&request, fetched, mode).await;
            assert_eq!(result.is_ok(), fresh, "{mode:?} fresh={fresh}: {:?}", result.err());
            assert_eq!(
                origin.state.requests.lock().await.len(),
                4,
                "each uncertain reset object is verified exactly once"
            );
            let state = request.session.read().await;
            assert_eq!(state.origin_epoch, u64::from(fresh));
            assert_eq!(state.proxy_next_seq, Some(if fresh { 8 } else { 4 }));
            assert_eq!(state.segments[&3].cache_key, old.cache_key);
            assert_eq!(state.segments[&3].occurrence_identity, old.occurrence_identity);
            assert_eq!(tokio::fs::read(request.segment_cache.object_path(&old.cache_key)).await.unwrap(), media(1));
        }
    }
}

fn window(origin: &Origin, first: u64, count: u64, token: &str) -> FetchedOriginManifest {
    let mut body = format!("#EXTM3U\n#EXT-X-VERSION:4\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:{first}\n");
    for msn in first..first.checked_add(count).unwrap() {
        writeln!(body, "#EXTINF:4,\n{}/0123456789abcdef/{msn}.ts?token={token}", origin.base).unwrap();
    }
    let mut fetched = fetched_manifest(&body);
    fetched.final_manifest_url = format!("{}/index.m3u8", origin.base);
    fetched.resolved_request_url.clone_from(&fetched.final_manifest_url);
    fetched.redirect_host = Some("127.0.0.1".into());
    fetched
}

fn context(request: &OriginRefreshRequest) -> crate::SegmentFetchContext {
    crate::SegmentFetchContext {
        session: Arc::clone(&request.session),
        segment_cache: Arc::clone(&request.segment_cache),
        segment_repair: Arc::clone(&request.segment_repair),
        repair_access_lease_id: None,
        headers: axum::http::HeaderMap::new(),
        origin_provider_session_headers: axum::http::HeaderMap::new(),
        client: request.client.clone(),
        no_redirect_client: request.no_redirect_client.clone(),
        use_manual_redirects: false,
        origin_io: None,
    }
}

async fn fetch(request: &OriginRefreshRequest, seq: u64) {
    assert_eq!(
        request
            .segment_worker_pool
            .demand_fetch_and_wait(
                context(request),
                &crate::HlsSegmentFile { proxy_seq: seq, extension: "ts".into() },
                request.now_ms
            )
            .await,
        crate::segment_fetcher::SegmentDemandFetchOutcome::Ready
    );
}

async fn ready_window(origin: &Origin) -> OriginRefreshRequest {
    ready_window_from(origin, window(origin, 101, 4, "old")).await
}

async fn ready_window_from(origin: &Origin, initial: FetchedOriginManifest) -> OriginRefreshRequest {
    let manager = Arc::new(HlsProxyManager::with_private_cache(&shared::model::HlsCacheConfigDto {
        startup: shared::model::HlsStartupConfigDto { mode: HlsStartupMode::FirstReady, ..Default::default() },
        ..Default::default()
    }));
    let now = super::super::super::current_time_millis();
    let session = manager.get_or_create_session(crate::HlsSessionKey::new(1, "12345"), b"secret", now).await;
    let mut request = switch_test_request(Arc::clone(&session), Arc::clone(manager.segment_cache()), &origin.base);
    request.hls_proxy = manager;
    request.now_ms = now;
    request.manifest_commit_requirement = HlsManifestCommitRequirement::CommittedManifestAllowed;
    request.origin_entry = LiveHlsOriginEntry::parse(&initial.resolved_request_url).unwrap();
    {
        let mut state = session.write().await;
        let startup = state.startup.as_mut().unwrap();
        startup.worker = Arc::downgrade(&request.segment_worker_pool);
        startup.policy_fixed = true;
        commit_fetched_manifest(&mut state, &initial, &request, now).unwrap();
    }
    for seq in 0..4 {
        fetch(&request, seq).await;
    }
    {
        let mut state = session.write().await;
        state.mark_authorized_media_access(now);
        state.render_and_store_manifest(now).unwrap();
    }
    request
}

fn rotate_tail(mut fetched: FetchedOriginManifest, token: &str) -> FetchedOriginManifest {
    fetched.body = fetched.body.replace("104.ts?token=old", &format!("104.ts?token={token}"));
    fetched
}

async fn reload(request: &mut OriginRefreshRequest, origin: &Origin, fetched: FetchedOriginManifest) {
    *origin.state.fallback_manifest.write().await = Some(fetched.body);
    request.attempt = None;
    request.now_ms = super::super::super::current_time_millis();
    request.session.write().await.origin_refresh.next_fetch_allowed_at_ms = 0;
    assert!(trigger_origin_refresh_sync(request.clone()).await);
}

#[tokio::test]
async fn confirmed_binding_carries_reload_and_successor_without_equality_downloads() {
    let origin = Origin::start().await;
    let mut request = ready_window(&origin).await;
    let (original, revision, commits) = {
        let state = request.session.read().await;
        (
            state.segments[&3].clone(),
            state.startup.as_ref().unwrap().revisions[&3].clone(),
            state.origin_manifest_commits(),
        )
    };
    let expected: [u8; 32] = Sha256::digest(media(1)).into();
    let candidate = rotate_tail(window(&origin, 101, 4, "old"), "new");
    origin.state.requests.lock().await.clear();
    reload(&mut request, &origin, candidate.clone()).await;
    let after = origin.state.requests.lock().await.clone();
    assert_eq!(after.iter().filter(|uri| uri.ends_with("104.ts?token=new")).count(), 1);
    let generation = request.session.read().await.origin_control.acceptance_generation;
    {
        let state = request.session.read().await;
        assert!(state.origin_manifest_commits() > commits);
        assert_eq!(HlsCacheOriginAnchor::capture(&state).unwrap().identity, original.occurrence_identity.unwrap());
        assert_eq!(
            state.origin_content.digest(&state.segments[&3], state.activity.origin_work_generation),
            Some(expected)
        );
        assert_eq!(state.segments[&3].origin_fetch_ref, original.origin_fetch_ref);
        assert_eq!(state.startup.as_ref().unwrap().revisions[&3], revision);
        assert_eq!(HlsOriginContentEvidence::verified_bindings(&state).len(), 1);
    }
    reload(&mut request, &origin, candidate.clone()).await;
    let advanced = rotate_tail(window(&origin, 101, 5, "old"), "new");
    reload(&mut request, &origin, advanced).await;
    let state = request.session.read().await;
    assert_eq!(state.origin_control.acceptance_generation, generation);
    assert_eq!(state.origin_epoch, 0);
    assert_eq!(HlsCacheOriginAnchor::capture(&state).unwrap().identity.origin_msn(), 105);
    assert_eq!(state.proxy_next_seq, Some(5));
    assert_eq!(state.segments[&3].origin_fetch_ref, original.origin_fetch_ref);
    assert!(revision.revision().origin_binding_is_current(&state));
    drop(state);
    let requests = origin.state.requests.lock().await.clone();
    assert_eq!(requests.iter().filter(|uri| uri.ends_with("104.ts?token=new")).count(), 1);
    assert_eq!(
        requests.iter().filter(|uri| path_has_extension(uri, "m3u8")).count(),
        after.iter().filter(|uri| path_has_extension(uri, "m3u8")).count() + 2
    );
    assert_eq!(
        tokio::fs::read(revision.revision().wait_complete(tokio::time::Instant::now()).await.unwrap().path)
            .await
            .unwrap(),
        media(1)
    );
    // A third binding for the current tail must acquire its own complete proof.
    fetch(&request, 4).await;
    let third = window(&origin, 101, 5, "old");
    let mut third = rotate_tail(third, "third");
    third.body = third.body.replace("105.ts?token=old", "105.ts?token=third");
    reload(&mut request, &origin, third).await;
    let state = request.session.read().await;
    assert!(state.origin_control.acceptance_generation.0 > generation.0);
    assert_eq!(HlsCacheOriginAnchor::capture(&state).unwrap().identity.origin_msn(), 105);
    assert_eq!(state.segments[&3].origin_fetch_ref, original.origin_fetch_ref);
    assert_eq!(state.origin_epoch, 0);
}

#[tokio::test]
async fn recovery_reuses_ready_overlap_and_fetches_only_missing_media() {
    let origin = Origin::start().await;
    let request = ready_window(&origin).await;
    let original = request.session.read().await.segments.clone();
    crate::refresh::origin_probe::commit_pinned_manifest(
        &request,
        &window(&origin, 104, 2, "old"),
        HlsManifestCommitAcceptanceMode::StrictPinnedHost,
    )
    .await
    .unwrap();
    let mut fetched = window(&origin, 101, 6, "old");
    fetched.final_manifest_url = fetched.final_manifest_url.replace("127.0.0.1", "localhost");
    fetched.resolved_request_url.clone_from(&fetched.final_manifest_url);
    fetched.redirect_host = Some("localhost".into());
    *origin.state.fallback_manifest.write().await = Some(fetched.body.clone());
    fetched.body = request.client.get(&fetched.final_manifest_url).send().await.unwrap().text().await.unwrap();
    origin.state.requests.lock().await.clear();
    let result = super::super::super::commit_manifest_recovery_candidate(
        &request,
        fetched,
        HlsManifestCommitAcceptanceMode::AllowHeldHostSwitchCandidate,
    )
    .await;
    assert!(result.is_ok(), "{:?}", result.err());
    assert!(origin.state.requests.lock().await.is_empty(), "known READY overlap needs no re-staging");
    {
        let state = request.session.read().await;
        assert_eq!(state.origin_epoch, 0);
        assert_eq!(state.proxy_next_seq, Some(6));
        for (seq, entry) in &original {
            assert_eq!(state.segments[seq].cache_key, entry.cache_key);
            assert_eq!(state.segments[seq].status, entry.status);
            assert_eq!(state.segments[seq].occurrence_identity, entry.occurrence_identity);
            assert_eq!(state.segments[seq].origin_fetch_ref, entry.origin_fetch_ref);
        }
    }
    fetch(&request, 4).await;
    fetch(&request, 5).await;
    let requests = origin.state.requests.lock().await.clone();
    assert_eq!(requests, ["/0123456789abcdef/105.ts?token=old", "/0123456789abcdef/106.ts?token=old"]);
}
