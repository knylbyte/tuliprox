use super::{
    access_lease_id_from_variant_uri, get_response, proxy_session_id_from_variant_uri, response_body,
    shared_request_flow_entry, shared_request_flow_fixture_with_origin, single_variant_uri, test_fingerprint, Arc,
    Duration, HlsAccessLeaseId, HlsSessionHandle, ProxySessionId, SegmentCacheStatus, SharedRequestFlowFixture,
    StatusCode,
};
use crate::api::endpoints::hls_api::tests::{spawn_test_binary_origin, support::TestBinaryOriginResponse};
use axum::{body::Body, http::Response};
use shared::model::{HlsSegmentTimeout, HlsStartupMode, HlsStripMode, Millis};
use std::fmt::Write;
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
    time::{sleep, timeout},
};
use tuliprox_core::utils::current_time_millis;

const FIRST_MSN: u64 = 1013;
const FAST_WINDOW: [u64; 3] = [1014, 1015, 1016];
const BODY: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../test/fixtures/hls/channel_unavailable.ts"));
const HEAD_PREFIX_BYTES: usize = 188 * 20;
const TEST_DEADLINE: Duration = Duration::from_secs(6);
const QUIET_INTERVAL: Duration = Duration::from_millis(250);

struct OriginReplies {
    manifest: watch::Sender<bool>,
    headers: watch::Sender<bool>,
    prefix: watch::Sender<bool>,
    finish_head: watch::Sender<bool>,
    finish_body: watch::Sender<bool>,
}

#[derive(Clone, Copy)]
enum OriginBehavior {
    Ready,
    HoldOptimisticBodies,
    MissingHead,
    HoldFourthBody,
}

struct ColdStart {
    fixture: SharedRequestFlowFixture,
    session: HlsSessionHandle,
    lease_id: HlsAccessLeaseId,
    proxy_id: ProxySessionId,
    response: JoinHandle<Response<Body>>,
    arrivals: mpsc::UnboundedReceiver<(u64, StatusCode)>,
    replies: OriginReplies,
}

