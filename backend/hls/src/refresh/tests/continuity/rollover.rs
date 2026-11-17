use super::*;
use crate::refresh::{
    commit_manifest_recovery_candidate,
    continuity::observe_recovery_anchor,
    origin_probe::commit_pinned_manifest,
    tests::support::{spawn_test_origin_with_response_gate, TestOriginResponseGate},
};
use tokio::sync::Notify;

const A: &str = "a.example";
const B: &str = "localhost";
const C: &str = "c.example";
const MODE: HlsManifestCommitAcceptanceMode = HlsManifestCommitAcceptanceMode::StrictPinnedHost;

fn window(host: &str, first: u64, last: u64, namespace: &str) -> FetchedOriginManifest {
    let mut fetched = manifest(&format!("https://{host}"), first, last);
    fetched.body = fetched.body.replace("/media/", &format!("/{namespace}/"));
    fetched.final_manifest_url = format!("https://{host}/delivery/{namespace}/reload.m3u8?window={first}-{last}");
    fetched
}

async fn commit(request: &OriginRefreshRequest, fetched: &FetchedOriginManifest) {
    commit_pinned_manifest(request, fetched, MODE).await.unwrap();
}

async fn reject(request: &OriginRefreshRequest, fetched: &FetchedOriginManifest) -> HlsManifestRejectLogReason {
    match commit_pinned_manifest(request, fetched, MODE).await {
        Err(HlsManifestCommitError::TimelineRejected { reason }) => reason,
        _ => panic!("expected a normal timeline rejection"),
    }
}

#[tokio::test]
async fn rollover_host_confirmation_from_an_unselected_recovery_response_has_its_own_expiry() {
    let b_body = window(B, 101, 106, "media").body;
    let delayed =
        Arc::new(TestOriginResponseGate { request_number: 2, entered: Notify::new(), release: Notify::new() });
    let alternate = spawn_test_origin_with_response_gate(
        Arc::new(move |_| (200, Vec::new(), b_body.clone())),
        Some(Arc::clone(&delayed)),
    )
    .await;
    let redirect = format!("{}/rotating/b.m3u8?manifest=2", alternate.base_url.replace("127.0.0.1", "localhost"));
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let pinned = spawn_test_origin(Arc::new(move |_| match counted.fetch_add(1, Ordering::AcqRel) {
        0 => (200, Vec::new(), window(A, 101, 105, "media").body),
        1 => (307, vec![("Location", redirect.clone())], String::new()),
        _ => (200, Vec::new(), window(A, 102, 107, "media").body),
    }))
    .await;
    let ctx = crate::hls_ctx::HlsCtx::for_test(tuliprox_core::model::Config::default());
    let mut request = bind_refresh_request_to_app_state(established(&pinned.base_url).await, &ctx);
    request.manifest_recovery_burst.level = shared::model::HlsManifestRecoveryBurstLevel::Friendly;
    let at = tuliprox_core::utils::current_time_millis();
    request.hls_proxy.freeze_clock_for_test(at);
    commit(&request, &manifest(&pinned.base_url, 101, 106)).await;
    assert_eq!(
        request.session.read().await.origin_control.manifest_host_expiry_for_test("127.0.0.1"),
        Some(at + 48_000)
    );
    let committed = fetch_and_commit_manifest_with_policy(&mut request).await.unwrap();
    assert_eq!(
        crate::manifest_fetch::fetched_effective_manifest_host(&committed.fetched).as_deref(),
        Some("127.0.0.1")
    );
    {
        let state = request.session.read().await;
        assert_eq!(state.origin_seq_highwater, Some(107));
        assert_eq!(state.origin_control.manifest_host_expiry_for_test("localhost"), Some(at + 48_000));
    }
    request.hls_proxy.freeze_clock_for_test(at + 32_000);
    let mut rotated = manifest(&pinned.base_url, 105, 110);
    rotated.final_manifest_url = format!("{}/rotating/a.m3u8?manifest=3", pinned.base_url);
    commit(&request, &rotated).await;
    {
        let state = request.session.read().await;
        assert_eq!(state.origin_control.manifest_host_expiry_for_test("127.0.0.1"), Some(at + 80_000));
        assert!(state.origin_control.manifest_host_confirmed("localhost", at + 47_999));
        assert!(!state.origin_control.manifest_host_confirmed("localhost", at + 48_000));
    }
    let timeout_request = OriginRefreshRequest {
        origin_entry: LiveHlsOriginEntry::parse(&format!("{}/index.m3u8", alternate.base_url.replace("127.0.0.1", B)))
            .unwrap(),
        ..request.clone()
    };
    let context = manifest_fetch_context(&timeout_request, Some(4));
    let fetch = fetch_hls_origin_manifest_request(HlsOriginManifestFetchRequest::initial_global_policy(&context));
    let timeout = std::time::Duration::from_millis(200);
    assert!(tokio::time::timeout(timeout, fetch).await.is_err());
    tokio::time::timeout(timeout, delayed.entered.notified()).await.unwrap();
    assert_eq!(
        request.session.read().await.origin_control.manifest_host_expiry_for_test("localhost"),
        Some(at + 48_000)
    );
    delayed.release.notify_one();
    request.hls_proxy.freeze_clock_for_test(at + 40_000);
    commit(&request, &window("localhost", 107, 110, "media")).await;
    assert_eq!(
        request.session.read().await.origin_control.manifest_host_expiry_for_test("localhost"),
        Some(at + 72_000)
    );
    let conflict = window("localhost", 105, 110, "changed");
    assert!(matches!(
        reject(&request, &conflict).await,
        HlsManifestRejectLogReason::OriginSequenceResourceConflict { .. }
    ));
    assert!(request.session.read().await.origin_control.manifest_host_expiry_for_test("localhost").is_none());
    assert!(matches!(
        reject(&request, &window(C, 110, 110, "unknown")).await,
        HlsManifestRejectLogReason::OriginSequenceResourceConflict { .. }
    ));
    assert!(request.session.read().await.origin_control.manifest_host_expiry_for_test(C).is_none());
    eprintln!("host_confirmation a_ttl_ms=48000 b_ttl_ms=48000 selected=127.0.0.1 unselected=localhost b_renewed_ttl_ms=32000 anchor=110");
}

