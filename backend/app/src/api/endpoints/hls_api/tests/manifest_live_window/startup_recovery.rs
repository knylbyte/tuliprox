use super::{
    current_time_millis, origin, prepare_revisions, response_body, test_app_state_with_hls_proxy,
    try_test_hls_cached_manifest_response, Arc, Duration, HlsAccessLeaseId, HlsAccessLeaseState, HlsCacheConfig,
    HlsCacheConfigDto, HlsProxyManager, HlsSessionKey, HlsStartupMode, HlsStripMode, SegmentCacheStatus, StatusCode,
    StripConfig,
};
use crate::{api::model::AppState, model::Config};
use tuliprox_hls::api::{HlsRuntimeCustomTailReason, HlsSessionHandle};

const CUSTOM_RESPONSE_CASES: [(bool, StatusCode); 4] = [
    (true, StatusCode::SERVICE_UNAVAILABLE),
    (false, StatusCode::BAD_GATEWAY),
    (false, StatusCode::SERVICE_UNAVAILABLE),
    (false, StatusCode::UNPROCESSABLE_ENTITY),
];

fn configure_custom_response(app: &Arc<AppState>, video: bool, error_status: StatusCode) {
    // Keep an asset configured even when disabled: the switch must take precedence.
    super::super::enable_channel_unavailable_custom_response(app);
    let config = app.app_config.config.load();
    app.app_config.config.store(Arc::new(Config {
        custom_stream_response_enabled: video,
        custom_stream_response_error_status: error_status.as_u16(),
        ..config.as_ref().clone()
    }));
}

async fn fixture(mode: HlsStartupMode, failed: &[u64]) -> (tempfile::TempDir, Arc<AppState>, HlsSessionHandle) {
    let directory = tempfile::tempdir().unwrap();
    let mut dto =
        HlsCacheConfigDto { cache_path: Some(directory.path().to_string_lossy().into_owned()), ..Default::default() };
    dto.startup.mode = mode;
    dto.strip.value = 3;
    let manager = Arc::new(HlsProxyManager::with_hls_cache_config(&HlsCacheConfig::from(&dto)));
    let app = test_app_state_with_hls_proxy(manager.clone());
    super::super::store_test_sources_with_target(
        &app,
        super::super::test_hls_input(),
        super::super::test_hls_share_target(true),
    );
    let session =
        manager.get_or_create_session(HlsSessionKey::new(1, "recovered-entry"), b"secret", current_time_millis()).await;
    {
        let mut state = session.write().await;
        state.apply_origin_manifest(&origin(409_624, 6, &[8])).unwrap();
        state.target_duration = Some(21);
        let ready = (0..6).filter(|seq| !failed.contains(seq)).collect::<Vec<_>>();
        prepare_revisions(&mut state, &ready);
        for seq in failed {
            state.segments.get_mut(seq).unwrap().status = SegmentCacheStatus::FailedPermanent {
                failed_at_ms: current_time_millis(),
                status: Some(StatusCode::NOT_FOUND),
            };
        }
        let _ = state.render_and_store_manifest(current_time_millis());
        state.origin_control.path_condition = tuliprox_hls::HlsOriginPathCondition::SegmentReadinessFailure;
    }
    (directory, app, session)
}

#[tokio::test]
async fn recovered_startup_publishes_ready_successors_including_short_windows_without_waiting() {
    for mode in [HlsStartupMode::FirstReady, HlsStartupMode::Progressive, HlsStartupMode::Optimistic] {
        for (failed, expected) in
            [(vec![0], vec![1, 2, 3]), (vec![0, 1, 2, 3], vec![4, 5]), (vec![0, 1, 2, 3, 4], vec![5])]
        {
            let (_directory, app, session) = fixture(mode, &failed).await;
            let proxy = session.read().await.proxy_session_id.clone();
            let id = HlsAccessLeaseId("recovered-entry".into());
            super::super::prepare_pending_test_hls_access_lease(&app, &proxy, &id).await;
            let response = tokio::time::timeout(
                Duration::from_secs(1),
                try_test_hls_cached_manifest_response(
                    &app,
                    &session,
                    &id,
                    HlsAccessLeaseState::Pending,
                    &StripConfig { mode: HlsStripMode::Segments, value: 3 },
                    None,
                    super::super::HlsCachedManifestOptions::initial(Duration::from_secs(30)),
                ),
            )
            .await
            .expect("ready successors must not wait for lease expiry")
            .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{mode:?} failed={failed:?}");
            let body = String::from_utf8(response_body(response).await.to_vec()).unwrap();
            assert!(!body.contains("#EXT-X-ENDLIST"));
            let lease = app.hls.proxy.access_lease_response_snapshot(&id, &proxy, current_time_millis()).await.unwrap();
            let snapshot = lease.last_manifest_snapshot.unwrap();
            assert_eq!(snapshot.visible_proxy_seqs().collect::<Vec<_>>(), expected);
            assert_eq!(body.lines().filter(|line| !line.is_empty() && !line.starts_with('#')).count(), expected.len());
        }
    }
}

