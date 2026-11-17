use super::{super::support::spawn_test_binary_origin_with_request, *};
use crate::api::model::{AppState, HlsAccessLeaseId, HlsSessionHandle, MapCacheStatus, SegmentCacheStatus};
use sha2::{Digest, Sha256};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};

const ORIGINAL: &str = "/media/105.ts?variant=old";
const ROTATED: &str = "/media/105.ts?variant=next";
const REPLACEMENT: &str = "/replacement/105.ts?variant=old";
const RETRY_MAP: &str = "#EXT-X-MAP:URI=\"/retry-init.mp4\"";

fn window(first: u64, count: u64) -> String {
    let mut body = format!("#EXTM3U\n#EXT-X-VERSION:4\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:{first}\n");
    for msn in first..first + count {
        writeln!(body, "#EXTINF:4,\n/media/{msn}.ts?variant=old").unwrap();
    }
    body
}

fn uris(body: &str) -> Vec<&str> { body.lines().filter(|line| line.starts_with("/hls/")).collect() }

struct Script {
    manifest: String,
    candidate_bytes: Arc<[u8]>,
    manifest_budget: Option<usize>,
    delivery_query: &'static str,
    key_failure_status: StatusCode,
    map_failure_gate: Option<tokio::sync::watch::Receiver<bool>>,
}

struct OriginGet {
    path: String,
    range: Option<String>,
}

fn fixture_dependency_response(
    path: &str,
    resource_path: &str,
    range: Option<&str>,
) -> Option<TestBinaryOriginResponse> {
    if matches!(resource_path, "/retry-init.mp4" | "/retry-access.key") {
        let (status, bytes) = if path.ends_with("?token=fresh") {
            (StatusCode::OK, &b"0123456789abcdef"[..])
        } else {
            (StatusCode::NOT_FOUND, &[][..])
        };
        return Some(TestBinaryOriginResponse::new(status, Arc::from(bytes)));
    }
    if path_has_extension(resource_path, "mp4") {
        let payload = if path.starts_with("/next-init") { b"CCCCDDDD" } else { b"AAAABBBB" };
        let (start, end) = match range {
            Some("bytes=0-3") => (0, 3),
            Some("bytes=4-7") => (4, 7),
            _ => panic!("unexpected fixture MAP range"),
        };
        let mut response = TestBinaryOriginResponse::new(StatusCode::PARTIAL_CONTENT, Arc::from(&payload[start..=end]));
        response.content_range = Some(format!("bytes {start}-{end}/8"));
        return Some(response);
    }
    if path_has_extension(resource_path, "key") {
        let key = if path.ends_with("key=next") { b"fedcba9876543210" } else { b"0123456789abcdef" };
        return Some(TestBinaryOriginResponse::new(StatusCode::OK, Arc::from(&key[..])));
    }
    None
}

impl OriginGet {
    fn is_candidate_probe(&self) -> bool { self.range.is_some() || matches!(self.path.as_str(), ROTATED | REPLACEMENT) }
}

struct Playback {
    app: Arc<AppState>,
    input: ConfigInput,
    url: String,
    session: HlsSessionHandle,
    first: String,
    original_bytes: Arc<[u8]>,
    script: Arc<Mutex<Script>>,
    requests: Arc<Mutex<Vec<OriginGet>>>,
    _origin: super::super::TestSegmentOrigin,
}

impl Playback {
    async fn start() -> Self { Self::with_initial_window(window(100, 6), media(1).into()).await }

