use super::super::{
    create_active_hls_user_session, get_response,
    manifest_entry_freshness::{
        canonical_response_for_lease, canonical_response_for_lease_with_key, enable_hls_cache_with_startup_mode,
    },
    manifest_media_sequence,
    origin_restart::{refresh_playing_lease_with_directive, refresh_playing_lease_with_directive_for},
    path_has_extension, prepare_pending_test_hls_access_lease, response_body,
    support::spawn_test_binary_origin_with_request,
    test_app_state_with_inputs, AppState, ConfigInput, TestBinaryOriginResponse, TestSegmentOrigin,
};
use crate::api::endpoints::hls_api::manifest::HLS_ENTRY_WARNINGS;
use axum::http::{header, StatusCode};
use sha2::{Digest, Sha256};
use shared::model::{HlsManifestRecoveryBurstLevel, HlsStartupMode, InputType, Millis, Secs};
use std::{
    fmt::Write as _,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{watch, Notify};
use tuliprox_core::utils::current_time_millis;
use tuliprox_hls::{
    api::{
        safe_hls_access_lease_id, HlsAccessLease, HlsAccessLeaseId, HlsManifestAcceptanceDirective, HlsSessionHandle,
        ProxySessionId, SegmentCacheStatus,
    },
    HlsManifestAcceptanceTrigger,
};

const TARGET: u64 = 6;
const FIRST: u64 = 100;
const RANGE_OFFSET: usize = 188;

mod forward_jump;
mod retained_poll;

#[derive(Clone, Copy, PartialEq, Eq)]
enum MediaShape {
    Plain,
    Map,
    ByteRange,
    Timestamped,
}

fn media(msn: u64) -> Vec<u8> {
    let mut bytes =
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../test/fixtures/hls/channel_unavailable.ts")).to_vec();
    bytes.extend_from_slice(&[0x47, 0x1f, 0xff, 0x10]);
    bytes.extend_from_slice(&[u8::try_from(msn).unwrap(); 184]);
    bytes
}

fn manifest(head: u64, count: u64, shape: MediaShape, discontinuity_at: Option<u64>) -> String {
    let target = if shape == MediaShape::Timestamped { forward_jump::TARGET } else { TARGET };
    manifest_with_target(head, count, shape, discontinuity_at, target)
}

fn manifest_with_target(
    head: u64,
    count: u64,
    shape: MediaShape,
    discontinuity_at: Option<u64>,
    target: u64,
) -> String {
    let mut body = format!("#EXTM3U\n#EXT-X-TARGETDURATION:{target}\n#EXT-X-MEDIA-SEQUENCE:{head}\n");
    if shape == MediaShape::ByteRange {
        body.push_str("#EXT-X-VERSION:4\n");
    }
    if shape == MediaShape::Map {
        body.push_str("#EXT-X-MAP:URI=\"/init.mp4\"\n");
    }
    for seq in head..head + count {
        if discontinuity_at == Some(seq) {
            body.push_str("#EXT-X-DISCONTINUITY\n");
        }
        writeln!(body, "#EXTINF:{target},").unwrap();
        if shape == MediaShape::ByteRange {
            writeln!(body, "#EXT-X-BYTERANGE:{}@{RANGE_OFFSET}", media(seq).len()).unwrap();
        }
        writeln!(body, "/{seq}.ts").unwrap();
    }
    body
}

fn media_urls(body: &str) -> Vec<&str> { body.lines().filter(|line| line.starts_with("/hls/")).collect() }

struct Script {
    head: u64,
    segment_count: u64,
    manifest_status: StatusCode,
    recovery_gate: Option<watch::Receiver<bool>>,
    successor_gate: Option<watch::Receiver<bool>>,
    missing_media: Option<u64>,
    missing_media_gate: Option<watch::Receiver<bool>>,
    discontinuity_at: Option<u64>,
    target_duration: Option<u64>,
}

async fn spawn_recovery_origin(
    shape: MediaShape,
    script: &Arc<Mutex<Script>>,
    recovery_at_origin: &Arc<Notify>,
) -> TestSegmentOrigin {
    let fixture_script = Arc::clone(script);
    let at_origin = Arc::clone(recovery_at_origin);
    let timestamped_media = (shape == MediaShape::Timestamped).then(forward_jump::transport_fixture);
    spawn_test_binary_origin_with_request(Arc::new(move |path, request| {
        let script = fixture_script.lock().unwrap();
        if path_has_extension(path, "m3u8") {
            let body = script.target_duration.map_or_else(
                || manifest(script.head, script.segment_count, shape, script.discontinuity_at),
                |target| {
                    manifest_with_target(script.head, script.segment_count, shape, script.discontinuity_at, target)
                },
            );
            let mut response = TestBinaryOriginResponse::new(script.manifest_status, Arc::from(body.into_bytes()));
            response.gate.clone_from(&script.recovery_gate);
            if response.gate.is_some() {
                at_origin.notify_one();
            }
            response
        } else if path == "/init.mp4" {
            TestBinaryOriginResponse::new(StatusCode::OK, Arc::from(&b"fixture-init-map"[..]))
        } else {
            let seq = path.trim_matches('/').strip_suffix(".ts").unwrap().parse::<u64>().unwrap();
            if script.missing_media == Some(seq) {
                let mut response = TestBinaryOriginResponse::new(StatusCode::NOT_FOUND, Arc::from([]));
                response.gate.clone_from(&script.missing_media_gate);
                return response;
            }
            let bytes =
                timestamped_media.as_ref().map_or_else(|| media(seq), |source| forward_jump::media(source, seq));
            let mut response = if shape == MediaShape::ByteRange {
                let end = RANGE_OFFSET + bytes.len() - 1;
                let expected = format!("bytes={RANGE_OFFSET}-{end}");
                let requested = request
                    .lines()
                    .filter_map(|line| line.split_once(':'))
                    .find(|(name, _)| name.eq_ignore_ascii_case("range"));
                assert_eq!(requested.map(|(_, value)| value.trim()), Some(expected.as_str()));
                let mut response = TestBinaryOriginResponse::new(StatusCode::PARTIAL_CONTENT, Arc::from(bytes));
                response.content_range = Some(format!("bytes {RANGE_OFFSET}-{end}/{}", end + 1));
                response
            } else {
                TestBinaryOriginResponse::new(StatusCode::OK, Arc::from(bytes))
            };
            if seq >= FIRST + 6 {
                response.gate.clone_from(&script.successor_gate);
            }
            response
        }
    }))
    .await
}

struct Playback {
    app: Arc<AppState>,
    input: ConfigInput,
    url: String,
    session: HlsSessionHandle,
    key: tuliprox_hls::api::HlsSessionKey,
    proxy: ProxySessionId,
    first: String,
    script: Arc<Mutex<Script>>,
    recovery_at_origin: Arc<Notify>,
    origin: TestSegmentOrigin,
    warnings: Arc<Mutex<Vec<String>>>,
}

struct HeldRecovery {
    release: watch::Sender<bool>,
    refresh: tokio::task::JoinHandle<()>,
    requests: usize,
    generation: u64,
    full_bursts: Option<u16>,
}

struct Reserve {
    playback_position_known: bool,
    guaranteed_reserve_ms: u64,
    initial_hidden_ready_duration_ms: u64,
}

impl HeldRecovery {
    async fn finish(self) {
        self.release.send(true).unwrap();
        tokio::time::timeout(Duration::from_secs(6), self.refresh).await.unwrap().unwrap();
    }
}

impl Playback {
    async fn start() -> Self { Self::with_media(MediaShape::Plain).await }

    async fn with_media(shape: MediaShape) -> Self { Self::with_target_duration(shape, None).await }

    async fn with_target_duration(shape: MediaShape, target_duration: Option<u64>) -> Self {
        Self::with_session_key(shape, target_duration, None).await
    }

    async fn with_session_key(
        shape: MediaShape,
        target_duration: Option<u64>,
        session_key: Option<tuliprox_hls::api::HlsSessionKey>,
    ) -> Self {
        let script = Arc::new(Mutex::new(Script {
            head: FIRST,
            segment_count: 6,
            manifest_status: StatusCode::OK,
            recovery_gate: None,
            successor_gate: None,
            missing_media: None,
            missing_media_gate: None,
            discontinuity_at: None,
            target_duration,
        }));
        let recovery_at_origin = Arc::new(Notify::new());
        let origin = spawn_recovery_origin(shape, &script, &recovery_at_origin).await;
        let input = ConfigInput {
            id: 1,
            name: "recovery-entry-origin".into(),
            input_type: InputType::M3u,
            url: origin.base_url.clone(),
            enabled: true,
            max_connections: 1,
            ..Default::default()
        };
        let app = test_app_state_with_inputs(vec![Arc::new(input.clone())]);
        let mode = if matches!(shape, MediaShape::Plain | MediaShape::Timestamped) {
            HlsStartupMode::FirstReady
        } else {
            HlsStartupMode::Conservative
        };
        enable_hls_cache_with_startup_mode(&app, mode).await;
        let mut config = app.app_config.config.load_full().as_ref().clone();
        let hls = config.reverse_proxy.as_mut().unwrap().hls_cache.as_mut().unwrap();
        hls.strip.value = 3;
        // Keep the held request count stable while the publication is tested.
        hls.manifest_recovery_burst.level = HlsManifestRecoveryBurstLevel::Off;
        hls.origin_manifest_timeout_ms = Millis::new(4_000);
        hls.initial_manifest_wait_timeout_secs = Secs::new(if shape == MediaShape::Timestamped { 10 } else { 2 });
        app.app_config.config.store(Arc::new(config));
        app.hls.proxy.update_config(&app.app_config).await;
        create_active_hls_user_session(&app).await;
        let url = format!("{}/index.m3u8", origin.base_url);
        let source = super::super::super::build_hls_origin_source(&input, "12345");
        let key = if let Some(key) = session_key {
            app.hls
                .proxy
                .get_or_create_session_with_source_and_outcome(
                    key.clone(),
                    source,
                    &app.get_encrypt_secret(),
                    app.hls.proxy.now_ms(),
                )
                .await;
            key
        } else {
            source.session_key()
        };
        let response = canonical_response_for_lease_with_key(&app, &input, &url, "warm", &key).await;
        assert_eq!(response.status(), StatusCode::OK);
        let first = text(response).await;
        let session = app.hls.proxy.sessions().get_by_key(&key).await.unwrap();
        let proxy = session.read().await.proxy_session_id.clone();
        let playback = Self {
            app,
            input,
            url,
            session,
            key,
            proxy,
            first,
            script,
            recovery_at_origin,
            origin,
            warnings: Arc::new(Mutex::new(Vec::new())),
        };
        let first_uri = media_urls(&playback.first)[0];
        if shape == MediaShape::Timestamped {
            forward_jump::assert_media(&playback, first_uri, FIRST).await;
        } else {
            playback.assert_media(first_uri).await;
        }
        playback.ready(FIRST, FIRST + 6).await;
        let mut state = playback.session.write().await;
        state.render_and_store_manifest(current_time_millis()).unwrap();
        state.origin_refresh.last_success_at_ms = Some(current_time_millis());
        drop(state);
        playback
    }

    async fn ready(&self, first: u64, end: u64) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let state = self.session.read().await;
                if (first - FIRST..end - FIRST).all(|seq| {
                    state
                        .segments
                        .get(&seq)
                        .is_some_and(|entry| matches!(entry.status, SegmentCacheStatus::Ready { .. }))
                }) {
                    break;
                }
                drop(state);
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fixture media become ready");
    }

    async fn age_only(&self) {
        let now = current_time_millis();
        self.app.hls.proxy.freeze_clock_for_test(now);
        self.session.write().await.origin_refresh.last_success_at_ms = Some(now - 3_100);
    }

    async fn retry_gap(&self) { self.retry_gap_with_status(StatusCode::BAD_GATEWAY).await; }

    async fn retry_gap_with_status(&self, status: StatusCode) {
        self.age_only().await;
        // The previous failure puts this refresh on the shared retry backoff.
        self.session.write().await.origin_refresh.consecutive_failures = 1;
        self.script.lock().unwrap().manifest_status = status;
        let before = self.origin.manifest_request_count();
        self.refresh(HlsManifestAcceptanceTrigger::Observe).await;
        // Hold terminal evaluation separately from the retryable origin result.
        self.app.hls.proxy.availability_reevaluations().cancel_session(&self.proxy);
        let now = self.app.hls.proxy.now_ms();
        let state = self.session.read().await;
        assert!(self.origin.manifest_request_count() > before);
        assert!(!state.origin_refresh.in_flight);
        assert_eq!(state.origin_refresh.last_error_at_ms, Some(now));
        assert!(state.origin_refresh.next_fetch_allowed_at_ms > now);
        let episode = state.origin_control.acceptance_episode.as_ref().unwrap();
        assert!(episode.next_retry_at_ms.is_some());
        assert_eq!(episode.generation, state.origin_control.acceptance_generation);
        eprintln!(
            "recovery_gap generation={} origin_status={} next_attempt_in_ms={} origin_manifest_requests={}",
            episode.generation.0,
            status.as_u16(),
            state.origin_refresh.next_fetch_allowed_at_ms - now,
            self.origin.manifest_request_count(),
        );
    }

    async fn hold_only_visible_ready(&self) {
        let mut state = self.session.write().await;
        for seq in 3..6 {
            state.segments.get_mut(&seq).unwrap().status = SegmentCacheStatus::Discovered;
        }
    }

    async fn reserve(&self, id: &str) -> Reserve {
        use tuliprox_hls::api::{evaluate_lease_reserve, HlsLeaseReserveInput, HlsRecoveryTriggerBudgetMs};
        let lease = self.lease(id).await;
        let snapshot = lease.last_manifest_snapshot.as_ref().unwrap();
        let now_ms = self.app.hls.proxy.now_ms();
        let timeline = self.session.read().await.ready_timeline_snapshot(
            lease.playback_cursor.ready_timeline_start_proxy_seq(snapshot.first_proxy_seq),
            now_ms,
        );
        let reserve = evaluate_lease_reserve(HlsLeaseReserveInput {
            manifest: snapshot,
            cursor: &lease.playback_cursor,
            ready_timeline: &timeline,
            now_ms,
            recovery_trigger_budget: HlsRecoveryTriggerBudgetMs::from_millis(0),
            origin_path_degraded: false,
            recovery_committed: false,
        });
        Reserve {
            playback_position_known: reserve.playback_position_known,
            guaranteed_reserve_ms: reserve.guaranteed_reserve_ms,
            initial_hidden_ready_duration_ms: reserve.initial_hidden_ready_duration_ms,
        }
    }

    async fn exhaust_lease(&self, id: &str) {
        let mut lease = self.lease(id).await;
        lease.playback_cursor.first_segment_started_at_ms = Some(self.app.hls.proxy.now_ms().saturating_sub(60_000));
        self.app.hls.proxy.prepare_access_lease(lease).await;
        let reserve = self.reserve(id).await;
        assert!(reserve.playback_position_known);
        assert_eq!(reserve.guaranteed_reserve_ms, 0);
    }

    async fn range(&self, uri: &str) -> (String, bytes::Bytes) {
        let response = get_response(Arc::clone(&self.app), uri, Some("bytes=0-187")).await;
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        let content_range = response.headers()[header::CONTENT_RANGE].to_str().unwrap().to_owned();
        (content_range, response_body(response).await)
    }

    async fn prepare_lease(&self, id: &str) {
        prepare_pending_test_hls_access_lease(&self.app, &self.proxy, &HlsAccessLeaseId(id.to_owned())).await;
    }

    async fn request(&self, id: &str) -> axum::response::Response {
        HLS_ENTRY_WARNINGS
            .scope(
                Arc::clone(&self.warnings),
                canonical_response_for_lease_with_key(&self.app, &self.input, &self.url, id, &self.key),
            )
            .await
    }

    fn warnings(&self, lease: &str) -> Vec<String> {
        let lease = safe_hls_access_lease_id(&HlsAccessLeaseId(lease.to_owned()));
        self.warnings.lock().unwrap().iter().filter(|line| line.contains(&format!("lease={lease} "))).cloned().collect()
    }

    async fn lease(&self, id: &str) -> HlsAccessLease {
        self.app
            .hls
            .proxy
            .access_lease_response_snapshot(&HlsAccessLeaseId(id.to_owned()), &self.proxy, self.app.hls.proxy.now_ms())
            .await
            .unwrap()
    }

    async fn assert_media(&self, uri: &str) {
        let response = get_response(Arc::clone(&self.app), uri, None).await;
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        let digest = Sha256::digest(response_body(response).await);
        let seq = uri.rsplit('/').next().unwrap().strip_suffix(".ts").unwrap().parse::<u64>().unwrap();
        assert_eq!(digest, Sha256::digest(media(FIRST + seq)));
        eprintln!("media uri={uri} sha256={digest:02x?}");
    }

    async fn refresh(&self, trigger: HlsManifestAcceptanceTrigger) {
        self.session.write().await.origin_refresh.next_fetch_allowed_at_ms = 0;
        let mut directive = HlsManifestAcceptanceDirective::none();
        directive.trigger = trigger;
        refresh_playing_lease_with_directive(&self.app, &self.session, &self.url, directive).await;
    }

    async fn wait_for_refresh_to_finish(&self) {
        tokio::time::timeout(Duration::from_secs(6), async {
            while self.session.read().await.origin_refresh.in_flight {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }

    async fn unready_successor(&self) -> watch::Sender<bool> {
        let (release, gate) = watch::channel(false);
        {
            let mut script = self.script.lock().unwrap();
            script.head = FIRST + 4;
            script.successor_gate = Some(gate);
        }
        self.refresh(HlsManifestAcceptanceTrigger::None).await;
        release
    }

    async fn hold_refresh(&self, head: u64, trigger: HlsManifestAcceptanceTrigger) -> HeldRecovery {
        self.hold_refresh_for(head, trigger, "warm").await
    }

    async fn hold_refresh_for(
        &self,
        head: u64,
        trigger: HlsManifestAcceptanceTrigger,
        lease: &'static str,
    ) -> HeldRecovery {
        let (release, gate) = watch::channel(false);
        {
            let mut script = self.script.lock().unwrap();
            script.head = head;
            script.recovery_gate = Some(gate);
        }
        self.session.write().await.origin_refresh.next_fetch_allowed_at_ms = 0;
        let mut directive = HlsManifestAcceptanceDirective::none();
        directive.trigger = trigger;
        let app = Arc::clone(&self.app);
        let session = Arc::clone(&self.session);
        let url = self.url.clone();
        let refresh = tokio::spawn(async move {
            refresh_playing_lease_with_directive_for(&app, &session, &url, lease, directive).await;
        });
        tokio::time::timeout(Duration::from_secs(3), self.recovery_at_origin.notified()).await.unwrap();
        let state = self.session.read().await;
        let generation = state.origin_control.acceptance_generation.0;
        let full_bursts = state.origin_control.acceptance_episode.as_ref().map(|episode| episode.full_bursts_completed);
        let requests = self.origin.manifest_request_count();
        HeldRecovery { release, refresh, requests, generation, full_bursts }
    }

    async fn assert_held(&self, held: &HeldRecovery) {
        let state = self.session.read().await;
        assert!(state.origin_refresh.in_flight && !held.refresh.is_finished());
        assert_eq!(state.origin_control.acceptance_generation.0, held.generation);
        assert_eq!(self.origin.manifest_request_count(), held.requests);
        assert_eq!(
            state.origin_control.acceptance_episode.as_ref().map(|episode| episode.full_bursts_completed),
            held.full_bursts
        );
        eprintln!(
            "recovery_generation={} origin_manifest_requests={} full_bursts_completed={:?} recovery_held=true",
            held.generation, held.requests, held.full_bursts
        );
    }
}

async fn text(response: axum::response::Response) -> String {
    String::from_utf8(response_body(response).await.to_vec()).unwrap()
}

fn assert_retry(response: &axum::response::Response) {
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers().get(header::RETRY_AFTER).unwrap(), "1");
}

async fn assert_fallback_warning(
    playback: &Playback,
    id: &str,
    snapshot: &tuliprox_hls::api::HlsLeaseManifestSnapshot,
    reason: &str,
) {
    use tuliprox_hls::api::{safe_proxy_session_id, safe_session_key};
    let captured = playback.warnings(id);
    assert_eq!(captured.len(), 1, "{captured:?}");
    let safe_session = safe_session_key(&playback.session.read().await.key);
    let safe_proxy = safe_proxy_session_id(&playback.proxy);
    let expected = [
        format!("session={safe_session} proxy_session={safe_proxy}"),
        "recovery=in_flight".into(),
        format!("reason={reason}"),
        format!(
            "commit={} first={} last={}",
            snapshot.source_commit_identity.commit_generation(),
            snapshot.first_proxy_seq,
            snapshot.last_proxy_seq
        ),
        "status=200".into(),
    ];
    assert!(expected.iter().all(|field| captured[0].contains(field)), "{captured:?}");
}

#[tokio::test]
async fn between_recovery_attempts_a_new_lease_uses_a_playable_window_without_hidden_reserve() {
    let playback = Playback::start().await;
    playback.retry_gap().await;
    playback.hold_only_visible_ready().await;
    let requests = playback.origin.manifest_request_count();
    let (source, origin_commits) = {
        let state = playback.session.read().await;
        (state.last_normal_manifest_commit_identity().unwrap(), state.origin_manifest_commits())
    };
    let a = playback.lease("warm").await.last_manifest_snapshot.unwrap();
    let reserve = playback.reserve("warm").await;
    assert!(reserve.playback_position_known && reserve.guaranteed_reserve_ms > 0);
    let id = "recovery-gap-b";
    playback.prepare_lease(id).await;
    let response = playback.request(id).await;
    eprintln!("gap_entry status={} a_reserve_ms={}", response.status().as_u16(), reserve.guaranteed_reserve_ms);
    assert_eq!(response.status(), StatusCode::OK);
    let body = text(response).await;
    let b = playback.lease(id).await.last_manifest_snapshot.unwrap();
    assert_eq!(b.source_commit_identity, source);
    assert_eq!(b.visible_proxy_seqs().collect::<Vec<_>>(), vec![0, 1, 2]);
    assert_eq!(playback.reserve(id).await.initial_hidden_ready_duration_ms, 0);
    let uris = media_urls(&body);
    assert_eq!(uris.len(), 3);
    assert!(uris.iter().all(|uri| uri.contains(id)));
    playback.assert_media(uris[0]).await;
    playback.assert_media(media_urls(&playback.first)[0]).await;
    assert_eq!(playback.lease("warm").await.last_manifest_snapshot.unwrap(), a);
    assert_eq!(playback.session.read().await.origin_manifest_commits(), origin_commits);
    assert_eq!(playback.origin.manifest_request_count(), requests);
    let warnings = playback.warnings(id);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("recovery=between_attempts reason=fresh_commit_pending"));
    assert!(warnings[0].contains(&format!("commit={} first=0 last=2 status=200", source.commit_generation())));
    eprintln!(
        "gap_source_commit={} b_hidden_ready_ms=0 origin_manifest_requests={requests}",
        source.commit_generation()
    );
}

#[tokio::test]
async fn recovery_entry_uses_shared_playback_reserve_after_a_media_delivery_gap() {
    let playback = Playback::start().await;
    playback.retry_gap().await;
    let now_ms = playback.app.hls.proxy.now_ms();
    let last_delivery_ms = now_ms - (TARGET * 3_000 + 1);
    let source = {
        let mut session = playback.session.write().await;
        session.activity.last_delivered_media_at_ms = Some(last_delivery_ms);
        assert!(session.last_rendered_manifest.as_ref().unwrap().valid_until_ms >= now_ms);
        session.last_normal_manifest_commit_identity().unwrap()
    };
    let mut warm = playback.lease("warm").await;
    warm.playback_cursor.first_segment_started_at_ms = Some(last_delivery_ms);
    playback.app.hls.proxy.prepare_access_lease(warm).await;
    let reserve = playback.reserve("warm").await;
    assert!(reserve.playback_position_known && reserve.guaranteed_reserve_ms > 0);

    let requests = playback.origin.manifest_request_count();
    let id = "recovery-after-delivery-gap";
    playback.prepare_lease(id).await;
    let response = playback.request(id).await;
    assert_eq!(response.status(), StatusCode::OK);
    let playlist = text(response).await;
    let published = playback.lease(id).await.last_manifest_snapshot.unwrap();
    assert_eq!(published.source_commit_identity, source);
    assert!(playback.warnings(id)[0].contains("recovery=between_attempts"));
    assert_eq!(playback.origin.manifest_request_count(), requests);

    playback.exhaust_lease("warm").await;
    playback.prepare_lease("recovery-after-reserve-exhausted").await;
    let response = playback.request("recovery-after-reserve-exhausted").await;
    assert_retry(&response);
    assert!(playback.lease("recovery-after-reserve-exhausted").await.last_manifest_snapshot.is_none());
    playback.assert_media(media_urls(&playlist)[0]).await;
}

#[tokio::test]
async fn unplayable_recovery_entry_still_triggers_the_due_origin_retry() {
    let playback = Playback::start().await;
    playback.retry_gap().await;
    let next_attempt = {
        let mut state = playback.session.write().await;
        state.startup.as_mut().unwrap().revisions.remove(&1).unwrap();
        state.origin_refresh.next_fetch_allowed_at_ms
    };
    let reserve = playback.reserve("warm").await;
    assert!(reserve.playback_position_known && reserve.guaranteed_reserve_ms > 0);
    assert!(playback.app.hls.proxy.availability_reevaluations().observe_owner(&playback.proxy).is_none());
    let (release, gate) = watch::channel(false);
    {
        let mut script = playback.script.lock().unwrap();
        script.manifest_status = StatusCode::OK;
        script.recovery_gate = Some(gate);
    }
    let id = "recovery-gap-missing-binding";
    playback.prepare_lease(id).await;
    let requests = playback.origin.manifest_request_count();
    assert_retry(&playback.request(id).await);
    assert_eq!(playback.origin.manifest_request_count(), requests, "the retry is still in backoff");
    assert!(playback.lease(id).await.last_manifest_snapshot.is_none());

    playback.app.hls.proxy.freeze_clock_for_test(next_attempt);
    let response = playback.request(id).await;
    let observed = tokio::time::timeout(Duration::from_secs(2), playback.recovery_at_origin.notified()).await;
    let requests_after = playback.origin.manifest_request_count();
    eprintln!(
        "due_retry entry_status={} origin_gets_before={requests} origin_gets_after={requests_after} established_reserve_ms={}",
        response.status().as_u16(),
        reserve.guaranteed_reserve_ms
    );
    assert_retry(&response);
    assert!(playback.lease(id).await.last_manifest_snapshot.is_none());
    assert!(playback.warnings(id).iter().any(|line| line.contains("reason=missing_window_bindings")));
    observed.expect("the waiting lease starts the due retry even though its fallback is unplayable");
    assert_eq!(requests_after, requests + 1);
    release.send(true).unwrap();
    playback.wait_for_refresh_to_finish().await;
}

#[tokio::test]
async fn new_lease_uses_the_last_shared_publication_while_newer_media_are_held() {
    let playback = Playback::start().await;
    let source = playback.session.read().await.last_normal_manifest_commit_identity().unwrap();
    let a = playback.lease("warm").await.last_manifest_snapshot.unwrap();
    let successors = playback.unready_successor().await;
    let held = playback.hold_refresh(FIRST + 5, HlsManifestAcceptanceTrigger::Observe).await;
    let commits = playback.session.read().await.origin_manifest_commits();
    let id = "recovery-window-b";
    playback.prepare_lease(id).await;
    let response = playback.request(id).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = text(response).await;
    let b = playback.lease(id).await.last_manifest_snapshot.unwrap();
    assert_eq!(b.source_commit_identity, source);
    assert_eq!(b.visible_proxy_seqs().collect::<Vec<_>>(), vec![0, 1, 2]);
    assert!(media_urls(&body).iter().all(|uri| uri.contains(id)));
    assert_eq!(playback.lease("warm").await.last_manifest_snapshot.unwrap(), a);
    assert_eq!(playback.session.read().await.origin_manifest_commits(), commits);
    playback.assert_held(&held).await;
    playback.assert_media(media_urls(&body)[0]).await;
    playback.assert_media(media_urls(&playback.first)[0]).await;
    let repeated = playback.request(id).await;
    assert_eq!(repeated.status(), StatusCode::OK);
    assert_eq!(text(repeated).await, body);
    playback.assert_held(&held).await;
    assert_fallback_warning(&playback, id, &b, "regular_window_unavailable").await;
    eprintln!(
        "fallback_source_commit={} latest_origin_commits={commits} playlist={body:?}",
        source.commit_generation()
    );
    let _ = successors.send(true);
    held.finish().await;
}

async fn assert_age_only_fallback(shape: MediaShape, id: &str) {
    use tuliprox_hls::api::{
        HlsCachedManifestOptions, HlsFreshManifestRequiredReason, HlsManifestCommitRequirement, HlsSessionStoreOutcome,
    };
    let playback = Playback::with_media(shape).await;
    let held = playback.hold_refresh(FIRST + 1, HlsManifestAcceptanceTrigger::Observe).await;
    playback.prepare_lease(id).await;
    playback.age_only().await;
    let now = playback.app.hls.proxy.now_ms();
    let lease = playback.lease(id).await;
    let (requirement, recovery_entry) = playback
        .app
        .hls
        .proxy
        .manifest_commit_requirement_with_recovery_entry_for_lease(
            &playback.session,
            HlsSessionStoreOutcome::Reused,
            None,
            true,
            &lease.lease_id,
            now,
        )
        .await;
    assert_eq!(
        requirement,
        HlsManifestCommitRequirement::FreshCommitRequired { reason: HlsFreshManifestRequiredReason::StaleEntryWindow }
    );
    let options = HlsCachedManifestOptions::initial(Duration::ZERO).with_recovery_entry(recovery_entry);
    assert!(options.permits_recovery_entry());
    assert!(playback.session.read().await.recovery_entry_is_current(&lease, options, now));
    let commits = playback.session.read().await.origin_manifest_commits();
    let response = playback.request(id).await;
    assert_eq!(response.status(), StatusCode::OK, "age-only recovery entry: {:?}", playback.warnings(id));
    let before = text(response).await;
    playback.assert_held(&held).await;
    assert_eq!(playback.session.read().await.origin_manifest_commits(), commits);
    let published = playback.lease(id).await.last_manifest_snapshot.unwrap();
    assert_eq!(manifest_media_sequence(&before), published.first_proxy_seq);
    assert_fallback_warning(&playback, id, &published, "fresh_commit_pending").await;
    let bindings = {
        let state = playback.session.read().await;
        published.visible_proxy_seqs().map(|seq| state.segments[&seq].origin_byte_range).collect::<Vec<_>>()
    };
    if shape == MediaShape::ByteRange {
        assert!(bindings.iter().all(Option::is_some));
        assert!(!before.contains("#EXT-X-BYTERANGE"), "the proxy publishes each origin slice as its own object");
    }
    let mut old_ranges = Vec::new();
    for uri in media_urls(&before) {
        playback.assert_media(uri).await;
        old_ranges.push(playback.range(uri).await);
    }
    held.finish().await;
    playback.ready(FIRST + 1, FIRST + 7).await;
    let response = playback.request(id).await;
    assert_eq!(response.status(), StatusCode::OK);
    let after = text(response).await;
    let before_start = manifest_media_sequence(&before);
    let after_start = manifest_media_sequence(&after);
    assert!(after_start >= before_start);
    assert!(after_start < before_start + 3, "the windows must overlap");
    let overlap = usize::try_from(after_start - before_start).unwrap();
    for (index, (old, new)) in media_urls(&before).iter().skip(overlap).zip(media_urls(&after)).enumerate() {
        assert_eq!(*old, new);
        playback.assert_media(new).await;
        let range = playback.range(new).await;
        assert_eq!(old_ranges[overlap + index], range);
        let seq = after_start + u64::try_from(index).unwrap();
        let origin_range = playback.session.read().await.segments[&seq].origin_byte_range;
        assert_eq!(origin_range, bindings[overlap + index]);
        eprintln!(
            "overlap_uri={new} origin_range={origin_range:?} content_range={} range_sha256={:02x?}",
            range.0,
            Sha256::digest(&range.1)
        );
    }
    playback.assert_media(media_urls(&playback.first)[0]).await;
    assert_eq!(playback.warnings(id).len(), 1);
    eprintln!(
        "age_ms=3100 fallback_commit={} before={before:?} after={after:?}",
        published.source_commit_identity.commit_generation()
    );
}

#[tokio::test]
async fn age_only_fallback_publishes_lease_urls_and_continues_monotonically() {
    assert_age_only_fallback(MediaShape::Plain, "recovery-age-b").await;
}

async fn warm_cache_continuation_window(playback: &Playback) -> tuliprox_hls::api::HlsManifestCommitIdentity {
    let mut config = playback.app.app_config.config.load_full().as_ref().clone();
    let cache = config.reverse_proxy.as_mut().unwrap().hls_cache.as_mut().unwrap();
    cache.strip.value = 2;
    // Keep the test lease valid on both clocks while the origin's real retry delays run.
    cache.initial_manifest_wait_timeout_secs = Secs::new(10);
    playback.app.app_config.config.store(Arc::new(config));
    playback.app.hls.proxy.update_config(&playback.app.app_config).await;
    playback.script.lock().unwrap().head = FIRST + 2;
    playback.refresh(HlsManifestAcceptanceTrigger::None).await;
    playback.ready(FIRST + 2, FIRST + 8).await;
    let response = playback.request("warm").await;
    assert_eq!(response.status(), StatusCode::OK);
    let playlist_a = text(response).await;
    let urls_a = media_urls(&playlist_a);
    assert!(urls_a[urls_a.len() - 2].ends_with("000006.ts"));
    assert!(urls_a[urls_a.len() - 1].ends_with("000007.ts"));
    for uri in &urls_a[urls_a.len() - 2..] {
        playback.assert_media(uri).await;
    }
    playback.lease("warm").await.last_manifest_snapshot.unwrap().source_commit_identity
}

#[tokio::test]
async fn established_fallback_lease_continues_from_cache_during_origin_407() {
    use super::super::origin_restart::trace_fixture;
    trace_fixture();
    let playback = Playback::start().await;
    let origin = warm_cache_continuation_window(&playback).await;
    let (render, origin_commits) = {
        let mut session = playback.session.write().await;
        let rendered = session.render_and_store_manifest(current_time_millis()).unwrap();
        assert_eq!(rendered.segment_proxy_seqs, (2..=7).collect::<Vec<_>>());
        (session.last_normal_manifest_commit_identity().unwrap(), session.origin_manifest_commits())
    };
    assert!(render > origin);

    playback.retry_gap_with_status(StatusCode::PROXY_AUTHENTICATION_REQUIRED).await;
    let (next_attempt, established) = {
        let state = playback.session.read().await;
        (state.origin_refresh.next_fetch_allowed_at_ms, state.recovery_entry_established_leases_for_test().to_vec())
    };
    assert_eq!(established, [(HlsAccessLeaseId("warm".into()), playback.lease("warm").await.issued_at_ms)]);
    let reserve_a = playback.reserve("warm").await;
    assert!(reserve_a.playback_position_known && reserve_a.guaranteed_reserve_ms > 0);
    let viewer = "cache-continuation-b";
    playback.prepare_lease(viewer).await;
    let response = playback.request(viewer).await;
    assert_eq!(response.status(), StatusCode::OK, "{:?}", playback.warnings(viewer));
    let before = text(response).await;
    let fallback = playback.lease(viewer).await.last_manifest_snapshot.unwrap();
    assert!(fallback.is_recovery_entry());
    assert_eq!(fallback.source_commit_identity, render);
    assert_eq!(fallback.visible_proxy_seqs().collect::<Vec<_>>(), [3, 4, 5]);
    for uri in media_urls(&before) {
        assert!(uri.contains(viewer));
        playback.assert_media(uri).await;
    }
    assert!(playback.warnings(viewer)[0].contains("recovery=between_attempts"));
    playback.exhaust_lease("warm").await;
    let origin_requests = playback.origin.manifest_request_count();
    let origin_progress = playback.session.read().await.origin_control.progress_generation;
    let (release, gate) = watch::channel(false);
    playback.script.lock().unwrap().recovery_gate = Some(gate);
    playback.app.hls.proxy.freeze_clock_for_test(next_attempt);
    let response = playback.request(viewer).await;
    let poll_status = response.status();
    tokio::time::timeout(Duration::from_secs(3), playback.recovery_at_origin.notified()).await.unwrap();
    assert!(playback.origin.manifest_request_count() > origin_requests);
    let current = playback.lease(viewer).await.last_manifest_snapshot.unwrap();
    eprintln!(
        "cache_continuation status={} fallback_render={} origin_commit={} snapshot_before={} snapshot_after={} origin_gets_before={origin_requests} origin_gets_after={}",
        poll_status.as_u16(), render.commit_generation(), origin.commit_generation(), fallback.snapshot_generation,
        current.snapshot_generation, playback.origin.manifest_request_count()
    );
    assert_eq!(poll_status, StatusCode::OK);
    let after = text(response).await;
    assert_eq!(manifest_media_sequence(&after), 3);
    assert_eq!(current.visible_proxy_seqs().collect::<Vec<_>>(), [3, 4, 5, 6, 7]);
    assert_eq!(current.source_commit_identity, origin);
    assert!(!current.is_recovery_entry());
    assert!(current.snapshot_generation > fallback.snapshot_generation);
    let urls = media_urls(&after);
    assert_eq!(urls.len(), 5);
    assert_eq!(urls[..3], media_urls(&before));
    let segment_gets = playback.origin.segment_request_count();
    for uri in urls {
        assert!(uri.contains(viewer));
        playback.assert_media(uri).await;
    }
    assert_eq!(playback.origin.segment_request_count(), segment_gets);
    let reserve_b = playback.reserve(viewer).await;
    assert!(reserve_b.playback_position_known && reserve_b.guaranteed_reserve_ms > 0);
    assert!(playback.session.read().await.origin_refresh.in_flight);
    let denied = "cache-continuation-c";
    playback.prepare_lease(denied).await;
    let denied_response = playback.request(denied).await;
    assert_retry(&denied_response);
    assert!(playback.lease(denied).await.last_manifest_snapshot.is_none());
    assert!(playback.warnings(denied)[0].contains("reason=established_live_reserve_exhausted"));
    release.send(true).unwrap();
    playback.wait_for_refresh_to_finish().await;
    playback.app.hls.proxy.availability_reevaluations().cancel_session(&playback.proxy);
    let state = playback.session.read().await;
    assert_eq!(state.origin_manifest_commits(), origin_commits);
    assert_eq!(state.origin_control.progress_generation, origin_progress);
    assert_eq!(state.origin_refresh.last_error_at_ms, Some(next_attempt));
    assert_eq!(state.recovery_entry_established_leases_for_test(), established);
    eprintln!(
        "closed_fallback reserve_a_before_ms={} reserve_a_after_ms=0 reserve_b_ms={} c_status={} origin_commits={origin_commits} origin_progress={origin_progress:?} established=warm origin_segment_gets={segment_gets}",
        reserve_a.guaranteed_reserve_ms, reserve_b.guaranteed_reserve_ms, denied_response.status().as_u16(),
    );
}

#[tokio::test]
async fn age_only_fallback_preserves_origin_byte_range_bindings_across_recovery() {
    assert_age_only_fallback(MediaShape::ByteRange, "recovery-byte-range-b").await;
}

#[tokio::test]
async fn a_ready_regular_candidate_wins_and_normal_refresh_cannot_enable_fallback() {
    let playback = Playback::start().await;
    let held = playback.hold_refresh(FIRST, HlsManifestAcceptanceTrigger::Observe).await;
    let id = "recovery-regular-b";
    playback.prepare_lease(id).await;
    assert_eq!(playback.request(id).await.status(), StatusCode::OK);
    assert_eq!(playback.warnings(id), Vec::<String>::new());
    playback.assert_held(&held).await;
    held.finish().await;

    let now = current_time_millis();
    let held = playback.hold_refresh(FIRST, HlsManifestAcceptanceTrigger::None).await;
    playback.app.hls.proxy.freeze_clock_for_test(now);
    {
        let mut state = playback.session.write().await;
        state.origin_refresh.last_success_at_ms = Some(now - 3_100);
        state.origin_control.path_condition = tuliprox_hls::api::HlsOriginPathCondition::PublicationLate;
    }
    let id = "recovery-normal-refresh-b";
    playback.prepare_lease(id).await;
    assert_ne!(playback.request(id).await.status(), StatusCode::OK);
    assert!(playback.lease(id).await.last_manifest_snapshot.is_none());
    assert_eq!(playback.warnings(id), Vec::<String>::new());
    playback.assert_held(&held).await;
    held.finish().await;
}

#[tokio::test]
async fn missing_expired_or_unbound_recovery_windows_stay_unpublished() {
    for case in ["missing", "expired", "revision", "map", "ready"] {
        let shape = if case == "map" { MediaShape::Map } else { MediaShape::Plain };
        let playback = Playback::with_media(shape).await;
        let successors = playback.unready_successor().await;
        let held = playback.hold_refresh(FIRST + 5, HlsManifestAcceptanceTrigger::Observe).await;
        let id = format!("recovery-unavailable-{case}-b");
        playback.prepare_lease(&id).await;
        playback.age_only().await;
        {
            let mut state = playback.session.write().await;
            match case {
                "missing" => state.last_rendered_manifest = None,
                "expired" => {
                    state.last_rendered_manifest.as_mut().unwrap().valid_until_ms = playback.app.hls.proxy.now_ms() - 1;
                }
                "revision" => {
                    state.startup.as_mut().unwrap().revisions.remove(&0).unwrap();
                }
                "map" => {
                    let id = state.segments[&0].map_ref.unwrap();
                    let cache_path = playback.app.hls.proxy.segment_cache().object_path(&state.maps[&id].cache_key);
                    std::fs::remove_file(cache_path).unwrap();
                }
                "ready" => state.segments.get_mut(&0).unwrap().status = SegmentCacheStatus::Discovered,
                _ => unreachable!(),
            }
        }
        let expected = match case {
            "missing" | "expired" => "reason=no_renderable_window",
            _ => "reason=missing_window_bindings",
        };
        for _ in 0..2 {
            let response = playback.request(&id).await;
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE, "{case}: {:?}", playback.warnings(&id));
            assert_retry(&response);
            assert!(playback.lease(&id).await.last_manifest_snapshot.is_none());
            playback.assert_held(&held).await;
        }
        let captured = playback.warnings(&id);
        assert_eq!(captured.len(), 2, "{case}: {captured:?}");
        assert!(captured.iter().all(|line| line.contains(expected) && line.contains("status=503 retry_after=1")));
        eprintln!("negative_window={case} status=503 retry_after=1 snapshot=none");
        let _ = successors.send(true);
        held.finish().await;
    }
}

