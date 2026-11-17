use super::*;
use crate::api::model::{AppState, HlsSessionHandle, TransientResourceKind};
use axum::{
    body::Body,
    http::{HeaderMap, Response, Uri},
    response::Redirect,
    routing::get,
    Router,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};
use tuliprox_processing::parser::hls::origin_manifest::{parse_origin_media_manifest, OriginManifestParseOutcome};

struct RangeOrigin {
    base: String,
    requests: Arc<AtomicUsize>,
    payload: Arc<std::sync::RwLock<Vec<u8>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for RangeOrigin {
    fn drop(&mut self) { self.task.abort(); }
}

impl RangeOrigin {
    async fn start() -> Self { Self::start_with_routes(Router::new()).await }

    async fn start_with_routes(manifests: Router) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&requests);
        let payload = Arc::new(std::sync::RwLock::new(b"0123456789".to_vec()));
        let served = Arc::clone(&payload);
        let router = manifests.fallback(move |headers: HeaderMap| {
            let counted = Arc::clone(&counted);
            let served = Arc::clone(&served);
            async move {
                counted.fetch_add(1, Ordering::Relaxed);
                let bytes = served.read().unwrap().clone();
                let range = headers
                    .get(header::RANGE)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.strip_prefix("bytes="))
                    .and_then(|value| value.split_once('-'))
                    .map(|(start, end)| (start.parse::<usize>().unwrap(), end.parse::<usize>().unwrap_or(9)));
                if let Some((start, end)) = range {
                    Response::builder()
                        .status(StatusCode::PARTIAL_CONTENT)
                        .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/10"))
                        .header(header::CONTENT_LENGTH, end - start + 1)
                        .body(Body::from(bytes[start..=end].to_vec()))
                        .unwrap()
                } else {
                    Response::builder().header(header::CONTENT_LENGTH, 10).body(Body::from(bytes)).unwrap()
                }
            }
        });
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self { base, requests, payload, task }
    }
}

async fn fixture(origin: &RangeOrigin) -> (Arc<AppState>, HlsSessionHandle, ProxySessionId, HlsAccessLeaseId) {
    let app = test_app_state();
    enable_hls_cache(&app);
    let (proxy, _) = map_transient_resource(&app, &format!("{}/unused.ts", origin.base), "ts", true).await;
    let uri = hls_proxy_uri(&app, &proxy, "index.m3u8").await;
    let lease = HlsAccessLeaseId(uri.split('/').nth(5).unwrap().into());
    let proxy = ProxySessionId(proxy);
    let session = app.hls.proxy.sessions().get_by_proxy_session_id(&proxy).await.unwrap();
    (app, session, proxy, lease)
}

async fn publish(
    app: &Arc<AppState>,
    session: &HlsSessionHandle,
    proxy: &ProxySessionId,
    lease: &HlsAccessLeaseId,
    body: &str,
    url: &str,
) -> String {
    let now = super::super::super::current_time_millis();
    {
        let mut state = session.write().await;
        state.transient.commit_origin_manifest_for_test(body, url, proxy, b"rewrite-secret", now).unwrap();
        state.mark_authorized_media_access(now);
    }
    let response = try_test_hls_cached_manifest_response(
        app,
        session,
        lease,
        HlsAccessLeaseState::Activated,
        &StripConfig { mode: HlsStripMode::Segments, value: 0 },
        None,
        super::super::HlsCachedManifestOptions::committed_only(Duration::ZERO),
    )
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    String::from_utf8(response_body(response).await.to_vec()).unwrap()
}

fn media(body: &str) -> Vec<&str> { body.lines().filter(|line| line.starts_with("/hls/")).collect() }