    async fn with_initial_window(manifest: String, original_bytes: Arc<[u8]>) -> Self {
        trace_fixture();
        let script = Arc::new(Mutex::new(Script {
            manifest,
            candidate_bytes: Arc::clone(&original_bytes),
            manifest_budget: None,
            delivery_query: "",
            key_failure_status: StatusCode::NOT_FOUND,
            map_failure_gate: None,
        }));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let served = Arc::clone(&script);
        let counted = Arc::clone(&requests);
        let original = Arc::clone(&original_bytes);
        let manifest_serial = AtomicUsize::new(0);
        let origin = spawn_test_binary_origin_with_request(Arc::new(move |path, request| {
            let range = request
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find_map(|(name, value)| name.eq_ignore_ascii_case("range").then(|| value.trim()))
                .map(str::to_owned);
            let is_range = range.is_some();
            counted.lock().unwrap().push(OriginGet { path: path.to_owned(), range: range.clone() });
            let resource_path = path.split_once('?').map_or(path, |(path, _)| path);
            if let Some(mut response) = fixture_dependency_response(path, resource_path, range.as_deref()) {
                if path == "/retry-init.mp4?token=old" {
                    response.gate.clone_from(&served.lock().unwrap().map_failure_gate);
                } else if path == "/retry-access.key?token=old" {
                    response.status = served.lock().unwrap().key_failure_status;
                }
                return response;
            }
            if is_range {
                return TestBinaryOriginResponse::new(StatusCode::RANGE_NOT_SATISFIABLE, Arc::from([]));
            }
            if path == "/index.m3u8" {
                let serial = manifest_serial.fetch_add(1, Ordering::Relaxed);
                let query = served.lock().unwrap().delivery_query;
                let target = if query.is_empty() {
                    format!("/delivery/{serial}.m3u8?turn={serial}")
                } else {
                    format!("/delivery/{serial}.m3u8?{query}")
                };
                return TestBinaryOriginResponse::redirect(target);
            }
            let mut response = served.lock().unwrap();
            if path_has_extension(resource_path, "m3u8") {
                if let Some(remaining) = &mut response.manifest_budget {
                    if *remaining == 0 {
                        return TestBinaryOriginResponse::new(StatusCode::SERVICE_UNAVAILABLE, Arc::from([]));
                    }
                    *remaining -= 1;
                }
            }
            let bytes: Arc<[u8]> = if path_has_extension(resource_path, "m3u8") {
                Arc::from(response.manifest.as_bytes())
            } else if matches!(path, ROTATED | REPLACEMENT) {
                Arc::clone(&response.candidate_bytes)
            } else {
                Arc::clone(&original)
            };
            TestBinaryOriginResponse::new(StatusCode::OK, bytes)
        }))
        .await;
        let input = ConfigInput {
            id: 1,
            name: "contract-origin".into(),
            input_type: InputType::M3u,
            url: origin.base_url.clone(),
            max_connections: 1,
            enabled: true,
            ..Default::default()
        };
        let app = test_app_state_with_inputs(vec![Arc::new(input.clone())]);
        enable_unstripped_cache(&app, HlsStartupMode::Conservative).await;
        let mut config = app.app_config.config.load_full().as_ref().clone();
        let cache = config.reverse_proxy.as_mut().unwrap().hls_cache.as_mut().unwrap();
        cache.manifest_recovery_burst.level = shared::model::HlsManifestRecoveryBurstLevel::Friendly;
        // Complete the fixture's initial media transfers even when the local test runner is busy.
        cache.origin_segment_timeout_ms = shared::model::HlsSegmentTimeout::Fixed(shared::model::Millis::new(10_000));
        app.app_config.config.store(Arc::new(config));
        app.hls.proxy.update_config(&app.app_config).await;
        create_active_hls_user_session(&app).await;
        let url = format!("{}/index.m3u8", origin.base_url);
        let first = read_manifest(&app, &input, &url, "initial publication").await;
        assert_eq!(uris(&first).len(), 6);
        let key = super::super::super::build_hls_origin_source(&input, "12345").session_key();
        let session = app.hls.proxy.sessions().get_by_key(&key).await.unwrap();
        let fixture = Self { app, input, url, session, first, original_bytes, script, requests, _origin: origin };
        fixture.assert_bytes(uris(&fixture.first)[0]).await;
        fixture.requests.lock().unwrap().clear();
        fixture
    }

    async fn reload(&self, manifest: String, bytes: Arc<[u8]>) { self.reload_with_budget(manifest, bytes, None).await; }