#[tokio::test]
async fn in_flight_entry_can_use_another_established_lease_with_positive_reserve() {
    let playback = Playback::start().await;
    let other = "recovery-established-other";
    playback.prepare_lease(other).await;
    let response = playback.request(other).await;
    assert_eq!(response.status(), StatusCode::OK);
    playback.assert_media(media_urls(&text(response).await)[0]).await;
    let held = playback.hold_refresh(FIRST, HlsManifestAcceptanceTrigger::Observe).await;
    playback.age_only().await;
    playback.hold_only_visible_ready().await;
    playback.exhaust_lease("warm").await;
    let other_reserve = playback.reserve(other).await;
    assert!(other_reserve.playback_position_known && other_reserve.guaranteed_reserve_ms > 0);
    let id = "recovery-reserve-b";
    playback.prepare_lease(id).await;
    let response = playback.request(id).await;
    assert_eq!(response.status(), StatusCode::OK);
    let b = playback.lease(id).await.last_manifest_snapshot.unwrap();
    assert_fallback_warning(&playback, id, &b, "fresh_commit_pending").await;
    assert_eq!(playback.reserve(id).await.initial_hidden_ready_duration_ms, 0);
    playback.assert_held(&held).await;
    eprintln!(
        "lease_a_reserve_ms=0 other_established_reserve_ms={} b_hidden_ready_ms=0 status=200",
        other_reserve.guaranteed_reserve_ms
    );
    held.finish().await;
}