#[tokio::test]
async fn delivered_overlap_keeps_its_complete_uri_and_cached_bytes_after_manifest_redirects() {
    let origin = RangeOrigin::start().await;
    let (app, session, proxy, lease) = fixture(&origin).await;
    let first = publish(&app, &session, &proxy, &lease,
        "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:10\n#EXTINF:4,\n10.ts\n#EXTINF:4,\n11.ts\n#EXTINF:4,\n12.ts\n",
        &format!("{}/live/index.m3u8", origin.base)).await;
    let old_uri = media(&first)[2].to_string();
    let response = get_response(Arc::clone(&app), &old_uri, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_body(response).await.as_ref(), b"0123456789");
    let second = publish(&app, &session, &proxy, &lease,
        "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:12\n#EXTINF:4,\n12.ts\n#EXTINF:4,\n13.ts\n#EXTINF:4,\n14.ts\n",
        &format!("{}/live/rotated.m3u8?turn=2", origin.base.replace("127.0.0.1", "localhost"))).await;
    assert_eq!(media(&second)[0], old_uri);
    let response = get_response(Arc::clone(&app), &old_uri, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_body(response).await.as_ref(), b"0123456789");
    {
        let mut state = session.write().await;
        state.handoff_origin_account(HlsOriginAccountBinding::rebound(
            Arc::from("input"),
            Arc::from("next-account"),
            "owner".into(),
            1,
            super::super::super::current_time_millis(),
        ));
    }
    let third = publish(&app, &session, &proxy, &lease,
        "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:12\n#EXTINF:4,\n12.ts\n#EXTINF:4,\n13.ts\n#EXTINF:4,\n14.ts\n#EXTINF:4,\n15.ts\n",
        &format!("{}/live/next.m3u8?turn=3", origin.base.replace("127.0.0.1", "localhost"))).await;
    assert_eq!(media(&third)[0], old_uri);
    assert_eq!(media(&third).len(), 4);
    let response = get_response(Arc::clone(&app), &old_uri, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_body(response).await.as_ref(), b"0123456789");
    assert_eq!(origin.requests.load(Ordering::Relaxed), 1);
    assert!(session
        .read()
        .await
        .transient
        .resources
        .values()
        .filter(|resource| resource.kind == TransientResourceKind::Segment)
        .any(|resource| old_uri.contains(&resource.id.0) && resource.resolved_origin_uri.starts_with(&origin.base)));
}

const FINAL_URLS: [&str; 3] =
    ["/delivery/initial.m3u8?turn=1", "/delivery/reload.m3u8?turn=2", "/delivery/conflict.m3u8?turn=3"];

async fn start_manifest_redirect_origin(
    manifests: [String; 3],
    redirect_index: Arc<AtomicUsize>,
    served_urls: Arc<Mutex<Vec<&'static str>>>,
) -> RangeOrigin {
    RangeOrigin::start_with_routes(
        Router::new()
            .route(
                "/entry.m3u8",
                get(move || {
                    let index = redirect_index.load(Ordering::Acquire);
                    async move { Redirect::temporary(FINAL_URLS[index]) }
                }),
            )
            .route(
                "/delivery/{playlist}",
                get(move |uri: Uri| {
                    let url = uri.path_and_query().unwrap().as_str();
                    let index = FINAL_URLS.iter().position(|&expected| expected == url).unwrap();
                    served_urls.lock().unwrap().push(FINAL_URLS[index]);
                    let manifest = manifests[index].clone();
                    async move { manifest }
                }),
            ),
    )
    .await
}

