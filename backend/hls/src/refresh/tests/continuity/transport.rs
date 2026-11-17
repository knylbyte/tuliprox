use super::*;

fn media_context(request: &OriginRefreshRequest) -> crate::SegmentFetchContext {
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

#[tokio::test]
async fn late_older_http_completion_does_not_replace_newest_discovered_or_ready_anchor() {
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let requests = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let (start, finish, captured) = (Arc::clone(&entered), Arc::clone(&release), Arc::clone(&requests));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = axum::Router::new().fallback(move |uri: axum::http::Uri| {
        let (start, finish, captured) = (Arc::clone(&start), Arc::clone(&finish), Arc::clone(&captured));
        async move {
            captured.lock().await.push(uri.path().to_owned());
            if uri.path() == "/media/101.ts" {
                start.notify_one();
                finish.notified().await;
            }
            ([(axum::http::header::CONTENT_TYPE, "video/mp2t")], CRITICAL_HANDOFF_TS_BODY)
        }
    });
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let request = established(&base).await;
    {
        let mut state = request.session.write().await;
        for entry in state.segments.values_mut() {
            entry.status = SegmentCacheStatus::Discovered;
        }
        commit_fetched_manifest(&mut state, &manifest(&base, 104, 105), &request, request.now_ms).unwrap();
    }
    let selected = HlsCacheOriginAnchor::capture(&*request.session.read().await).unwrap();
    assert_eq!((selected.proxy_seq, selected.identity.origin_msn()), (4, 105));
    let pool = Arc::clone(&request.segment_worker_pool);
    let context = media_context(&request);
    let old_context = context.clone();
    let older = tokio::spawn(async move {
        pool.demand_fetch_and_wait(
            old_context,
            &crate::HlsSegmentFile { proxy_seq: 0, extension: "ts".into() },
            tuliprox_core::utils::current_time_millis(),
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified()).await.unwrap();
    assert_eq!(HlsCacheOriginAnchor::capture(&*request.session.read().await), Some(selected));
    assert_eq!(
        request
            .segment_worker_pool
            .demand_fetch_and_wait(
                context,
                &crate::HlsSegmentFile { proxy_seq: 4, extension: "ts".into() },
                request.now_ms
            )
            .await,
        crate::segment_fetcher::SegmentDemandFetchOutcome::Ready
    );
    assert_eq!(HlsCacheOriginAnchor::capture(&*request.session.read().await), Some(selected));
    release.notify_one();
    assert_eq!(older.await.unwrap(), crate::segment_fetcher::SegmentDemandFetchOutcome::Ready);
    let state = request.session.read().await;
    assert_eq!(HlsCacheOriginAnchor::capture(&state), Some(selected));
    assert!(matches!(state.segments[&0].status, SegmentCacheStatus::Ready { .. }));
    assert!(matches!(state.segments[&4].status, SegmentCacheStatus::Ready { .. }));
    assert_eq!(*requests.lock().await, ["/media/101.ts", "/media/105.ts"]);
    server.abort();
}

#[tokio::test]
async fn alternating_manifest_and_media_hosts_preserve_ready_bytes_and_revision_without_recovery() {
    let body = manifest("http://fixture.example", 103, 106).body;
    let origin = spawn_test_origin(Arc::new(move |path| {
        if path.contains("m3u8") {
            (200, vec![("Content-Range", "bytes 200-299/1000".into())], body.clone())
        } else {
            (500, Vec::new(), String::new())
        }
    }))
    .await;
    let mut request = established(&origin.base_url).await;
    let (epoch, old) = {
        let state = request.session.read().await;
        (state.origin_epoch, [state.segments[&0].clone(), state.segments[&1].clone()])
    };
    let store = crate::SegmentRevisionStore::default();
    let mut revisions = Vec::new();
    for entry in &old {
        let metadata =
            request.segment_cache.write_temp_and_commit(&entry.cache_key, std::io::Cursor::new(b"r")).await.unwrap();
        let revision = store
            .create(entry.cache_key.proxy_session_id().clone(), entry.proxy_seq, crate::SegmentRevisionKind::Raw)
            .unwrap();
        revision.revision().bind_origin(&*request.session.read().await);
        assert!(revision.revision().complete(metadata));
        revisions.push((revision.revision().id(), revision));
    }
    let mirror = origin.base_url.replace("127.0.0.1", "localhost");
    for (index, base) in
        [mirror.as_str(), origin.base_url.as_str(), mirror.as_str(), origin.base_url.as_str()].into_iter().enumerate()
    {
        request.origin_entry = LiveHlsOriginEntry::parse(&format!("{base}/index.m3u8")).unwrap();
        request.attempt = None;
        request.headers.insert(
            axum::http::header::RANGE,
            if index % 2 == 0 { "bytes=0-99" } else { "bytes=200-299" }.parse().unwrap(),
        );
        let result = fetch_and_commit_manifest_with_policy(&mut request).await;
        assert!(result.is_ok(), "{:?}", result.err());
        let state = request.session.read().await;
        assert_eq!(state.origin_epoch, epoch);
        assert_eq!(state.proxy_next_seq, Some(6));
        assert!(state.origin_control.acceptance_episode.is_none());
        assert!(state.segments.values().all(|entry| !entry.discontinuity_before));
        assert_eq!([state.segments[&0].clone(), state.segments[&1].clone()], old);
        for (id, revision) in &revisions {
            assert_eq!(revision.revision().id(), *id);
            assert!(revision.revision().origin_binding_is_current(&state));
        }
    }
    for entry in old {
        assert_eq!(tokio::fs::read(request.segment_cache.object_path(&entry.cache_key)).await.unwrap(), b"r");
    }
    assert_eq!(origin.requests.lock().await.len(), 4);
    assert!(origin.requests.lock().await.iter().all(|path| path.contains("m3u8")), "no equality or READY refetch GET");
}

#[tokio::test]
async fn empty_origin_cache_bootstraps_and_known_unready_entries_add_no_ready_reserve() {
    let request = test_origin_refresh_request(test_session());
    assert!(HlsCacheOriginAnchor::capture(&*request.session.read().await).is_none());
    let first = manifest("https://a.example", 101, 104);
    assert!(crate::refresh::origin_probe::commit_pinned_manifest(
        &request,
        &first,
        HlsManifestCommitAcceptanceMode::StrictPinnedHost
    )
    .await
    .is_ok());
    let before = request.session.read().await.ready_timeline_snapshot(0, request.now_ms);
    assert!(before.units.iter().all(|unit| unit.state == crate::media_reserve::HlsReadyMediaState::NotReady));
    let followup = manifest("https://b.example", 103, 104);
    assert!(crate::refresh::origin_probe::commit_pinned_manifest(
        &request,
        &followup,
        HlsManifestCommitAcceptanceMode::StrictPinnedHost
    )
    .await
    .is_ok());
    let state = request.session.read().await;
    assert_eq!(HlsCacheOriginAnchor::capture(&state).unwrap().identity.origin_msn(), 104);
    assert_eq!(state.ready_timeline_snapshot(0, request.now_ms), before);
    assert!(state.origin_control.acceptance_episode.is_none());
}