#[tokio::test]
async fn exhausted_or_unproven_established_playback_cannot_open_a_recovery_entry() {
    for case in ["between_attempts", "in_flight", "unknown_cursor", "terminal"] {
        let playback = Playback::start().await;
        let held = if case == "between_attempts" {
            playback.retry_gap().await;
            None
        } else {
            let held = playback.hold_refresh(FIRST, HlsManifestAcceptanceTrigger::Observe).await;
            playback.age_only().await;
            Some(held)
        };
        let expected = match case {
            "terminal" => {
                super::super::terminalize_existing_test_lease(&playback.app, &playback.proxy.0, "warm", 0).await;
                let response = playback.request("warm").await;
                assert_eq!(response.status(), StatusCode::OK);
                let body = text(response).await;
                assert!(body.contains("#EXT-X-ENDLIST"));
                let uri = body.lines().find(|line| !line.starts_with('#') && line.contains("/terminal/")).unwrap();
                let response =
                    get_response(Arc::clone(&playback.app), uri.strip_prefix("/iptv").unwrap_or(uri), None).await;
                assert_eq!(response.status(), StatusCode::OK);
                assert!(!response_body(response).await.is_empty());
                eprintln!("terminal_manifest_endlist=true terminal_media_status=200");
                "no_established_live_reserve"
            }
            "unknown_cursor" => {
                let mut lease = playback.lease("warm").await;
                lease.playback_cursor.first_segment_started_at_ms = None;
                playback.app.hls.proxy.prepare_access_lease(lease).await;
                "no_established_live_reserve"
            }
            _ => {
                playback.exhaust_lease("warm").await;
                "established_live_reserve_exhausted"
            }
        };
        let id = "recovery-denied-b";
        let requests = playback.origin.manifest_request_count();
        playback.prepare_lease(id).await;
        let response = playback.request(id).await;
        eprintln!(
            "reserve_denial_phase={case} actual_status={} warnings={:?}",
            response.status().as_u16(),
            playback.warnings(id)
        );
        assert_retry(&response);
        assert!(playback.lease(id).await.last_manifest_snapshot.is_none());
        assert_eq!(playback.origin.manifest_request_count(), requests);
        let warnings = playback.warnings(id);
        assert_eq!(warnings.len(), 1, "{case}: {warnings:?}");
        assert!(warnings[0].contains(&format!("reason={expected} status=503 retry_after=1")));
        eprintln!("established_playback={case} reason={expected} status=503 retry_after=1 snapshot=none");
        if let Some(held) = held {
            playback.assert_held(&held).await;
            held.finish().await;
        }
    }
}