#[tokio::test]
async fn rollover_discards_host_proofs_and_fences_a_delayed_manifest_from_the_previous_epoch() {
    let request = established(&format!("https://{A}")).await;
    commit(&request, &window(A, 101, 104, "media")).await;
    let clock = request.hls_proxy.now_ms();
    observe_recovery_anchor(&mut *request.session.write().await, &window(B, 101, 104, "media"), 0, clock);
    let gate = Arc::new(TestOriginResponseGate { request_number: 1, entered: Notify::new(), release: Notify::new() });
    let late_body = window(B, 101, 105, "media").body;
    let late = spawn_test_origin_with_response_gate(
        Arc::new(move |_| (200, Vec::new(), late_body.clone())),
        Some(Arc::clone(&gate)),
    )
    .await;
    let mut old = request.clone();
    old.origin_entry =
        LiveHlsOriginEntry::parse(&format!("{}/index.m3u8", late.base_url.replace("127.0.0.1", B))).unwrap();
    assert!(mark_origin_refresh_started(&mut old, clock + 1).await);
    let delayed = tokio::spawn(async move { fetch_and_commit_manifest_with_policy(&mut old).await });
    tokio::time::timeout(std::time::Duration::from_secs(2), gate.entered.notified()).await.unwrap();
    commit(&request, &window(A, 0, 2, "new")).await;
    {
        let state = request.session.read().await;
        assert_eq!((state.origin_epoch, state.proxy_next_seq), (1, Some(7)));
        for host in [A, B] {
            assert!(state.origin_control.manifest_host_expiry_for_test(host).is_none());
        }
    }
    gate.release.notify_one();
    assert!(delayed.await.unwrap().is_err());
    {
        let mut state = request.session.write().await;
        // Even a late recovery response with matching new bytes cannot renew an old proof.
        observe_recovery_anchor(&mut state, &window(B, 0, 2, "new"), 0, clock);
        assert_eq!((state.origin_epoch, state.proxy_next_seq), (1, Some(7)));
        for host in [A, B] {
            assert!(state.origin_control.manifest_host_expiry_for_test(host).is_none());
        }
    }
    commit(&request, &window(B, 1, 3, "new")).await;
    assert!(request.session.read().await.origin_control.manifest_host_expiry_for_test(B).is_some());
    eprintln!("old_epoch_response fenced=true new_epoch=1 host_reconfirmed={B}");
}