#[tokio::test]
async fn exhausted_startup_returns_configured_fallback_immediately_and_keeps_active_leases() {
    for (video, error_status) in CUSTOM_RESPONSE_CASES {
        let (_directory, app, session) = fixture(HlsStartupMode::Optimistic, &[0, 1, 2, 3, 4, 5]).await;
        let proxy = session.read().await.proxy_session_id.clone();
        let id = HlsAccessLeaseId("exhausted-entry".into());
        super::super::prepare_pending_test_hls_access_lease(&app, &proxy, &id).await;
        configure_custom_response(&app, video, error_status);
        let response = tokio::time::timeout(
            Duration::from_secs(1),
            try_test_hls_cached_manifest_response(
                &app,
                &session,
                &id,
                HlsAccessLeaseState::Pending,
                &StripConfig { mode: HlsStripMode::Segments, value: 3 },
                None,
                super::super::HlsCachedManifestOptions::initial(Duration::from_secs(30)),
            ),
        )
        .await
        .expect("known exhaustion must bypass the startup wait")
        .unwrap();
        assert_eq!(response.status(), if video { StatusCode::OK } else { error_status });
        if video {
            assert!(String::from_utf8(response_body(response).await.to_vec()).unwrap().contains("#EXT-X-ENDLIST"));
        }
        let mut lease = app.hls.proxy.access_lease_response_snapshot(&id, &proxy, current_time_millis()).await.unwrap();
        assert!(lease.last_manifest_snapshot.is_none());
        assert!(lease.permits_unpublished_standalone_tail(HlsRuntimeCustomTailReason::ChannelUnavailable));
        lease.state = HlsAccessLeaseState::Activated;
        assert!(super::super::super::hls_exhausted_startup_response(&app, &session, &lease).await.is_none());
    }
}