#[tokio::test]
async fn transient_parts_ranges_and_dependencies_keep_the_same_http_routes_across_manifest_redirects() {
    use super::super::{
        create_active_hls_user_session,
        manifest_entry_freshness::{canonical_response_for_lease, enable_hls_cache_with_startup_mode},
        origin_restart::refresh_playing_lease_with_directive,
        ConfigInput,
    };
    use shared::model::{HlsStartupMode, InputType};
    use tuliprox_hls::api::HlsManifestAcceptanceDirective;

    let headers =
        "#EXTM3U\n#EXT-X-VERSION:9\n#EXT-X-PLAYLIST-TYPE:EVENT\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:10\n";
    let dependency_tags =
        "#EXT-X-MAP:URI=\"/init.mp4\",BYTERANGE=\"3@1\"\n#EXT-X-KEY:METHOD=AES-128,URI=\"/media.key\",IV=0x01\n";
    let overlap =
        "#EXT-X-PART:DURATION=1,URI=\"/part.m4s\",BYTERANGE=\"3@2\"\n#EXTINF:4,\n#EXT-X-BYTERANGE:4@4\n/shared.ts\n";
    let initial = format!("{headers}{dependency_tags}{overlap}");
    let next = format!("{initial}#EXTINF:4,\n/next.ts\n");
    let changed = next.replace("/part.m4s", "/changed.m4s");
    let current = Arc::new(AtomicUsize::new(0));
    let served = Arc::new(Mutex::new(Vec::new()));
    let origin =
        start_manifest_redirect_origin([initial, next, changed], Arc::clone(&current), Arc::clone(&served)).await;

    let input = ConfigInput {
        id: 1,
        name: "transient-redirect-origin".into(),
        input_type: InputType::M3u,
        url: origin.base.clone(),
        max_connections: 1,
        enabled: true,
        ..Default::default()
    };
    let app = test_app_state_with_inputs(vec![Arc::new(input.clone())]);
    enable_hls_cache_with_startup_mode(&app, HlsStartupMode::Conservative).await;
    create_active_hls_user_session(&app).await;
    let entry = format!("{}/entry.m3u8", origin.base);
    let read_manifest = || async {
        let response = canonical_response_for_lease(&app, &input, &entry, "warm").await;
        assert_eq!(response.status(), StatusCode::OK);
        String::from_utf8(response_body(response).await.to_vec()).unwrap()
    };
    let first = read_manifest().await;
    let initial_segment = media(&first)[0];
    let response = get_response(Arc::clone(&app), initial_segment, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_body(response).await.as_ref(), b"0123456789");

    let key = super::super::super::build_hls_origin_source(&input, "12345").session_key();
    let session = app.hls.proxy.sessions().get_by_key(&key).await.unwrap();
    assert!(matches!(session.read().await.mode, HlsSessionMode::TransientPassthrough { .. }));
    let refresh = async |index| {
        current.store(index, Ordering::Release);
        session.write().await.origin_refresh.next_fetch_allowed_at_ms = 0;
        refresh_playing_lease_with_directive(&app, &session, &entry, HlsManifestAcceptanceDirective::none()).await;
    };
    let commits = session.read().await.origin_manifest_commits();
    refresh(1).await;
    assert_eq!(session.read().await.origin_manifest_commits(), commits + 1);
    let second = read_manifest().await;
    assert_eq!(media(&second).len(), 2);
    let mut routes = Vec::new();
    for (tag, range, bytes) in [
        ("#EXT-X-MAP:", Some("bytes=1-3"), &b"123"[..]),
        ("#EXT-X-KEY:", None, &b"0123456789"[..]),
        ("#EXT-X-PART:", Some("bytes=2-4"), &b"234"[..]),
    ] {
        let first_uri = first
            .lines()
            .find(|line| line.starts_with(tag))
            .unwrap()
            .split_once("URI=\"")
            .unwrap()
            .1
            .split_once('"')
            .unwrap()
            .0;
        assert!(second.lines().any(|line| line.starts_with(tag) && line.contains(first_uri)));
        routes.push((first_uri, range, bytes));
    }
    let first_segment = media(&first)[0];
    assert_eq!(first_segment, media(&second)[0]);
    routes.push((first_segment, Some("bytes=4-7"), &b"4567"[..]));
    for (uri, range, expected) in routes {
        let response = get_response(Arc::clone(&app), uri, range).await;
        assert_eq!(response.status(), if range.is_some() { StatusCode::PARTIAL_CONTENT } else { StatusCode::OK });
        assert_eq!(response_body(response).await.as_ref(), expected);
    }

    let previous = session.read().await.transient.last_manifest_body.clone();
    refresh(2).await;
    {
        let state = session.read().await;
        assert_eq!(state.origin_manifest_commits(), commits + 1);
        assert!(state.origin_refresh.consecutive_failures > 0);
        assert_eq!(state.transient.last_manifest_body, previous);
    }
    let rejected = read_manifest().await;
    assert_eq!(media(&rejected), media(&second));
    let mut requests = served.lock().unwrap().clone();
    requests.dedup();
    assert_eq!(requests, FINAL_URLS);
    eprintln!("transient_manifest_redirects={FINAL_URLS:?} accepted_commits=2 conflict_commits=0");
}