    async fn reload_with_budget(&self, manifest: String, bytes: Arc<[u8]>, manifest_budget: Option<usize>) {
        *self.script.lock().unwrap() = Script {
            manifest,
            candidate_bytes: bytes,
            manifest_budget,
            delivery_query: "",
            key_failure_status: StatusCode::NOT_FOUND,
            map_failure_gate: None,
        };
        self.refresh().await;
    }

    async fn refresh(&self) {
        self.session.write().await.origin_refresh.next_fetch_allowed_at_ms = 0;
        refresh_playing_lease(&self.app, &self.session, &self.url).await;
    }

    async fn reload_with_delivery_query(&self, manifest: String, delivery_query: &'static str) {
        {
            let mut script = self.script.lock().unwrap();
            script.manifest = manifest;
            script.delivery_query = delivery_query;
        }
        self.refresh().await;
    }

    async fn wait_for_ready(&self, seq: u64) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if matches!(self.session.read().await.segments[&seq].status, SegmentCacheStatus::Ready { .. }) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the advanced window finishes fetching before playback");
    }

    async fn manifest(&self) -> String {
        // Manifest reads observe playback. Only reload_with_budget advances the origin script.
        self.session.write().await.origin_refresh.next_fetch_allowed_at_ms = u64::MAX;
        read_manifest(&self.app, &self.input, &self.url, "published window").await
    }

    async fn wait_for_dependency(&self, tag: &str) -> String {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let manifest = self.manifest().await;
                if manifest.lines().any(|line| line.starts_with(tag)) {
                    break last_dependency_uri(&manifest, tag).to_owned();
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the lease publishes the ready dependency")
    }

    async fn assert_bytes(&self, uri: &str) {
        assert_eq!(Sha256::digest(get_bound_media(&self.app, uri).await), Sha256::digest(&self.original_bytes));
    }

    fn count(&self, predicate: impl Fn(&str) -> bool) -> usize {
        self.requests.lock().unwrap().iter().filter(|request| predicate(&request.path)).count()
    }

    async fn lease(&self) -> tuliprox_hls::api::HlsAccessLease {
        let proxy = self.session.read().await.proxy_session_id.clone();
        self.app
            .hls
            .proxy
            .access_lease_response_snapshot(
                &HlsAccessLeaseId("warm".into()),
                &proxy,
                super::super::super::current_time_millis(),
            )
            .await
            .unwrap()
    }
}

async fn read_manifest(app: &Arc<AppState>, input: &ConfigInput, url: &str, phase: &str) -> String {
    let response = canonical_response_for_lease(app, input, url, "warm").await;
    assert_eq!(response.status(), StatusCode::OK, "HLS manifest fixture: {phase}");
    let body = String::from_utf8(response_body(response).await.to_vec()).unwrap();
    assert!(!body.contains("#EXT-X-ENDLIST"), "positive reserve keeps the lease live");
    body
}

async fn rejected_candidate(
    playback: &Playback,
    candidate: String,
    bytes: Arc<[u8]>,
    expected_proof: bool,
    manifest_budget: Option<usize>,
) {
    let before = playback.session.read().await;
    let commits = before.origin_manifest_commits();
    let next = before.proxy_next_seq;
    let before_entries = before
        .segments
        .values()
        .map(|e| (e.proxy_seq, e.cache_key.clone(), e.origin_fetch_ref.clone()))
        .collect::<Vec<_>>();
    drop(before);
    let lease_before = playback.lease().await;
    eprintln!(
        "held_digest={:02x?} candidate_digest={:02x?}",
        Sha256::digest(&playback.original_bytes),
        Sha256::digest(&bytes)
    );
    playback.reload_with_budget(candidate, bytes, manifest_budget).await;
    let after = playback.session.read().await;
    assert_eq!(after.origin_manifest_commits(), commits);
    assert_eq!(after.origin_epoch, 0);
    assert_eq!(after.proxy_next_seq, next);
    assert_eq!(
        after
            .segments
            .values()
            .map(|e| (e.proxy_seq, e.cache_key.clone(), e.origin_fetch_ref.clone()))
            .collect::<Vec<_>>(),
        before_entries
    );
    let recovery =
        after.origin_control.acceptance_episode.as_ref().expect("continuity rejection starts existing recovery");
    assert!(recovery.full_burst_completed);
    assert!(recovery.exhaustion_reason().is_some());
    let outcome = recovery.exhaustion_reason();
    drop(after);
    let lease = playback.lease().await;
    assert_eq!(lease.playback_mode, lease_before.playback_mode);
    assert_eq!(lease.last_manifest_snapshot, lease_before.last_manifest_snapshot);
    let retained = playback.manifest().await;
    assert_eq!(uris(&retained), uris(&playback.first));
    playback.assert_bytes(uris(&playback.first).last().unwrap()).await;
    let requests = playback.requests.lock().unwrap();
    let manifests = requests.iter().filter(|r| r.path == "/index.m3u8" || r.path.starts_with("/delivery/")).count();
    let probes = requests.iter().filter(|r| r.is_candidate_probe()).count();
    assert!(manifests > 1);
    assert_eq!(probes > 0, expected_proof);
    assert_eq!(requests.len(), manifests + probes, "held bytes do not refetch");
    eprintln!("recovery={outcome:?} manifest_requests={manifests} candidate_gets={probes}");
}