#[tokio::test]
async fn late_leases_keep_recovering_without_extending_the_entry_fallback() {
    let playback = Playback::start().await;
    playback.retry_gap().await;
    let now = playback.app.hls.proxy.now_ms();
    let (origin_progress, generation, next_attempt) = {
        let state = playback.session.read().await;
        (
            state.origin_control.progress_generation,
            state.origin_control.acceptance_generation,
            state.origin_refresh.next_fetch_allowed_at_ms,
        )
    };
    let mut late_playlist = String::new();
    for (offset, id) in [(0, "recovery-late-b"), (1, "recovery-late-c")] {
        playback.app.hls.proxy.freeze_clock_for_test(now + offset);
        playback.prepare_lease(id).await;
        let response = playback.request(id).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = text(response).await;
        playback.assert_media(media_urls(&body)[0]).await;
        assert!(playback.warnings(id)[0].contains("recovery=between_attempts"));
        if offset == 0 {
            late_playlist = body;
        }
    }
    playback.exhaust_lease("warm").await;
    let b = "recovery-late-b";
    assert!(playback.reserve(b).await.guaranteed_reserve_ms > 0);
    let denied = "recovery-late-d";
    playback.prepare_lease(denied).await;
    assert_retry(&playback.request(denied).await);
    assert!(playback.lease(denied).await.last_manifest_snapshot.is_none());
    assert!(playback.warnings(denied)[0].contains("reason=established_live_reserve_exhausted"));

    // A has stopped consuming the stream. B and C still have their own live playback.
    playback
        .app
        .hls
        .proxy
        .access_leases()
        .write()
        .await
        .remove_access_lease(&HlsAccessLeaseId("warm".to_owned()))
        .unwrap();
    let (release, gate) = watch::channel(false);
    {
        let mut script = playback.script.lock().unwrap();
        script.manifest_status = StatusCode::OK;
        script.recovery_gate = Some(gate);
    }
    let origin_gets = playback.origin.manifest_request_count();
    let commits = playback.session.read().await.origin_manifest_commits();
    playback.app.hls.proxy.freeze_clock_for_test(next_attempt);
    let response = playback.request(b).await;
    let poll_status = response.status();
    if poll_status != StatusCode::OK {
        assert_retry(&response);
    }
    tokio::time::timeout(Duration::from_secs(3), playback.recovery_at_origin.notified()).await.unwrap();
    assert!(playback.origin.manifest_request_count() > origin_gets);
    assert!(playback.session.read().await.origin_refresh.in_flight);
    playback.assert_media(media_urls(&late_playlist)[0]).await;
    eprintln!("late_lease_b_triggered_origin_after_fallback_closed origin_requests_before={origin_gets} origin_requests_now={} poll_status={poll_status}",
        playback.origin.manifest_request_count());
    release.send(true).unwrap();
    playback.wait_for_refresh_to_finish().await;
    {
        let state = playback.session.read().await;
        assert_eq!(state.origin_control.progress_generation, origin_progress);
        assert!(state.origin_manifest_commits() > commits, "the origin accepted the unchanged playlist");
    }

    let advanced = playback.hold_refresh_for(FIRST + 1, HlsManifestAcceptanceTrigger::Observe, b).await;
    assert!(advanced.generation > generation.0);
    playback.age_only().await;
    let later_denied = "recovery-late-e";
    playback.prepare_lease(later_denied).await;
    let response = playback.request(later_denied).await;
    eprintln!("new_internal_generation={} entry_status={}", advanced.generation, response.status().as_u16());
    assert_retry(&response);
    assert!(playback.lease(later_denied).await.last_manifest_snapshot.is_none());
    assert!(playback.warnings(later_denied)[0].contains("reason=established_live_reserve_exhausted"));
    advanced.finish().await;
    playback.ready(FIRST + 1, FIRST + 7).await;
    assert!(playback.session.read().await.origin_control.progress_generation > origin_progress);

    let later = playback.hold_refresh_for(FIRST + 1, HlsManifestAcceptanceTrigger::Observe, b).await;
    playback.age_only().await;
    let next = "recovery-late-after-progress";
    playback.prepare_lease(next).await;
    assert_eq!(playback.request(next).await.status(), StatusCode::OK);
    let publication = playback.lease(next).await.last_manifest_snapshot.unwrap();
    assert_fallback_warning(&playback, next, &publication, "fresh_commit_pending").await;
    playback.assert_held(&later).await;
    eprintln!("later_generation={} after_new_playable_origin_media=true status=200", later.generation);
    later.finish().await;
}