#[tokio::test]
async fn distinct_manifest_ranges_preserve_absolute_ranges_and_never_promote_a_partial_fill() {
    let origin = RangeOrigin::start().await;
    let (app, session, proxy, lease) = fixture(&origin).await;
    let body = publish(&app, &session, &proxy, &lease,
        "#EXTM3U\n#EXT-X-VERSION:4\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:10\n#EXT-X-BYTERANGE:5@0\n#EXTINF:4,\nshared.ts\n#EXT-X-BYTERANGE:5\n#EXTINF:4,\nshared.ts\n#EXTINF:4,\nnext.ts\n",
        &format!("{}/live/index.m3u8", origin.base)).await;
    let uris = media(&body);
    assert_ne!(uris[0], uris[1]);
    let OriginManifestParseOutcome::Normal(parsed) =
        parse_origin_media_manifest(&body, &format!("{}/live/index.m3u8", origin.base))
    else {
        panic!("the delivered manifest must expose resolvable ranges");
    };
    let ranges: Vec<_> = parsed
        .segments
        .iter()
        .take(2)
        .map(|segment| {
            let range = segment.origin_byte_range.unwrap();
            format!("bytes={}-{}", range.offset, range.offset + range.length - 1)
        })
        .collect();
    for (uri, range, content_range, expected) in [
        (uris[0], ranges[0].as_str(), "bytes 0-4/10", &b"01234"[..]),
        (uris[1], ranges[1].as_str(), "bytes 5-9/10", &b"56789"[..]),
    ] {
        for _ in 0..2 {
            let response = get_response(Arc::clone(&app), uri, Some(range)).await;
            assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
            assert_eq!(response.headers()[header::CONTENT_RANGE], content_range);
            assert_eq!(response.headers()[header::CONTENT_LENGTH], "5");
            assert_eq!(response_body(response).await.as_ref(), expected);
        }
    }
    assert!(session.read().await.transient.object_cache.is_empty());
    assert_eq!(origin.requests.load(Ordering::Relaxed), 4);
    for uri in [uris[0], uris[1]] {
        let full = get_response(Arc::clone(&app), uri, None).await;
        assert_eq!(full.status(), StatusCode::OK);
        assert_eq!(response_body(full).await.as_ref(), b"0123456789");
    }
    for (uri, range, expected) in
        [(uris[0], ranges[0].as_str(), &b"01234"[..]), (uris[1], ranges[1].as_str(), &b"56789"[..])]
    {
        let response = get_response(Arc::clone(&app), uri, Some(range)).await;
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response_body(response).await.as_ref(), expected);
    }
    assert_eq!(origin.requests.load(Ordering::Relaxed), 6);
}

#[tokio::test]
async fn removed_cached_occurrence_keeps_full_and_range_bytes_when_origin_uri_is_reused() {
    let origin = RangeOrigin::start().await;
    let (app, session, proxy, lease) = fixture(&origin).await;
    let body =
        "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:10\n#EXTINF:4,\nshared.ts\n#EXTINF:4,\nnext.ts\n";
    let url = format!("{}/live/index.m3u8", origin.base);
    let first = publish(&app, &session, &proxy, &lease, body, &url).await;
    let old_uri = media(&first)[0].to_owned();
    let response = get_response(Arc::clone(&app), &old_uri, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_body(response).await.as_ref(), b"0123456789");
    *origin.payload.write().unwrap() = b"ABCDEFGHIJ".to_vec();
    let second = publish(&app, &session, &proxy, &lease, &body.replace("SEQUENCE:10", "SEQUENCE:12"), &url).await;
    let new_uri = media(&second)[0];
    assert_ne!(old_uri, new_uri);
    let response = get_response(Arc::clone(&app), new_uri, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_body(response).await.as_ref(), b"ABCDEFGHIJ");
    app.hls.proxy.run_garbage_collection_once(super::super::super::current_time_millis()).await.unwrap();
    for range in [None, Some("bytes=2-5")] {
        let response = get_response(Arc::clone(&app), &old_uri, range).await;
        if range.is_some() {
            assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
            assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes 2-5/10");
            assert_eq!(response.headers()[header::CONTENT_LENGTH], "4");
            assert_eq!(response_body(response).await.as_ref(), b"2345");
        } else {
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response_body(response).await.as_ref(), b"0123456789");
        }
    }
    assert_eq!(origin.requests.load(Ordering::Relaxed), 2, "old cache bindings never refetch the reused origin URI");
}