#[tokio::test]
async fn missing_selected_anchor_keeps_the_published_playback_window() {
    let playback = Playback::start().await;
    rejected_candidate(&playback, window(100, 5), Arc::clone(&playback.original_bytes), false, None).await;
}

#[tokio::test]
async fn forward_window_without_the_required_successor_cannot_skip_the_gap() {
    let playback = Playback::start().await;
    // The initial forward window omits both the anchor and the required successor.
    // Recovery cannot authorize a disjoint window when later origin requests fail.
    rejected_candidate(&playback, window(107, 6), Arc::clone(&playback.original_bytes), false, Some(1)).await;
}

#[tokio::test]
async fn retained_anchor_with_a_missing_successor_rejects_then_accepts_the_contiguous_reload() {
    let playback = Playback::start().await;
    let mut rejection = playback.session.write().await.skip_manifest_successor_for_test(106);
    // Wire MSNs are implicit. The test hook turns the last parsed MSN into 107 while preserving 105.
    let candidate = window(100, 7).replace("/media/106.ts?", "/media/107.ts?");
    rejected_candidate(&playback, candidate, Arc::clone(&playback.original_bytes), false, Some(1)).await;
    let reason = rejection.try_recv().unwrap().expect("normal commit must report the rejection");
    assert_eq!(reason, "origin-cache-successor-missing previous=105 candidate=107");

    let manifests = playback.count(|path| path.starts_with("/delivery/"));
    playback.reload(window(101, 6), Arc::clone(&playback.original_bytes)).await;
    playback.wait_for_ready(6).await;
    let next = playback.manifest().await;
    assert_eq!(manifest_media_sequence(&next), 1);
    assert_eq!(&uris(&next)[..5], &uris(&playback.first)[1..]);
    playback.assert_bytes(uris(&playback.first)[5]).await;
    playback.assert_bytes(uris(&next)[5]).await;
    assert_eq!(playback.session.read().await.origin_epoch, 0);
    assert_eq!(playback.count(|path| path.starts_with("/delivery/")), manifests + 1);
    assert_eq!(playback.count(|path| path == "/media/107.ts?variant=old"), 0);
    assert_eq!(playback.count(|path| path.ends_with("?variant=old")), 1);
    assert_eq!(playback.count(|path| path == "/media/106.ts?variant=old"), 1);
    eprintln!("successor_gap reason={reason} manifests={} rejected_media_gets=0 successor_gets=1", manifests + 1);
}

#[tokio::test]
async fn conflicting_path_with_different_media_preserves_the_old_uri() {
    let playback = Playback::start().await;
    let candidate = window(100, 6).replace(ORIGINAL, REPLACEMENT);
    let changed = media(2);
    assert_ne!(Sha256::digest(&changed), Sha256::digest(&playback.original_bytes));
    rejected_candidate(&playback, candidate, changed.into(), true, None).await;
    let requests = playback.requests.lock().unwrap();
    assert!(requests.iter().filter(|r| r.is_candidate_probe()).all(|r| r.path == REPLACEMENT && r.range.is_none()));
    eprintln!("path_only original={ORIGINAL} candidate={REPLACEMENT} range=none");
}