#[tokio::test]
async fn ready_media_behind_a_gap_cannot_reopen_the_recovery_entry() {
    let playback = Playback::start().await;
    let first_new_msn = FIRST + 6;
    playback.script.lock().unwrap().missing_media = Some(first_new_msn);
    let advancing = playback.hold_refresh(FIRST + 2, HlsManifestAcceptanceTrigger::Observe).await;
    playback.age_only().await;
    let active = "recovery-gap-established-b";
    playback.prepare_lease(active).await;
    let response = playback.request(active).await;
    assert_eq!(response.status(), StatusCode::OK);
    playback.assert_media(media_urls(&text(response).await)[0]).await;
    let source = playback.lease(active).await.last_manifest_snapshot.unwrap().source_commit_identity;
    playback.exhaust_lease("warm").await;
    let denied = "recovery-gap-closed";
    playback.prepare_lease(denied).await;
    assert_retry(&playback.request(denied).await);
    assert!(playback.warnings(denied)[0].contains("reason=established_live_reserve_exhausted"));
    playback.app.hls.proxy.access_leases().write().await.remove_access_lease(&HlsAccessLeaseId("warm".into())).unwrap();

    let generation = advancing.generation;
    advancing.finish().await;
    playback.ready(first_new_msn + 1, first_new_msn + 2).await;
    let first_new_seq = first_new_msn - FIRST;
    tokio::time::timeout(Duration::from_secs(3), async {
        while !matches!(
            playback.session.read().await.segments[&first_new_seq].status,
            SegmentCacheStatus::FailedPermanent { .. }
        ) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the origin returned 404 for the predecessor");
    let stalled = playback.hold_refresh_for(FIRST + 2, HlsManifestAcceptanceTrigger::Observe, active).await;
    assert!(stalled.generation > generation);
    playback.age_only().await;
    assert!(playback.reserve(active).await.guaranteed_reserve_ms > 0);
    {
        let state = playback.session.read().await;
        assert_eq!(state.last_normal_manifest_commit_identity(), Some(source));
        assert!(state.last_rendered_manifest.as_ref().unwrap().last_proxy_seq < first_new_seq);
    }
    let late = "recovery-gap-late";
    playback.prepare_lease(late).await;
    let response = playback.request(late).await;
    eprintln!(
        "isolated_ready first_failed_seq={first_new_seq} ready_successor_seq={} retained_source={} new_generation={} entry_status={}",
        first_new_seq + 1, source.commit_generation(), stalled.generation, response.status().as_u16()
    );
    assert_retry(&response);
    assert!(playback.lease(late).await.last_manifest_snapshot.is_none());
    assert!(playback.warnings(late)[0].contains("reason=established_live_reserve_exhausted"));
    stalled.finish().await;

    let repaired_head = first_new_msn + 1;
    playback.hold_refresh_for(repaired_head, HlsManifestAcceptanceTrigger::Observe, active).await.finish().await;
    playback.ready(repaired_head, repaired_head + 6).await;
    let committed = playback.session.read().await.last_normal_manifest_commit_identity().unwrap();
    assert_ne!(committed, source, "the playable new window became the committed source");
    let later = playback.hold_refresh_for(repaired_head, HlsManifestAcceptanceTrigger::Observe, active).await;
    playback.age_only().await;
    let next = "recovery-gap-after-publication";
    playback.prepare_lease(next).await;
    assert_eq!(playback.request(next).await.status(), StatusCode::OK);
    let publication = playback.lease(next).await.last_manifest_snapshot.unwrap();
    assert_eq!(publication.source_commit_identity, committed);
    assert_fallback_warning(&playback, next, &publication, "fresh_commit_pending").await;
    eprintln!(
        "playable_source_commit={} later_generation={} entry_status=200",
        committed.commit_generation(),
        later.generation
    );
    later.finish().await;
}

#[tokio::test]
async fn explicit_freshness_and_account_boundaries_prevent_recovery_publication() {
    use tuliprox_hls::api::HlsFreshManifestRequiredReason as Reason;
    for (id, reason) in [
        ("recovery-provisioning-b", Some(Reason::ProvisioningHandoff)),
        ("recovery-reserve-barrier-b", Some(Reason::OriginReserveExhausted)),
        ("recovery-revalidation-b", Some(Reason::ExpiredRevalidation)),
        ("recovery-account-barrier-b", None),
    ] {
        let playback = Playback::start().await;
        let held = playback.hold_refresh(FIRST, HlsManifestAcceptanceTrigger::Observe).await;
        playback.age_only().await;
        {
            let mut state = playback.session.write().await;
            if let Some(reason) = reason {
                state.require_fresh_manifest_commit(reason);
            } else {
                state.origin_account_manifest_pending = Some(1);
            }
        }
        playback.prepare_lease(id).await;
        let response = playback.request(id).await;
        assert_ne!(response.status(), StatusCode::OK, "{id}");
        assert!(playback.lease(id).await.last_manifest_snapshot.is_none());
        assert_eq!(playback.warnings(id), Vec::<String>::new());
        playback.assert_held(&held).await;
        eprintln!("separate_boundary={id} status={} snapshot=none", response.status().as_u16());
        held.finish().await;
    }
}

#[tokio::test]
async fn recovery_entry_commit_rechecks_the_current_source_reserve_and_live_boundaries() {
    use crate::api::endpoints::hls_api::manifest::{HlsManifestPublicationBarrier, HLS_MANIFEST_PUBLICATION_BARRIER};
    use tuliprox_hls::{api::HlsLeasePlaybackMode, HlsTerminalTailCompatibility};
    for changed in ["source", "reserve", "episode", "binding", "revalidation", "terminal", "regular"] {
        let playback = Playback::start().await;
        let origin_msn = if changed == "regular" { FIRST + 1 } else { FIRST };
        let held = playback.hold_refresh(origin_msn, HlsManifestAcceptanceTrigger::Observe).await;
        playback.age_only().await;
        let id = "recovery-race-b";
        playback.prepare_lease(id).await;
        let barrier = Arc::new(HlsManifestPublicationBarrier::default());
        let scope = Arc::clone(&barrier);
        let app = Arc::clone(&playback.app);
        let input = playback.input.clone();
        let url = playback.url.clone();
        let warnings = Arc::clone(&playback.warnings);
        let task = tokio::spawn(async move {
            HLS_ENTRY_WARNINGS
                .scope(
                    warnings,
                    HLS_MANIFEST_PUBLICATION_BARRIER.scope(
                        std::cell::RefCell::new(Some(scope)),
                        canonical_response_for_lease(&app, &input, &url, id),
                    ),
                )
                .await
        });
        tokio::time::timeout(Duration::from_secs(2), barrier.prepared.notified()).await.unwrap();
        assert!(playback.lease(id).await.last_manifest_snapshot.is_none());
        let mut held = Some(held);
        match changed {
            "terminal" => {
                let mut lease = playback.lease(id).await;
                lease.playback_mode = HlsLeasePlaybackMode::TerminalUnavailable {
                    decision_generation: 1,
                    reason: HlsTerminalTailCompatibility::MissingAsset,
                };
                playback.app.hls.proxy.prepare_access_lease(lease).await;
            }
            "reserve" => playback.exhaust_lease("warm").await,
            "regular" => {
                held.take().unwrap().finish().await;
                playback.ready(FIRST + 1, FIRST + 7).await;
            }
            _ => {
                let mut state = playback.session.write().await;
                match changed {
                    "source" => {
                        state.last_rendered_manifest.as_mut().unwrap().valid_until_ms =
                            playback.app.hls.proxy.now_ms() - 1;
                    }
                    "episode" => state.origin_control.acceptance_episode.as_mut().unwrap().complete(),
                    "binding" => {
                        state.startup.as_mut().unwrap().revisions.remove(&0).unwrap();
                    }
                    "revalidation" => state.live_entry_revalidation.required = true,
                    _ => unreachable!(),
                }
            }
        }
        barrier.resume.notify_one();
        let response = tokio::time::timeout(Duration::from_secs(5), task).await.unwrap().unwrap();
        if changed == "regular" {
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(manifest_media_sequence(&text(response).await), 1);
            assert!(playback.lease(id).await.last_manifest_snapshot.is_some());
            assert_eq!(playback.warnings(id), Vec::<String>::new());
            eprintln!("publication_race=regular status=200 snapshot=regular");
            continue;
        }
        if changed == "terminal" {
            assert_ne!(response.status(), StatusCode::OK);
            assert!(response.headers().get(header::RETRY_AFTER).is_none());
            assert_eq!(playback.warnings(id), Vec::<String>::new());
        } else if changed == "revalidation" {
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(playback.warnings(id), Vec::<String>::new());
        } else {
            assert_retry(&response);
            let reason = match changed {
                "source" => Some("no_renderable_window"),
                "reserve" => Some("established_live_reserve_exhausted"),
                "binding" => Some("missing_window_bindings"),
                _ => None,
            };
            if let Some(reason) = reason {
                assert!(playback.warnings(id).iter().any(|line| line.contains(&format!("reason={reason}"))));
            }
        }
        assert!(playback.lease(id).await.last_manifest_snapshot.is_none());
        if changed != "revalidation" {
            playback.assert_held(held.as_ref().unwrap()).await;
        }
        eprintln!("publication_race={changed} status={} snapshot=none", response.status().as_u16());
        held.unwrap().finish().await;
    }
}