fn cache_file_inventory(root: &std::path::Path) -> Vec<(std::path::PathBuf, u64)> {
    let mut directories = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = directories.pop() {
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => panic!("cache inventory: {error}"),
        };
        for entry in entries {
            let entry = entry.unwrap();
            let metadata = entry.metadata().unwrap();
            if metadata.is_dir() {
                directories.push(entry.path());
            } else {
                files.push((entry.path().strip_prefix(root).unwrap().to_path_buf(), metadata.len()));
            }
        }
    }
    files.sort();
    files
}

#[tokio::test]
async fn client_range_passthrough_remains_uncached_after_publication_removal() {
    let origin = RangeOrigin::start().await;
    let (app, session, proxy, lease) = fixture(&origin).await;
    let body =
        "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:10\n#EXTINF:4,\nshared.ts\n#EXTINF:4,\nnext.ts\n";
    let url = format!("{}/live/index.m3u8", origin.base);
    let first = publish(&app, &session, &proxy, &lease, body, &url).await;
    let old_uri = media(&first)[0].to_owned();
    let cache = app.hls.proxy.segment_cache();
    let before = cache.capacity_usage(&proxy).await.unwrap();
    let disk_before = cache_file_inventory(&cache.cache_path());
    assert!(session.read().await.transient.object_cache.is_empty());
    assert_eq!(before.session_bytes, 0);
    for removed in [false, true] {
        if removed {
            publish(&app, &session, &proxy, &lease, &body.replace("SEQUENCE:10", "SEQUENCE:12"), &url).await;
        }
        let response = get_response(Arc::clone(&app), &old_uri, Some("bytes=2-5")).await;
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes 2-5/10");
        assert_eq!(response_body(response).await.as_ref(), b"2345");
        assert!(
            session.read().await.transient.object_cache.is_empty(),
            "the existing client-range direct stream creates no cache entry"
        );
        let after = cache.capacity_usage(&proxy).await.unwrap();
        assert_eq!(after.session_bytes, before.session_bytes);
        assert_eq!(after.global_bytes, before.global_bytes);
        assert!(!cache.has_active_temp_files(), "direct streams do not create a spool");
        assert_eq!(
            cache_file_inventory(&cache.cache_path()),
            disk_before,
            "direct streams persist no additional media files or bytes"
        );
    }
    assert_eq!(origin.requests.load(Ordering::Relaxed), 2, "both range requests use the direct origin path");
}