#[tokio::test]
async fn query_rotation_with_only_the_last_byte_changed_cannot_rebind() {
    let playback = Playback::start().await;
    let mut changed = playback.original_bytes.to_vec();
    *changed.last_mut().unwrap() ^= 1;
    assert_eq!(&changed[..changed.len() - 1], &playback.original_bytes[..changed.len() - 1]);
    assert_ne!(Sha256::digest(&changed), Sha256::digest(&playback.original_bytes));
    rejected_candidate(&playback, window(100, 6).replace(ORIGINAL, ROTATED), changed.into(), true, None).await;
}

#[tokio::test]
async fn conflicting_range_with_an_unavailable_slice_cannot_rebind() {
    let playback = Playback::start().await;
    let candidate = window(100, 6).replace(ORIGINAL, &format!("#EXT-X-BYTERANGE:188@999999\n{ORIGINAL}"));
    rejected_candidate(&playback, candidate, Arc::clone(&playback.original_bytes), true, None).await;
    let requests = playback.requests.lock().unwrap();
    assert!(requests
        .iter()
        .filter(|r| r.is_candidate_probe())
        .all(|r| { r.path == ORIGINAL && r.range.as_deref() == Some("bytes=999999-1000186") }));
    eprintln!("range_only uri={ORIGINAL} range=bytes=999999-1000186 status=416");
}

#[tokio::test]
async fn query_rotation_without_a_complete_reference_does_not_probe_or_rebind() {
    let playback = Playback::start().await;
    playback.session.write().await.clear_origin_content_evidence_for_test();
    rejected_candidate(
        &playback,
        window(100, 6).replace(ORIGINAL, ROTATED),
        Arc::clone(&playback.original_bytes),
        false,
        None,
    )
    .await;
}

async fn accept_equal_binding(playback: &Playback) {
    let (before, generation) = {
        let state = playback.session.read().await;
        (state.origin_manifest_commits(), state.origin_control.acceptance_generation)
    };
    playback.reload(window(100, 6).replace(ORIGINAL, ROTATED), Arc::clone(&playback.original_bytes)).await;
    let state = playback.session.read().await;
    assert!(state.origin_manifest_commits() > before);
    assert_ne!(state.origin_control.acceptance_generation, generation, "content proof belongs to explicit recovery");
    assert_eq!(state.origin_epoch, 0);
    assert_eq!(state.proxy_next_seq, Some(6));
    assert!(state.segments[&5].origin_fetch_ref.as_ref().unwrap().resolved_origin_url.ends_with(ORIGINAL));
    assert_eq!(playback.count(|p| p == ROTATED), 1);
    eprintln!(
        "qualified_binding complete_bytes={} digest={:02x?} candidate_gets=1",
        playback.original_bytes.len(),
        Sha256::digest(&playback.original_bytes)
    );
    assert_eq!(playback.count(|p| p.ends_with("?variant=old")), 0);
    assert!(playback.count(|p| p.starts_with("/delivery/")) > 1);
    drop(state);
    assert_eq!(uris(&playback.manifest().await), uris(&playback.first));
}

#[tokio::test]
async fn query_rotation_with_equal_complete_bytes_reuses_the_published_cache_binding() {
    let playback = Playback::start().await;
    accept_equal_binding(&playback).await;
    playback.assert_bytes(uris(&playback.first)[5]).await;
    assert_eq!(playback.count(|p| p == ROTATED), 1);
}