impl ColdStart {
    async fn new(mode: HlsStartupMode, behavior: OriginBehavior, strip: u64, fetch_slots: usize) -> Self {
        let _ = env_logger::Builder::new()
            .filter_module("tuliprox_hls::segment_fetcher::scheduler", log::LevelFilter::Debug)
            .is_test(true)
            .try_init();
        let mut manifest = format!("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:{FIRST_MSN}\n");
        for msn in FIRST_MSN..FIRST_MSN + 6 {
            writeln!(manifest, "#EXTINF:4,\n/{msn}.ts").unwrap();
        }
        let manifest: Arc<[u8]> = Arc::from(manifest.into_bytes());
        let media: Arc<[u8]> = Arc::from(BODY);
        let (seen, arrivals) = mpsc::unbounded_channel();
        let hold_bodies = matches!(behavior, OriginBehavior::HoldOptimisticBodies);
        let missing_head = matches!(behavior, OriginBehavior::MissingHead);
        let hold_fourth_body = matches!(behavior, OriginBehavior::HoldFourthBody);
        let replies = OriginReplies {
            manifest: watch::Sender::new(false),
            headers: watch::Sender::new(!(hold_bodies || missing_head)),
            prefix: watch::Sender::new(!hold_bodies),
            finish_head: watch::Sender::new(!hold_bodies),
            finish_body: watch::Sender::new(!(hold_bodies || hold_fourth_body)),
        };
        let manifest_gate = replies.manifest.subscribe();
        let header_gate = replies.headers.subscribe();
        let prefix_gate = replies.prefix.subscribe();
        let head_gate = replies.finish_head.subscribe();
        let body_gate = replies.finish_body.subscribe();
        let origin = spawn_test_binary_origin(Arc::new(move |path| {
            if path == "/channel/index.m3u8" {
                let mut response = TestBinaryOriginResponse::new(StatusCode::OK, Arc::clone(&manifest));
                response.gate = Some(manifest_gate.clone());
                return response;
            }
            let msn =
                path.strip_prefix('/').and_then(|path| path.strip_suffix(".ts")).and_then(|path| path.parse().ok());
            let Some(msn) = msn else { return TestBinaryOriginResponse::new(StatusCode::NOT_FOUND, Arc::from([])) };
            let (status, body) = if missing_head && msn == FAST_WINDOW[0] {
                (StatusCode::NOT_FOUND, Arc::from([]))
            } else {
                (StatusCode::OK, Arc::clone(&media))
            };
            let _ = seen.send((msn, status));
            let mut response = TestBinaryOriginResponse::new(status, body);
            if (hold_bodies || missing_head) && FAST_WINDOW.contains(&msn) {
                response.gate = Some(header_gate.clone());
            }
            if hold_fourth_body && msn == FIRST_MSN + 3 {
                response.body_gates.push((0, body_gate.clone()));
            }
            if hold_bodies && FAST_WINDOW.contains(&msn) {
                if msn == FAST_WINDOW[0] {
                    response.body_gates.push((0, prefix_gate.clone()));
                    response.body_gates.push((HEAD_PREFIX_BYTES, head_gate.clone()));
                } else {
                    response.body_gates.push((0, body_gate.clone()));
                }
            }
            response
        }))
        .await;
        let fixture = shared_request_flow_fixture_with_origin(origin, 3).await;
        let app = &fixture.app_state;
        let mut config = app.app_config.config.load().as_ref().clone();
        let cache = config.reverse_proxy.as_mut().unwrap().hls_cache.as_mut().unwrap();
        cache.startup.mode = mode;
        cache.strip.mode = HlsStripMode::Segments;
        cache.strip.value = strip;
        cache.max_concurrent_segment_fetches_per_session = fetch_slots;
        cache.origin_segment_timeout_ms = HlsSegmentTimeout::Fixed(Millis::new(10_000));
        app.app_config.config.store(Arc::new(config));
        app.hls.proxy.update_config(&app.app_config).await;
        let uri = single_variant_uri(shared_request_flow_entry(&fixture, &test_fingerprint()).await).await;
        let proxy_id = ProxySessionId(proxy_session_id_from_variant_uri(&uri).to_owned());
        let lease_id = HlsAccessLeaseId(access_lease_id_from_variant_uri(&uri).to_owned());
        let response = {
            let app = Arc::clone(app);
            tokio::spawn(async move { get_response(app, &uri, None).await })
        };
        let session = timeout(TEST_DEADLINE, async {
            loop {
                if let Some(session) = app.hls.proxy.sessions().get_by_proxy_session_id(&proxy_id).await {
                    break session;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the canonical request creates a session before the origin manifest is released");
        Self { fixture, session, lease_id, proxy_id, response, arrivals, replies }
    }

    async fn next_selected_get(&mut self, selected: &[u64]) -> (u64, StatusCode) {
        loop {
            let (msn, status) = self.arrivals.recv().await.expect("the origin is running");
            if selected.contains(&msn) {
                return (msn, status);
            }
        }
    }

    async fn assert_next_get(&mut self, selected: &[u64], expected: u64, status: StatusCode) {
        let arrival = timeout(TEST_DEADLINE, self.next_selected_get(selected)).await.expect("origin GET");
        eprintln!("origin_get msn={} proxy_seq={} status={}", arrival.0, arrival.0 - FIRST_MSN, arrival.1.as_u16());
        assert_eq!(arrival, (expected, status));
    }

    async fn assert_gets_before_header_release(&mut self, expected: &[(u64, StatusCode)]) {
        assert!(!*self.replies.headers.borrow());
        let mut actual = Vec::with_capacity(expected.len());
        timeout(TEST_DEADLINE, async {
            for _ in expected {
                let (msn, status) = self.arrivals.recv().await.expect("the origin is running");
                eprintln!("origin_get msn={msn} planned_status={} header_gate_open=false", status.as_u16());
                actual.push((msn, status));
            }
        })
        .await
        .unwrap_or_else(|_| panic!("expected GETs {expected:?} before header release, received {actual:?}"));
        actual.sort_unstable_by_key(|(msn, _)| *msn);
        assert_eq!(actual, expected);
    }

    async fn assert_publication(&mut self, expected_msns: &[u64]) {
        let response = timeout(TEST_DEADLINE, &mut self.response).await.expect("canonical playlist").unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = String::from_utf8(response_body(response).await.to_vec()).unwrap();
        assert!(!body.contains("#EXT-X-ENDLIST"));
        let lease = self
            .fixture
            .app_state
            .hls
            .proxy
            .access_lease_response_snapshot(&self.lease_id, &self.proxy_id, current_time_millis())
            .await
            .expect("published lease");
        let snapshot = lease.last_manifest_snapshot.expect("the playlist is bound to this lease");
        let session = self.session.read().await;
        let proxy_seqs: Vec<_> = snapshot.visible_proxy_seqs().collect();
        let msns: Vec<_> = proxy_seqs.iter().map(|seq| session.segments[seq].origin_key.host_local_sequence).collect();
        assert_eq!(msns, expected_msns);
        let media: Vec<_> = body.lines().filter(|line| !line.is_empty() && !line.starts_with('#')).collect();
        assert_eq!(media.len(), proxy_seqs.len());
        for (uri, seq) in media.into_iter().zip(&proxy_seqs) {
            assert!(uri.ends_with(&format!("/{seq:06}.ts")));
        }
        eprintln!("client_manifest status=200 proxy_seqs={proxy_seqs:?} origin_msns={msns:?}");
        for line in body.lines() {
            // The test playlist only contains media URIs and tags without URI attributes.
            eprintln!("{}", if line.starts_with('#') { line } else { line.rsplit('/').next().unwrap() });
        }
    }
}

#[tokio::test]
async fn conservative_start_orders_first_origin_gets() {
    assert_start_get_order(HlsStartupMode::Conservative, &[1013, 1014, 1015, 1016]).await;
}

#[tokio::test]
async fn conservative_start_keeps_remaining_gets_ordered_after_first_render() {
    let window = [1013, 1014, 1015, 1016, 1017, 1018];
    let mut start = ColdStart::new(HlsStartupMode::Conservative, OriginBehavior::HoldFourthBody, 0, 2).await;
    let barrier = start.session.write().await.pause_startup_segment_request_for_test(1017 - FIRST_MSN);
    start.replies.manifest.send_replace(true);
    for msn in &window[..4] {
        start.assert_next_get(&window, *msn, StatusCode::OK).await;
    }
    timeout(TEST_DEADLINE, barrier.prepare_ready.notified()).await.expect("fifth segment pauses before HTTP");
    start.replies.finish_body.send_replace(true);
    let active_fetches = timeout(TEST_DEADLINE, async {
        loop {
            {
                let session = start.session.read().await;
                if session
                    .last_rendered_manifest
                    .as_ref()
                    .is_some_and(|manifest| manifest.segment_proxy_seqs.len() == window.len())
                    && matches!(
                        session.segments[&(1018 - FIRST_MSN)].status,
                        SegmentCacheStatus::Fetching { .. } | SegmentCacheStatus::Ready { .. }
                    )
                {
                    break session.active_segment_fetches;
                }
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the first render and the sixth fetch are scheduled while the fifth is paused");
    let early_get = timeout(Duration::from_millis(250), start.next_selected_get(&window)).await;
    eprintln!("startup mode=Conservative strip=0 fetch_slots=2 first_render_stored=true fifth_delayed=true early_get={early_get:?}");
    barrier.resume_prepare.notify_one();
    assert!(early_get.is_err(), "no GET overtakes the delayed fifth segment after the first render");
    assert_eq!(active_fetches, 2);
    for msn in &window[4..] {
        start.assert_next_get(&window, *msn, StatusCode::OK).await;
    }
    start.assert_publication(&window).await;
}

#[tokio::test]
async fn first_ready_start_orders_first_origin_gets() {
    assert_start_get_order(HlsStartupMode::FirstReady, &FAST_WINDOW).await;
}

#[tokio::test]
async fn progressive_start_orders_first_origin_gets() {
    assert_start_get_order(HlsStartupMode::Progressive, &FAST_WINDOW).await;
}

async fn assert_start_get_order(mode: HlsStartupMode, window: &[u64]) {
    let mut start = ColdStart::new(mode, OriginBehavior::Ready, 2, 3).await;
    start.replies.manifest.send_replace(true);
    eprintln!("startup mode={mode:?} strip=2 fetch_slots=3");
    for msn in window {
        start.assert_next_get(window, *msn, StatusCode::OK).await;
    }
    start.assert_publication(window).await;
}

#[tokio::test]
async fn optimistic_start_gets_three_segments_before_headers_and_publishes_from_head_prefix() {
    let mut start = ColdStart::new(HlsStartupMode::Optimistic, OriginBehavior::HoldOptimisticBodies, 2, 3).await;
    eprintln!("startup mode=Optimistic strip=2 fetch_slots=3 header_gate_open=false");
    start.replies.manifest.send_replace(true);
    start.assert_gets_before_header_release(&FAST_WINDOW.map(|msn| (msn, StatusCode::OK))).await;
    start.replies.headers.send_replace(true);
    eprintln!("origin_headers_released msns={FAST_WINDOW:?} status=200 prefix_released=false bodies_released=false");
    assert!(timeout(QUIET_INTERVAL, &mut start.response).await.is_err(), "startup waits for the head prefix");
    start.replies.prefix.send_replace(true);
    eprintln!("origin_head_prefix_released msn=1014 bytes={HEAD_PREFIX_BYTES}");
    start.assert_publication(&FAST_WINDOW).await;
    assert!(!*start.replies.finish_head.borrow());
    assert!(!*start.replies.finish_body.borrow());
    assert_eq!(start.session.read().await.active_segment_fetches, 3);
    eprintln!("client_manifest_published active_fetches=3 bodies_released=false");
    start.replies.finish_head.send_replace(true);
    start.replies.finish_body.send_replace(true);
    eprintln!("origin_bodies_released msns={FAST_WINDOW:?}");
}

#[tokio::test]
async fn optimistic_start_advances_after_head_returns_404_with_successors_in_flight() {
    let mut start = ColdStart::new(HlsStartupMode::Optimistic, OriginBehavior::MissingHead, 2, 3).await;
    eprintln!("startup mode=Optimistic strip=2 fetch_slots=3 header_gate_open=false");
    start.replies.manifest.send_replace(true);
    start
        .assert_gets_before_header_release(&[
            (1014, StatusCode::NOT_FOUND),
            (1015, StatusCode::OK),
            (1016, StatusCode::OK),
        ])
        .await;
    start.replies.headers.send_replace(true);
    eprintln!("origin_headers_released msn=1014 status=404 successors=[1015, 1016] successor_status=200");
    start.assert_publication(&[1015, 1016, 1017]).await;
}

#[tokio::test]
async fn optimistic_start_waits_for_a_free_fetch_slot_before_third_get() {
    let mut start = ColdStart::new(HlsStartupMode::Optimistic, OriginBehavior::HoldOptimisticBodies, 2, 2).await;
    eprintln!("startup mode=Optimistic strip=2 fetch_slots=2 header_gate_open=false");
    start.replies.manifest.send_replace(true);
    start.assert_gets_before_header_release(&[(1014, StatusCode::OK), (1015, StatusCode::OK)]).await;
    start.replies.headers.send_replace(true);
    eprintln!("origin_header_gate_open=true bodies_released=false");
    let early_get = timeout(QUIET_INTERVAL, start.arrivals.recv()).await;
    assert!(early_get.is_err(), "both fetch slots are still occupied, got {early_get:?}");
    assert_eq!(start.session.read().await.active_segment_fetches, 2);
    eprintln!("no_third_get active_fetches=2 bodies_released=false");

    start.replies.prefix.send_replace(true);
    start.replies.finish_head.send_replace(true);
    eprintln!("origin_body_released msn=1014");
    let third = timeout(TEST_DEADLINE, start.arrivals.recv()).await.expect("the freed slot admits the third GET");
    assert_eq!(third, Some((1016, StatusCode::OK)));
    assert_eq!(start.session.read().await.active_segment_fetches, 2);
    eprintln!("origin_get msn=1016 active_fetches=2 successor_bodies_released=false");
    start.assert_publication(&FAST_WINDOW).await;
    start.replies.finish_body.send_replace(true);
    eprintln!("origin_bodies_released msns=[1015, 1016]");
}