#[tokio::test]
async fn removed_http_media_survives_capacity_pressure_without_spooling_direct_ranges() {
    use sha2::{Digest, Sha256};
    use tuliprox_core::utils::current_time_millis;
    use tuliprox_hls::api::TransientResourceFile;

    let origin = RangeOrigin::start().await;
    let (app, session, proxy, lease) = fixture(&origin).await;
    session.write().await.transient.set_resource_ttl_ms(1_000);
    let body =
        "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:10\n#EXTINF:4,\nshared.ts\n#EXTINF:4,\nnext.ts\n";
    let url = format!("{}/live/index.m3u8", origin.base);
    let first = publish(&app, &session, &proxy, &lease, body, &url).await;
    let old = media(&first)[0].to_owned();
    let response = get_response(Arc::clone(&app), &old, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let expected = Sha256::digest(b"0123456789");
    assert_eq!(Sha256::digest(response_body(response).await), expected);
    let old_resource = TransientResourceFile::parse(old.rsplit('/').next().unwrap()).unwrap().resource_id;
    // Keep the next window fetchable after the old mappings expire.
    session.write().await.transient.set_resource_ttl_ms(30_000);
    let second = publish(&app, &session, &proxy, &lease, &body.replace("SEQUENCE:10", "SEQUENCE:12"), &url).await;
    assert!(!media(&second).contains(&old.as_str()));
    let new = media(&second)[0];
    let snapshot = app
        .hls
        .proxy
        .access_lease_response_snapshot(&lease, &proxy, current_time_millis())
        .await
        .unwrap()
        .last_manifest_snapshot
        .unwrap();
    let removal = snapshot.delivered_at_ms;
    assert_eq!(snapshot.playlist_duration_ms, 8_000);
    let retention_deadline = removal + 4_000 + 8_000;
    let mapping_expiry = {
        let state = session.read().await;
        let resource = &state.transient.resources[&old_resource];
        assert_eq!(resource.expires_at_ms, resource.created_at_ms + 1_000);
        resource.expires_at_ms
    };

    let mut config = app.app_config.config.load_full().as_ref().clone();
    let limits = config.reverse_proxy.as_mut().unwrap().hls_cache.as_mut().unwrap();
    limits.cache_bytes = shared::model::Bytes::new(9);
    limits.cache_bytes_str = "9B".into();
    limits.cache_bytes_per_session = limits.cache_bytes;
    limits.cache_bytes_per_session_str.clone_from(&limits.cache_bytes_str);
    app.app_config.config.store(Arc::new(config));
    app.hls.proxy.update_config(&app.app_config).await;
    let gc_policy = app.hls.proxy.gc_policy();
    assert_eq!((gc_policy.cache_bytes_per_session, gc_policy.cache_bytes_global), (9, 9));
    let cache = app.hls.proxy.segment_cache();
    let before = cache.capacity_usage(&proxy).await.unwrap();
    assert_eq!((before.session_bytes, before.global_bytes), (10, 10));

    // The HTTP endpoint and projected-write reclamation read the wall clock themselves.
    tokio::time::sleep(std::time::Duration::from_millis((mapping_expiry + 10).saturating_sub(current_time_millis())))
        .await;
    let checked_now = || {
        let now = current_time_millis();
        assert!(mapping_expiry < now && now < retention_deadline, "outside the retained-only window: {now}");
        now
    };
    let gc_ms = checked_now();
    let report = app.hls.proxy.run_garbage_collection_once(gc_ms).await.unwrap();
    assert_eq!(report.cache_object_deletions_planned, 0, "{report:?}");
    assert_eq!(report.cache_object_deletions_succeeded, 0, "{report:?}");
    let disk = cache_file_inventory(&cache.cache_path());
    let incoming = tuliprox_hls::api::SegmentCacheKey::new(proxy.clone(), 990, "ts");
    let capacity_ms = checked_now();
    let error = cache.write_bytes_and_commit(&incoming, b"blocked").await.unwrap_err();
    assert!(error.to_string().contains("hls cache capacity unavailable"), "existing capacity deferral: {error}");
    assert!(cache.metadata(&incoming).await.unwrap().is_none());
    let full_ms = checked_now();
    let full = get_response(Arc::clone(&app), &old, None).await;
    assert_eq!(full.status(), StatusCode::OK);
    assert_eq!(Sha256::digest(response_body(full).await), expected);
    let range_ms = checked_now();
    let partial = get_response(Arc::clone(&app), &old, Some("bytes=2-5")).await;
    assert_eq!(partial.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(partial.headers()[header::CONTENT_RANGE], "bytes 2-5/10");
    assert_eq!(partial.headers()[header::CONTENT_LENGTH], "4");
    assert_eq!(response_body(partial).await.as_ref(), b"2345");
    assert_eq!(origin.requests.load(Ordering::Relaxed), 1);
    *origin.payload.write().unwrap() = b"ABCDEFGHIJ".to_vec();
    let direct = get_response(Arc::clone(&app), new, Some("bytes=2-5")).await;
    assert_eq!(direct.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(response_body(direct).await.as_ref(), b"CDEF");
    assert_eq!(
        origin.requests.load(Ordering::Relaxed),
        2,
        "only the direct range reaches origin after the initial fill"
    );
    let after = cache.capacity_usage(&proxy).await.unwrap();
    assert_eq!((after.session_bytes, after.global_bytes), (10, 10));
    assert_eq!(session.read().await.transient.object_cache.len(), 1);
    assert_eq!(cache_file_inventory(&cache.cache_path()), disk);
    assert!(!cache.has_active_temp_files());
    let finished_ms = checked_now();
    eprintln!(
        "removal_ms={removal} mapping_expiry_ms={mapping_expiry} retention_deadline_ms={retention_deadline} \
         gc_ms={gc_ms} capacity_ms={capacity_ms} full_ms={full_ms} range_ms={range_ms} finished_ms={finished_ms} \
         deleted=0 cache_bytes=10/10 limits=9/9 origin_gets=2 old_digest={expected:02x?}"
    );
}