#[tokio::test]
async fn confirmed_query_binding_accepts_the_next_reload_without_another_proof() {
    let playback = Playback::start().await;
    accept_equal_binding(&playback).await;
    let generation = playback.session.read().await.origin_control.acceptance_generation;
    let manifests = playback.count(|p| p.starts_with("/delivery/"));
    playback.reload(window(101, 6).replace(ORIGINAL, ROTATED), Arc::clone(&playback.original_bytes)).await;
    let state = playback.session.read().await;
    assert_eq!(state.origin_control.acceptance_generation, generation);
    assert_eq!(state.origin_epoch, 0);
    assert_eq!(playback.count(|p| p == ROTATED), 1);
    assert_eq!(playback.count(|p| p.starts_with("/delivery/")), manifests + 1);
    drop(state);
    playback.wait_for_ready(6).await;
    let next = playback.manifest().await;
    assert_eq!(manifest_media_sequence(&next), 1);
    assert_eq!(&uris(&next)[..5], &uris(&playback.first)[1..]);
    playback.assert_bytes(uris(&playback.first)[5]).await;
    assert_eq!(playback.count(|p| p == "/media/106.ts?variant=old"), 1);
    assert_eq!(playback.count(|p| p == ROTATED), 1);
    eprintln!("confirmed_reload manifests={} proof_gets=1 new_successor_gets=1", manifests + 1);
}

#[tokio::test]
async fn delayed_old_window_cannot_roll_back_the_advanced_live_lease() {
    let mut playback = Playback::start().await;
    playback.reload(window(101, 6), Arc::clone(&playback.original_bytes)).await;
    playback.wait_for_ready(6).await;
    playback.first = playback.manifest().await;
    assert!(manifest_media_sequence(&playback.first) > 0);
    playback.requests.lock().unwrap().clear();
    rejected_candidate(&playback, window(100, 6), Arc::clone(&playback.original_bytes), false, None).await;
}

fn encrypted_window(first: u64, map_path: &str, map_offset: u64, key_query: &str) -> String {
    let media = window(first, 6).replace(".ts?", ".m4s?");
    let media_tag = media.find("#EXTINF:").unwrap();
    format!(
        "{}#EXT-X-MAP:URI=\"/{map_path}.mp4\",BYTERANGE=\"4@{map_offset}\"\n#EXT-X-KEY:METHOD=AES-128,URI=\"/media.key?key={key_query}\",IV=0x01010101010101010101010101010101\n{}",
        &media[..media_tag],
        &media[media_tag..]
    )
}

fn last_dependency_uri<'a>(body: &'a str, tag: &str) -> &'a str {
    body.lines()
        .rev()
        .find(|line| line.starts_with(tag))
        .and_then(|line| line.split_once("URI=\""))
        .and_then(|(_, tail)| tail.split_once('"'))
        .unwrap()
        .0
}