#[tokio::test]
async fn exhausted_startup_with_enabled_custom_response_without_asset_keeps_hls_not_found_fallback() {
    let (_directory, app, session) = fixture(HlsStartupMode::Optimistic, &[0, 1, 2, 3, 4, 5]).await;
    let proxy = session.read().await.proxy_session_id.clone();
    let id = HlsAccessLeaseId("missing-custom-asset".into());
    super::super::prepare_pending_test_hls_access_lease(&app, &proxy, &id).await;
    configure_custom_response(&app, true, StatusCode::SERVICE_UNAVAILABLE);
    app.app_config.custom_stream_response.store(None);
    let mut lease = app.hls.proxy.access_lease_response_snapshot(&id, &proxy, current_time_millis()).await.unwrap();
    for state in [HlsAccessLeaseState::Pending, HlsAccessLeaseState::Idle] {
        lease.state = state;
        let response = super::super::super::hls_exhausted_startup_response(&app, &session, &lease).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}

#[tokio::test]
async fn exhausted_idle_reentry_preserves_previous_lease_evidence() {
    use axum::{body::Body, extract::ConnectInfo, http::Request};
    use tower::ServiceExt;

    for published in [false, true] {
        for (video, error_status) in CUSTOM_RESPONSE_CASES {
            let (_directory, app, session) = fixture(HlsStartupMode::Optimistic, &[]).await;
            let proxy = session.read().await.proxy_session_id.clone();
            let id = HlsAccessLeaseId("idle-exhausted-entry".into());
            super::super::prepare_pending_test_hls_access_lease(&app, &proxy, &id).await;
            if published {
                session.write().await.origin_control.path_condition =
                    tuliprox_hls::HlsOriginPathCondition::ProgressExpected;
                let response = try_test_hls_cached_manifest_response(
                    &app,
                    &session,
                    &id,
                    HlsAccessLeaseState::Pending,
                    &StripConfig { mode: HlsStripMode::Segments, value: 3 },
                    None,
                    super::super::HlsCachedManifestOptions::initial(Duration::ZERO),
                )
                .await
                .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                let _ = response_body(response).await;
            }
            let mut before =
                app.hls.proxy.access_lease_response_snapshot(&id, &proxy, current_time_millis()).await.unwrap();
            assert_eq!(before.last_manifest_snapshot.is_some(), published);
            before.state = HlsAccessLeaseState::Idle;
            before.active_until_ms = None;
            before.pending_deadline = None;
            app.hls.proxy.prepare_access_lease(before.clone()).await;
            for entry in session.write().await.segments.values_mut() {
                entry.status = SegmentCacheStatus::FailedPermanent {
                    failed_at_ms: current_time_millis(),
                    status: Some(StatusCode::NOT_FOUND),
                };
            }
            configure_custom_response(&app, video, error_status);
            let response = tokio::time::timeout(
                Duration::from_secs(1),
                try_test_hls_cached_manifest_response(
                    &app,
                    &session,
                    &id,
                    HlsAccessLeaseState::Idle,
                    &StripConfig { mode: HlsStripMode::Segments, value: 3 },
                    None,
                    super::super::HlsCachedManifestOptions::initial(Duration::from_secs(30)),
                ),
            )
            .await
            .expect("idle re-entry must not wait for lease expiry")
            .unwrap();
            assert_eq!(response.status(), if video { StatusCode::OK } else { error_status });
            let after = app.hls.proxy.access_lease_response_snapshot(&id, &proxy, current_time_millis()).await.unwrap();
            assert_eq!(after, before, "the fallback must preserve all previous lease evidence");
            if video {
                let body = String::from_utf8(response_body(response).await.to_vec()).unwrap();
                assert!(body.contains("#EXT-X-ENDLIST"));
                for uri in body.lines().filter(|line| !line.is_empty() && !line.starts_with('#')) {
                    let router =
                        crate::api::endpoints::custom_video_stream_api::cvs_api_register().with_state(Arc::clone(&app));
                    let parsed = url::Url::parse(uri).unwrap();
                    let route_start = parsed.path().rfind("/cvs/").unwrap();
                    let mut request =
                        Request::builder().uri(&parsed.path()[route_start..]).body(Body::empty()).unwrap();
                    request.extensions_mut().insert(ConnectInfo(super::super::test_fingerprint().addr));
                    let media = router.oneshot(request).await.unwrap();
                    assert_eq!(media.status(), StatusCode::OK, "idle custom media remains authorized: {uri}");
                }
            }
            let mut terminal = before.clone();
            terminal.playback_mode = tuliprox_hls::api::HlsLeasePlaybackMode::Ended;
            assert!(super::super::super::hls_exhausted_startup_response(&app, &session, &terminal).await.is_none());
            for state in [
                HlsAccessLeaseState::Activated,
                HlsAccessLeaseState::PolicyRevoking,
                HlsAccessLeaseState::Denied,
                HlsAccessLeaseState::Expired,
            ] {
                let mut protected = before.clone();
                protected.state = state;
                assert!(super::super::super::hls_exhausted_startup_response(&app, &session, &protected)
                    .await
                    .is_none());
            }
        }
    }
}

#[tokio::test]
#[ignore = "requires local Postman simulation via TULIPROX_HLS_STARTUP_ORIGIN"]
#[expect(
    clippy::too_many_lines,
    reason = "Keeps real origin failures and the pending-to-idle retry in one HTTP scenario."
)]
async fn startup_recovery_http_simulation() {
    use super::super::{
        create_active_hls_user_session, enable_channel_unavailable_custom_response, get_response,
        manifest_entry_freshness::canonical_response_for_lease, test_app_state_with_inputs,
    };
    use crate::model::{ConfigInput, ReverseProxyConfig};
    use shared::model::{HlsStartupConfigDto, InputType, ReverseProxyConfigDto};

    let base = std::env::var("TULIPROX_HLS_STARTUP_ORIGIN").expect("local simulation origin address");
    for (case, video, expected) in [
        ("healthy", false, vec![0, 1, 2]),
        ("missing-head", false, vec![1, 2, 3]),
        ("short", false, vec![4, 5]),
        ("all-missing", true, vec![]),
        ("all-missing", false, vec![]),
    ] {
        let input = ConfigInput {
            id: 1,
            name: Arc::from("startup-origin"),
            input_type: InputType::M3u,
            url: base.clone(),
            max_connections: 8,
            enabled: true,
            ..Default::default()
        };
        let app = test_app_state_with_inputs(vec![Arc::new(input.clone())]);
        let mut cache = app.hls.proxy.keep_cache_path(HlsCacheConfigDto {
            startup: HlsStartupConfigDto { mode: HlsStartupMode::Optimistic, ..Default::default() },
            ..Default::default()
        });
        cache.strip.value = 3;
        app.app_config.config.store(Arc::new(Config {
            custom_stream_response_enabled: video,
            custom_stream_response_error_status: StatusCode::SERVICE_UNAVAILABLE.as_u16(),
            reverse_proxy: Some(ReverseProxyConfig::from(&ReverseProxyConfigDto {
                hls_cache: Some(cache),
                ..Default::default()
            })),
            ..Default::default()
        }));
        app.hls.proxy.update_config(&app.app_config).await;
        if video {
            enable_channel_unavailable_custom_response(&app);
        }
        create_active_hls_user_session(&app).await;
        let url = format!("{base}/{case}/index.m3u8");
        let response = tokio::time::timeout(
            Duration::from_secs(5),
            canonical_response_for_lease(&app, &input, &url, "recovered-start"),
        )
        .await
        .expect("the canonical request must finish before the 42-second lease deadline");
        assert_eq!(
            response.status(),
            if expected.is_empty() && !video { StatusCode::SERVICE_UNAVAILABLE } else { StatusCode::OK },
            "{case} video={video}"
        );
        if expected.is_empty() {
            if video {
                let body = String::from_utf8(response_body(response).await.to_vec()).unwrap();
                assert!(body.contains("#EXT-X-ENDLIST"));
            }
            let origin_source = super::super::super::build_hls_origin_source(&input, "12345");
            let session = app.hls.proxy.sessions().get_by_key(&origin_source.session_key()).await.unwrap();
            let proxy = session.read().await.proxy_session_id.clone();
            let id = HlsAccessLeaseId("recovered-start".into());
            let mut before =
                app.hls.proxy.access_lease_response_snapshot(&id, &proxy, current_time_millis()).await.unwrap();
            before.state = HlsAccessLeaseState::Idle;
            before.pending_deadline = None;
            app.hls.proxy.prepare_access_lease(before.clone()).await;
            let idle =
                tokio::time::timeout(Duration::from_secs(5), canonical_response_for_lease(&app, &input, &url, &id.0))
                    .await
                    .expect("idle retry must not wait for the owner deadline");
            assert_eq!(idle.status(), if video { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE });
            if video {
                let body = String::from_utf8(response_body(idle).await.to_vec()).unwrap();
                assert!(body.contains("#EXT-X-ENDLIST"));
            }
            let after = app.hls.proxy.access_lease_response_snapshot(&id, &proxy, current_time_millis()).await.unwrap();
            assert_eq!(after.last_manifest_snapshot, before.last_manifest_snapshot);
            assert_eq!(after.playback_cursor, before.playback_cursor);
            println!("{case} video={video}: pending and idle fallback completed");
            continue;
        }
        if response.status() != StatusCode::OK {
            continue;
        }
        let body = String::from_utf8(response_body(response).await.to_vec()).unwrap();
        assert!(!body.contains("#EXT-X-ENDLIST"), "{case}: {body}");
        let origin_source = super::super::super::build_hls_origin_source(&input, "12345");
        let session = app.hls.proxy.sessions().get_by_key(&origin_source.session_key()).await.unwrap();
        let proxy = session.read().await.proxy_session_id.clone();
        let lease = app
            .hls
            .proxy
            .access_lease_response_snapshot(&HlsAccessLeaseId("recovered-start".into()), &proxy, current_time_millis())
            .await
            .unwrap();
        assert_eq!(lease.last_manifest_snapshot.unwrap().visible_proxy_seqs().collect::<Vec<_>>(), expected);
        for uri in body.lines().filter(|line| !line.is_empty() && !line.starts_with('#')) {
            let media = get_response(Arc::clone(&app), uri, None).await;
            assert_eq!(media.status(), StatusCode::OK, "{case}: {uri}");
            assert_eq!(response_body(media).await.as_ref(), &[0x47; 8192]);
        }
        println!("{case} video={video}: published {expected:?}");
    }
}