#[tokio::test]
async fn ordinary_rollover_requires_known_host_explicit_zero_and_absent_anchor() {
    let request = established(&format!("https://{A}")).await;
    commit(&request, &window(A, 101, 104, "media")).await;
    for candidate in [window(B, 0, 2, "new"), window(A, 6, 8, "new")] {
        assert!(matches!(
            reject(&request, &candidate).await,
            HlsManifestRejectLogReason::OriginCacheAnchorMissing { .. }
        ));
        assert_eq!(request.session.read().await.proxy_next_seq, Some(4));
    }
    let mut implicit = window(A, 0, 2, "new");
    implicit.body = implicit.body.replace("#EXT-X-MEDIA-SEQUENCE:0\n", "");
    assert!(matches!(reject(&request, &implicit).await, HlsManifestRejectLogReason::OriginCacheAnchorMissing { .. }));
    let first = window(A, 0, 2, "new");
    commit(&request, &first).await;
    commit(&request, &first).await;
    let state = request.session.read().await;
    assert_eq!((state.origin_epoch, state.proxy_next_seq), (1, Some(7)));
    assert!(state.origin_control.manifest_host_expiry_for_test(A).is_some());
    assert_eq!(state.segments.values().filter(|entry| entry.discontinuity_before).count(), 1);
}

#[tokio::test]
async fn rollover_rejects_old_bindings_including_other_hosts_and_prior_published_epochs() {
    let request = established(&format!("https://{A}")).await;
    commit(&request, &window(A, 101, 104, "media")).await;
    let expires = request.session.read().await.origin_control.manifest_host_expiry_for_test(A);
    for (replacement, position, original) in
        [("/media/104.ts?token=a", 0, 3), ("https://other.example/media/102.ts?token=a", 1, 1)]
    {
        let mut candidate = window(A, 0, 2, "new");
        candidate.body = candidate.body.replace(&format!("/new/{position}.ts?token=a"), replacement);
        assert_eq!(
            reject(&request, &candidate).await,
            HlsManifestRejectLogReason::OriginRolloverResourceReplay {
                existing_proxy_seq: original,
                candidate_origin_seq: position,
            }
        );
        let recovery = commit_manifest_recovery_candidate(
            &request,
            candidate,
            HlsManifestCommitAcceptanceMode::FreshPinnedRevalidation,
        )
        .await;
        assert!(matches!(
            recovery,
            Err(HlsManifestCommitError::TimelineRejected {
                reason: HlsManifestRejectLogReason::OriginRolloverResourceReplay { .. }
            })
        ));
        let state = request.session.read().await;
        assert_eq!((state.origin_epoch, state.proxy_next_seq), (0, Some(4)));
        assert_eq!(state.origin_control.manifest_host_expiry_for_test(A), expires);
        assert!(state.segments.values().all(|entry| !entry.discontinuity_before));
    }
    let first = window(A, 0, 2, "new");
    commit(&request, &first).await;
    commit(&request, &window(A, 1, 3, "new")).await;
    {
        let mut state = request.session.write().await;
        for seq in 4..7 {
            let identity = state.segments[&seq].occurrence_identity.unwrap();
            state.published_resource_history.record(identity, seq);
        }
    }
    commit(&request, &window(A, 0, 3, "third")).await;
    commit(&request, &window(A, 1, 3, "third")).await;
    {
        let mut state = request.session.write().await;
        state.segments.retain(|seq, _| *seq >= 8);
    }
    let expires = request.session.read().await.origin_control.manifest_host_expiry_for_test(A);
    assert_eq!(
        reject(&request, &first).await,
        HlsManifestRejectLogReason::OriginRolloverResourceReplay { existing_proxy_seq: 4, candidate_origin_seq: 0 }
    );
    let state = request.session.read().await;
    assert_eq!((state.origin_epoch, state.proxy_next_seq), (2, Some(12)));
    assert_eq!(state.origin_control.manifest_host_expiry_for_test(A), expires);
    eprintln!("rollover_guard anchor=true cross_host_other_occurrence=true history_after_gc=true recovery=true");
}