#[tokio::test]
async fn published_map_key_and_media_survive_redirects_and_real_dependency_changes() {
    let bytes: Arc<[u8]> =
        super::super::encrypt_test_aes128_cbc_pkcs7(b"synthetic encrypted media", b"0123456789abcdef", [1; 16]).into();
    let playback = Playback::with_initial_window(encrypted_window(100, "init", 0, "old"), Arc::clone(&bytes)).await;
    let original_map = last_dependency_uri(&playback.first, "#EXT-X-MAP:");
    let original_key = last_dependency_uri(&playback.first, "#EXT-X-KEY:");
    let original_media = uris(&playback.first)[5];
    let mut previous_map = original_map.to_owned();
    let mut previous_key = original_key.to_owned();

    for (slot, map_path, offset, key_query, map_changed, key_changed, map_bytes, key_bytes) in [
        (6, "init", 0, "old", false, false, b"AAAA", b"0123456789abcdef"),
        (7, "next-init", 0, "old", true, false, b"CCCC", b"0123456789abcdef"),
        (8, "next-init", 4, "old", true, false, b"DDDD", b"0123456789abcdef"),
        (9, "next-init", 4, "next", false, true, b"DDDD", b"fedcba9876543210"),
    ] {
        playback.reload(encrypted_window(95 + slot, map_path, offset, key_query), Arc::clone(&bytes)).await;
        playback.wait_for_ready(slot).await;
        // MAP readiness is scheduled separately from the media fill.
        let body = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let body = playback.manifest().await;
                let start = manifest_media_sequence(&body);
                let end = start + u64::try_from(uris(&body).len()).unwrap();
                if (start..end).contains(&slot) {
                    break body;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the lease publishes the new segment with its ready dependencies");
        let map = last_dependency_uri(&body, "#EXT-X-MAP:");
        let key = last_dependency_uri(&body, "#EXT-X-KEY:");
        assert_eq!(map != previous_map, map_changed, "MAP at slot {slot}");
        assert_eq!(key != previous_key, key_changed, "KEY at slot {slot}");
        for (uri, expected) in [
            (map, &map_bytes[..]),
            (key, &key_bytes[..]),
            (original_map, &b"AAAA"[..]),
            (original_key, &b"0123456789abcdef"[..]),
        ] {
            let response = super::super::get_response(Arc::clone(&playback.app), uri, None).await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response_body(response).await.as_ref(), expected);
        }
        playback.assert_bytes(original_media).await;
        previous_map = map.to_owned();
        previous_key = key.to_owned();
    }
    assert_eq!(playback.count(|path| path == "/init.mp4"), 0, "the original MAP stays cached");
    assert_eq!(playback.count(|path| path == "/media.key?key=old"), 0, "the original KEY stays cached");
    assert_eq!(playback.count(|path| path == "/media.key?key=next"), 1);
    assert_eq!(playback.count(|path| path == "/next-init.mp4"), 2, "distinct MAP ranges each fetch once");
}

fn window_with_tail_dependency(first: u64, dependency: &str) -> String {
    window(first, 6).replace("#EXTINF:4,\n/media/106.ts", &format!("{dependency}\n#EXTINF:4,\n/media/106.ts"))
}

#[tokio::test]
async fn unfetched_key_uses_the_refreshed_manifest_transport() {
    assert_key_transport_refresh(StatusCode::NOT_FOUND, 1).await;
}

#[tokio::test]
async fn exhausted_key_retries_use_the_refreshed_manifest_transport() {
    assert_key_transport_refresh(StatusCode::SERVICE_UNAVAILABLE, 5).await;
}

#[tokio::test]
async fn rejected_key_request_uses_the_refreshed_manifest_transport() {
    assert_key_transport_refresh(StatusCode::IM_A_TEAPOT, 1).await;
}

async fn assert_key_transport_refresh(failure_status: StatusCode, old_attempts: usize) {
    use tuliprox_hls::api::{TransientObjectUnavailableState, TransientPassthroughState};

    const KEY: &str = "#EXT-X-KEY:METHOD=AES-128,URI=\"/retry-access.key\",IV=0x01010101010101010101010101010101";
    let playback = Playback::start().await;
    playback.script.lock().unwrap().key_failure_status = failure_status;
    playback.reload_with_delivery_query(window_with_tail_dependency(101, KEY), "token=old").await;
    let (old_key, media_failures) = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let state = playback.session.read().await;
            let entry = &state.segments[&6];
            let encryption = entry.encryption.as_ref().unwrap();
            let cache_key = TransientPassthroughState::transient_object_key(
                &state.proxy_session_id,
                &encryption.resource_id,
                &encryption.resource_extension,
            );
            if matches!(entry.status, SegmentCacheStatus::Discovered | SegmentCacheStatus::FailedRetryable { .. })
                && matches!(
                    state.transient.object_unavailable_state(&cache_key, super::super::super::current_time_millis()),
                    TransientObjectUnavailableState::FailedRetryable { .. }
                )
            {
                break (encryption.resource_id.clone(), state.segment_failure_tracker.consecutive_temporary_failures);
            }
            drop(state);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the first key transport fails before the refresh");
    assert_eq!(playback.count(|path| path == "/retry-access.key?token=old"), old_attempts);
    assert_eq!(playback.count(|path| path.starts_with("/media/106.ts?")), 0);

    playback.reload_with_delivery_query(window_with_tail_dependency(102, KEY), "token=fresh").await;
    playback.wait_for_ready(6).await;
    playback.wait_for_ready(7).await;
    assert_eq!(media_failures, 0, "a failed key transfer does not count as a media failure");
    let current = playback.session.read().await;
    assert_eq!(current.segments[&7].encryption.as_ref().unwrap().resource_id, old_key);
    drop(current);
    let uri = playback.wait_for_dependency("#EXT-X-KEY:").await;
    assert_eq!(get_bound_media(&playback.app, &uri).await.as_ref(), b"0123456789abcdef");
    assert_eq!(playback.count(|path| path.starts_with("/media/106.ts?")), 1);
    assert_eq!(playback.count(|path| path.starts_with("/media/107.ts?")), 1);
    playback.reload_with_delivery_query(window_with_tail_dependency(103, KEY), "token=rotated").await;
    playback.wait_for_ready(8).await;
    assert_eq!(playback.wait_for_dependency("#EXT-X-KEY:").await, uri);
    assert_eq!(get_bound_media(&playback.app, &uri).await.as_ref(), b"0123456789abcdef");
    assert_eq!(playback.count(|path| path == "/retry-access.key?token=rotated"), 0);
    assert_eq!(playback.count(|path| path == "/retry-access.key?token=fresh"), 1);
    assert_eq!(playback.count(|path| path == "/retry-access.key?token=old"), old_attempts);
}

#[tokio::test]
async fn failed_map_retries_with_the_refreshed_manifest_transport() {
    let playback = Playback::start().await;
    let manifest = window_with_tail_dependency(101, RETRY_MAP);
    playback.reload_with_delivery_query(manifest.clone(), "token=old").await;
    let map_id = playback.session.read().await.segments[&6].map_ref.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if matches!(
                playback.session.read().await.maps[&map_id].status,
                MapCacheStatus::FailedPermanent { status: Some(StatusCode::NOT_FOUND), .. }
            ) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the first map transport fails before the refresh");

    // Even when the media is ready and the window stays put, a MAP with a
    // new transport can finish without downloading that media again.
    playback.wait_for_ready(6).await;
    playback.reload_with_delivery_query(manifest.clone(), "token=old").await;
    assert_eq!(playback.count(|path| path == "/retry-init.mp4?token=old"), 1);
    playback.reload_with_delivery_query(manifest, "token=fresh").await;
    assert_retry_map_published(&playback, map_id).await;
    assert_eq!(playback.count(|path| path.starts_with("/media/106.ts?")), 1);
}

#[tokio::test]
async fn late_map_failure_does_not_block_the_refreshed_manifest_transport() {
    let playback = Playback::start().await;
    let (release, hold) = tokio::sync::watch::channel(false);
    playback.script.lock().unwrap().map_failure_gate = Some(hold);
    let manifest = window_with_tail_dependency(101, RETRY_MAP);
    playback.reload_with_delivery_query(manifest.clone(), "token=old").await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if playback.count(|path| path == "/retry-init.mp4?token=old") == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the old MAP fetch reaches the origin");
    playback.wait_for_ready(6).await;
    let map_id = playback.session.read().await.segments[&6].map_ref.unwrap();
    playback.reload_with_delivery_query(manifest, "token=fresh").await;
    assert!(matches!(playback.session.read().await.maps[&map_id].status, MapCacheStatus::Fetching { .. }));
    assert_eq!(playback.count(|path| path == "/retry-init.mp4?token=fresh"), 0);
    release.send(true).unwrap();
    assert_retry_map_published(&playback, map_id).await;
}

async fn assert_retry_map_published(playback: &Playback, map_id: crate::api::model::ProxyMapId) {
    let uri = playback.wait_for_dependency("#EXT-X-MAP:").await;
    assert_eq!(playback.session.read().await.segments[&6].map_ref, Some(map_id));
    assert_eq!(get_bound_media(&playback.app, &uri).await.as_ref(), b"0123456789abcdef");
    assert_eq!(playback.count(|path| path == "/retry-init.mp4?token=old"), 1);
    assert_eq!(playback.count(|path| path == "/retry-init.mp4?token=fresh"), 1);
}
