use super::*;
use crate::api::model::{AppState, HlsSessionHandle, SegmentCacheStatus};
use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, Uri},
    response::Response,
};
use std::sync::{atomic::AtomicBool, Mutex};

const HOST: &str = "127.0.0.1";
const ALTERNATE_HOST: &str = "localhost";

fn window(first: u64, last: u64, namespace: &str) -> String {
    let mut body = format!("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:{first}\n");
    for msn in first..=last {
        writeln!(body, "#EXTINF:4,\n/{namespace}/{msn}.ts").unwrap();
    }
    body
}

fn media_uris(body: &str) -> Vec<&str> { body.lines().filter(|line| line.starts_with("/hls/")).collect() }

#[derive(Clone, Debug)]
struct Request {
    path: String,
    host: String,
    epoch: u64,
    commits: u64,
}

#[derive(Default)]
struct ManifestScript {
    redirect: Option<String>,
    alternate: Option<String>,
    fallback: Option<String>,
}

struct Origin {
    manifest: Mutex<ManifestScript>,
    session: Mutex<Option<HlsSessionHandle>>,
    requests: Mutex<Vec<Request>>,
    reject_new_media: AtomicBool,
}

async fn serve(State(origin): State<Arc<Origin>>, uri: Uri, headers: HeaderMap) -> Response {
    let path = uri.path();
    let host = headers.get(header::HOST).unwrap().to_str().unwrap().split(':').next().unwrap();
    let session = origin.session.lock().unwrap().clone();
    let (epoch, commits) = if let Some(session) = session {
        let state = session.read().await;
        (state.origin_epoch, state.origin_manifest_commits())
    } else {
        (0, 0)
    };
    origin.requests.lock().unwrap().push(Request { path: path.to_owned(), host: host.to_owned(), epoch, commits });
    if path_has_extension(path, "m3u8") {
        let mut manifest = origin.manifest.lock().unwrap();
        if host == HOST && path == "/index.m3u8" {
            if let Some(url) = manifest.redirect.take() {
                return Response::builder()
                    .status(StatusCode::TEMPORARY_REDIRECT)
                    .header(header::LOCATION, url)
                    .body(Body::empty())
                    .unwrap();
            }
        } else if let Some(body) = manifest.alternate.take() {
            return Response::new(Body::from(body));
        }
        manifest.fallback.as_ref().map_or_else(
            || Response::builder().status(StatusCode::SERVICE_UNAVAILABLE).body(Body::empty()).unwrap(),
            |body| Response::new(Body::from(body.clone())),
        )
    } else {
        let marker = match path.split('/').nth(1) {
            Some("a") => 1,
            Some("b") => 2,
            Some("c") => 3,
            _ => panic!("unexpected media path {path}"),
        };
        if marker > 1 && origin.reject_new_media.load(Ordering::Acquire) {
            return Response::builder().status(StatusCode::NOT_FOUND).body(Body::empty()).unwrap();
        }
        Response::new(Body::from(media(marker)))
    }
}

struct Playback {
    app: Arc<AppState>,
    input: ConfigInput,
    url: String,
    origin: Arc<Origin>,
    session: HlsSessionHandle,
    first: String,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Playback {
    fn drop(&mut self) { self.task.abort(); }
}

impl Playback {
    async fn start(strip: u64) -> Self {
        trace_fixture();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let origin = Arc::new(Origin {
            manifest: Mutex::new(ManifestScript { fallback: Some(window(310, 312, "a")), ..Default::default() }),
            session: Mutex::new(None),
            requests: Mutex::new(Vec::new()),
            reject_new_media: AtomicBool::new(false),
        });
        let router = axum::Router::new().fallback(serve).with_state(Arc::clone(&origin));
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let input = ConfigInput {
            id: 1,
            name: "rollover-origin".into(),
            input_type: InputType::M3u,
            url: base.clone(),
            max_connections: 1,
            enabled: true,
            ..Default::default()
        };
        let app = test_app_state_with_inputs(vec![Arc::new(input.clone())]);
        enable_unstripped_cache(&app, HlsStartupMode::Conservative).await;
        super::super::support::configure_test_hls_strip(
            &app,
            &crate::model::StripConfig { mode: shared::model::HlsStripMode::Segments, value: strip },
        )
        .await;
        create_active_hls_user_session(&app).await;
        let url = format!("{base}/index.m3u8");
        let response = canonical_response_for_lease(&app, &input, &url, "warm").await;
        assert_eq!(response.status(), StatusCode::OK);
        let first = String::from_utf8(response_body(response).await.to_vec()).unwrap();
        let key = super::super::super::build_hls_origin_source(&input, "12345").session_key();
        let session = app.hls.proxy.sessions().get_by_key(&key).await.unwrap();
        *origin.session.lock().unwrap() = Some(Arc::clone(&session));
        let playback = Self { app, input, url, origin, session, first, task };
        for uri in media_uris(&playback.first) {
            playback.assert_media(uri, 1).await;
        }
        playback
    }

    fn script(&self, body: String) {
        *self.origin.manifest.lock().unwrap() = ManifestScript { fallback: Some(body), ..Default::default() };
        self.origin.requests.lock().unwrap().clear();
    }

    fn script_redirect_once(&self, body: String, fallback: Option<String>) {
        let alternate =
            self.url.replace(HOST, ALTERNATE_HOST).replace("/index.m3u8", "/delivery/alternate.m3u8?turn=2");
        self.script_manifest_redirect(alternate, body, fallback);
    }

    fn script_same_host_redirect(&self, body: String) {
        let target = "/delivery/returning.m3u8?turn=3".to_string();
        self.script_manifest_redirect(target, body.clone(), Some(body));
    }

    fn script_manifest_redirect(&self, target: String, body: String, fallback: Option<String>) {
        *self.origin.manifest.lock().unwrap() =
            ManifestScript { redirect: Some(target), alternate: Some(body), fallback };
        self.origin.requests.lock().unwrap().clear();
    }

    async fn refresh(&self, body: String) {
        self.script(body);
        self.refresh_script().await;
    }

    async fn refresh_script(&self) {
        self.session.write().await.origin_refresh.next_fetch_allowed_at_ms = 0;
        refresh_playing_lease(&self.app, &self.session, &self.url).await;
    }

    async fn manifest(&self, lease: &str) -> String {
        if lease == "warm" {
            self.session.write().await.origin_refresh.next_fetch_allowed_at_ms = u64::MAX;
        }
        let response = canonical_response_for_lease(&self.app, &self.input, &self.url, lease).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = String::from_utf8(response_body(response).await.to_vec()).unwrap();
        assert!(body.starts_with("#EXTM3U\n") && !body.contains("#EXT-X-ENDLIST"), "{body}");
        body
    }

    async fn assert_media(&self, uri: &str, marker: u8) {
        assert_eq!(Sha256::digest(get_bound_media(&self.app, uri).await), Sha256::digest(media(marker)));
    }

    async fn wait_for_ready(&self, seq: u64) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if self
                    .session
                    .read()
                    .await
                    .segments
                    .get(&seq)
                    .is_some_and(|entry| matches!(entry.status, SegmentCacheStatus::Ready { .. }))
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    }

    async fn assert_epoch(&self, epoch: u64, start: u64, first_msn: u64, last_msn: u64) {
        let state = self.session.read().await;
        assert_eq!(state.origin_epoch, epoch);
        let mapped = state
            .segments
            .values()
            .filter(|entry| entry.origin_key.origin_epoch == epoch)
            .map(|entry| (entry.proxy_seq, entry.origin_key.host_local_sequence, entry.discontinuity_before))
            .collect::<Vec<_>>();
        let expected = (first_msn..=last_msn)
            .enumerate()
            .map(|(offset, msn)| (start + u64::try_from(offset).unwrap(), msn, offset == 0 && epoch > 0))
            .collect::<Vec<_>>();
        assert_eq!(mapped, expected);
        assert_eq!(state.proxy_next_seq, Some(start + last_msn - first_msn + 1));
        eprintln!("epoch={epoch} mapped={mapped:?}");
    }

    async fn idle_at(&self, delta_ms: u64) -> u64 {
        // Request and media authorization must agree on the wall-clock lease timestamps.
        // Age the confirmed media evidence while freezing only the admission decision clock.
        let now = super::super::super::current_time_millis();
        let mut state = self.session.write().await;
        assert!(state.activity.last_delivered_media_at_ms.is_some());
        state.activity.last_delivered_media_at_ms = Some(now - delta_ms);
        self.app.hls.proxy.freeze_clock_for_test(now);
        now
    }

    fn assert_media_follows_commit(&self, namespace: &str, old_epoch: u64, old_commits: u64, manifest_hosts: &[&str]) {
        let requests = self.origin.requests.lock().unwrap();
        let manifests = requests.iter().filter(|request| path_has_extension(&request.path, "m3u8")).collect::<Vec<_>>();
        assert_eq!(
            manifests.iter().map(|request| request.host.as_str()).collect::<Vec<_>>(),
            manifest_hosts,
            "{requests:?}"
        );
        assert!(manifests.iter().all(|request| (request.epoch, request.commits) == (old_epoch, old_commits)));
        let next = requests.iter().filter(|request| request.path.starts_with(namespace)).collect::<Vec<_>>();
        assert!(!next.is_empty(), "{requests:?}");
        assert!(next.iter().all(|request| request.host == *manifest_hosts.last().unwrap()));
        assert!(next.iter().all(|request| (request.epoch, request.commits) == (old_epoch + 1, old_commits + 1)));
        eprintln!("manifest_then_commit_then_media requests={requests:?}");
    }
}

fn assert_boundary(body: &str, proxy_seq: u64) {
    let first = manifest_media_sequence(body);
    let uri = media_uris(body)[usize::try_from(proxy_seq - first).unwrap()];
    let mut lines = body.lines().rev().skip_while(|line| *line != uri);
    assert_eq!(lines.next(), Some(uri));
    assert!(lines.next().unwrap().starts_with("#EXTINF:"));
    assert_eq!(lines.next(), Some("#EXT-X-DISCONTINUITY"));
}

#[tokio::test]
async fn observed_rollover_uses_next_proxy_slot_without_recovery_or_probe() {
    let playback = Playback::start(0).await;
    playback.refresh(window(310, 312, "a")).await;
    let (commits, generation) = {
        let state = playback.session.read().await;
        assert!(state.origin_control.manifest_host_expiry_for_test(HOST).is_some());
        (state.origin_manifest_commits(), state.origin_control.acceptance_generation)
    };
    playback.refresh(window(0, 2, "b")).await;
    playback.wait_for_ready(5).await;
    playback.assert_epoch(1, 3, 0, 2).await;
    playback.assert_media_follows_commit("/b/", 0, commits, &[HOST]);
    {
        let state = playback.session.read().await;
        assert_eq!(state.origin_control.acceptance_generation, generation);
        assert!(state.origin_control.acceptance_episode.is_none());
        assert!(state.origin_control.manifest_host_expiry_for_test(HOST).is_none());
    }
    let next = playback.manifest("warm").await;
    assert_boundary(&next, 3);
    assert_eq!(next.lines().filter(|line| *line == "#EXT-X-DISCONTINUITY").count(), 1);
    let start = manifest_media_sequence(&next);
    for uri in &media_uris(&next)[usize::try_from(3 - start).unwrap()..] {
        playback.assert_media(uri, 2).await;
    }
    playback.assert_media(media_uris(&playback.first)[0], 1).await;
    eprintln!("rollover_manifests first={:?} next={next:?}", playback.first);
}

#[tokio::test]
async fn confirmed_alternate_host_rolls_over_a_short_window_without_recovery() {
    use shared::model::HlsManifestRecoveryBurstLevel;
    use tuliprox_hls::{api::HlsManifestAcceptanceDirective, HlsManifestAcceptanceTrigger};

    let playback = Playback::start(0).await;
    let mut config = playback.app.app_config.config.load_full().as_ref().clone();
    config.reverse_proxy.as_mut().unwrap().hls_cache.as_mut().unwrap().manifest_recovery_burst.level =
        HlsManifestRecoveryBurstLevel::Friendly;
    playback.app.app_config.config.store(Arc::new(config));
    playback.app.hls.proxy.update_config(&playback.app.app_config).await;
    playback.refresh(window(310, 312, "a")).await;
    playback.script_redirect_once(window(310, 312, "a"), Some(window(310, 313, "a")));
    let mut directive = HlsManifestAcceptanceDirective::none();
    directive.trigger = HlsManifestAcceptanceTrigger::Observe;
    playback.session.write().await.origin_refresh.next_fetch_allowed_at_ms = 0;
    refresh_playing_lease_with_directive(&playback.app, &playback.session, &playback.url, directive).await;
    playback.wait_for_ready(3).await;
    let (commits, generation) = {
        let state = playback.session.read().await;
        assert_eq!(state.origin_control.pinned_host.as_deref(), Some(HOST));
        assert!(state.origin_control.manifest_host_expiry_for_test(ALTERNATE_HOST).is_some());
        assert_eq!(state.origin_seq_highwater, Some(313));
        (state.origin_manifest_commits(), state.origin_control.acceptance_generation)
    };
    eprintln!("short_rollover_candidate host={ALTERNATE_HOST} confirmed=true pinned={HOST} anchor=313");

    playback.script_redirect_once(window(0, 1, "b"), None);
    playback.refresh_script().await;
    playback.assert_epoch(1, 4, 0, 1).await;
    playback.wait_for_ready(5).await;
    playback.assert_media_follows_commit("/b/", 0, commits, &[HOST, ALTERNATE_HOST]);
    {
        let state = playback.session.read().await;
        assert_eq!(state.origin_control.acceptance_generation, generation);
        assert!(state.origin_control.acceptance_episode.is_none());
    }
    let next = playback.manifest("warm").await;
    assert_boundary(&next, 4);
    assert_eq!(next.lines().filter(|line| *line == "#EXT-X-DISCONTINUITY").count(), 1);
    let first_new = usize::try_from(4 - manifest_media_sequence(&next)).unwrap();
    for uri in &media_uris(&next)[first_new..] {
        playback.assert_media(uri, 2).await;
    }
    playback.assert_media(media_uris(&playback.first)[0], 1).await;
    eprintln!("short_rollover_manifest next={next:?}");
}

#[tokio::test]
async fn second_rollover_requires_new_anchor_confirmation() {
    let playback = Playback::start(0).await;
    playback.refresh(window(310, 312, "a")).await;
    playback.refresh(window(0, 2, "b")).await;
    playback.wait_for_ready(5).await;
    let first_restart = playback.manifest("warm").await;
    let first_seq = manifest_media_sequence(&first_restart);
    let retained = media_uris(&first_restart)[usize::try_from(3 - first_seq).unwrap()];
    playback.refresh(window(0, 3, "c")).await;
    playback.assert_epoch(1, 3, 0, 2).await;
    playback.refresh(window(1, 3, "b")).await;
    playback.wait_for_ready(6).await;
    assert!(playback.session.read().await.origin_control.manifest_host_expiry_for_test(HOST).is_some());
    playback.refresh(window(0, 3, "c")).await;
    playback.wait_for_ready(10).await;
    playback.assert_epoch(2, 7, 0, 3).await;
    let next = playback.manifest("warm").await;
    let index = usize::try_from(7 - manifest_media_sequence(&next)).unwrap();
    assert_boundary(&next, 7);
    playback.assert_media(media_uris(&next)[index], 3).await;
    playback.assert_media(retained, 2).await;
    eprintln!(
        "second_rollover_manifests first={first_restart:?} next={next:?} digests={:02x?}/{:02x?}",
        Sha256::digest(media(2)),
        Sha256::digest(media(3))
    );
}

async fn idle_restart(delta_ms: u64, host_confirmed: bool) {
    let playback = Playback::start(1).await;
    if !host_confirmed {
        playback.app.hls.proxy.freeze_clock_for_test(super::super::super::current_time_millis() - delta_ms);
    }
    playback.refresh(window(310, 312, "a")).await;
    let (commits, expires) = {
        let state = playback.session.read().await;
        (state.origin_manifest_commits(), state.origin_control.manifest_host_expiry_for_test(HOST).unwrap())
    };
    let now = playback.idle_at(delta_ms).await;
    assert_eq!(now < expires, host_confirmed);
    let proxy = {
        let state = playback.session.read().await;
        if host_confirmed {
            assert!(now.saturating_sub(state.origin_refresh.last_success_at_ms.unwrap()) <= 2_000);
            assert!(state.last_rendered_manifest.as_ref().unwrap().valid_until_ms > now);
        }
        state.proxy_session_id.clone()
    };
    // Register the new lease without the canonical test helper aging origin freshness for it.
    super::super::prepare_pending_test_hls_access_lease(
        &playback.app,
        &proxy,
        &super::super::HlsAccessLeaseId("returning".into()),
    )
    .await;
    playback.script_same_host_redirect(window(6, 9, "b"));
    let next = playback.manifest("returning").await;
    playback.assert_epoch(1, 3, 6, 9).await;
    playback.assert_media_follows_commit("/b/", 0, commits, &[HOST, HOST]);
    assert_eq!(manifest_media_sequence(&next), 3);
    assert_boundary(&next, 3);
    assert_eq!(media_uris(&next).len(), 3, "one origin segment is held back by strip");
    assert_eq!(media_uris(&playback.manifest("returning").await), media_uris(&next));
    for uri in media_uris(&next) {
        playback.assert_media(uri, 2).await;
    }
    playback.assert_media(media_uris(&playback.first)[0], 1).await;
    assert!(playback.session.read().await.origin_control.manifest_host_expiry_for_test(HOST).is_none());
    eprintln!("idle_restart last_media_age_ms={delta_ms} confirmed_host={host_confirmed} next={next:?}");
}

#[tokio::test]
async fn idle_reentry_commits_all_missed_msns_before_media_with_valid_or_expired_host() {
    idle_restart(12_100, true).await;
    idle_restart(25_000, false).await;
}

#[tokio::test]
async fn idle_reentry_commits_the_only_short_window_from_an_alternate_host() {
    let playback = Playback::start(1).await;
    playback.refresh(window(310, 312, "a")).await;
    playback.idle_at(12_100).await;
    let (commits, generation) = {
        let state = playback.session.read().await;
        assert_eq!(state.origin_control.pinned_host.as_deref(), Some(HOST));
        assert!(state.origin_control.manifest_host_expiry_for_test(ALTERNATE_HOST).is_none());
        (state.origin_manifest_commits(), state.origin_control.acceptance_generation)
    };
    playback.script_redirect_once(window(6, 7, "b"), None);
    let next = playback.manifest("returning").await;
    playback.assert_epoch(1, 3, 6, 7).await;
    playback.assert_media_follows_commit("/b/", 0, commits, &[HOST, ALTERNATE_HOST]);
    {
        let state = playback.session.read().await;
        assert_eq!(state.origin_control.acceptance_generation, generation);
        assert!(state.origin_control.acceptance_episode.is_none());
    }
    eprintln!("short_idle_manifest next={next:?}");
    // Keep the published predecessor and both new segments when strip would leave too few entries.
    assert_eq!(manifest_media_sequence(&next), 2);
    assert_boundary(&next, 3);
    let uris = media_uris(&next);
    assert_eq!(uris.len(), 3);
    playback.assert_media(uris[0], 1).await;
    for uri in &uris[1..] {
        playback.assert_media(uri, 2).await;
    }
    playback.assert_media(media_uris(&playback.first)[0], 1).await;
}

#[tokio::test]
async fn idle_reentry_with_current_anchor_is_ordinary_continuity() {
    let playback = Playback::start(1).await;
    playback.idle_at(12_100).await;
    playback.script_same_host_redirect(window(310, 313, "a"));
    let next = playback.manifest("returning").await;
    assert!(playback.origin.requests.lock().unwrap().iter().any(|request| request.path == "/delivery/returning.m3u8"));
    playback.assert_epoch(0, 0, 310, 313).await;
    assert!(!next.contains("#EXT-X-DISCONTINUITY\n"));
    playback.assert_media(media_uris(&next)[0], 1).await;
}

#[tokio::test]
async fn idle_reentry_rejects_zero_window_replay_and_does_not_start_at_exactly_three_targets() {
    for delta_ms in [12_000, 12_100] {
        let playback = Playback::start(1).await;
        playback.refresh(window(310, 312, "a")).await;
        let expires = playback.session.read().await.origin_control.manifest_host_expiry_for_test(HOST);
        let checked_now = playback.idle_at(delta_ms).await;
        let candidate = if delta_ms == 12_000 {
            playback.origin.reject_new_media.store(true, Ordering::Release);
            window(6, 9, "b")
        } else {
            window(0, 2, "b").replace("/b/0.ts", "/a/312.ts")
        };
        playback.script(candidate);
        let response = canonical_response_for_lease(&playback.app, &playback.input, &playback.url, "returning").await;
        let _ = response_body(response).await;
        playback.assert_epoch(0, 0, 310, 312).await;
        assert_eq!(playback.session.read().await.origin_control.manifest_host_expiry_for_test(HOST), expires);
        let requests = playback.origin.requests.lock().unwrap();
        assert!(requests.iter().any(|request| request.path == "/index.m3u8"));
        assert!(requests.iter().all(|request| request.epoch == 0));
        if delta_ms > 12_000 {
            assert!(requests.iter().all(|request| !request.path.starts_with("/b/")));
        }
        eprintln!("idle_counterprobe age_ms={delta_ms} checked_at_ms={checked_now} requests={requests:?}");
    }
}

#[tokio::test]
async fn gc_removal_starts_a_cold_session_without_previous_proxy_slots_or_host_proofs() {
    let playback = Playback::start(0).await;
    playback.refresh(window(310, 312, "a")).await;
    assert!(playback.session.read().await.origin_control.manifest_host_expiry_for_test(HOST).is_some());
    let manager = &playback.app.hls.proxy;
    manager.remove_access_lease_for_test(&super::super::HlsAccessLeaseId("warm".into())).await;
    let future = manager.now_ms() + 3_600_000;
    manager
        .sync_all_session_access_leases_and_detach_if_needed(
            &playback.app.active_users,
            &playback.app.active_provider,
            future,
        )
        .await;
    let report = manager.run_garbage_collection_once(future).await.unwrap();
    assert_eq!(report.sessions_deleted, 1);
    let key = super::super::super::build_hls_origin_source(&playback.input, "12345").session_key();
    assert!(manager.sessions().get_by_key(&key).await.is_none());
    *playback.origin.session.lock().unwrap() = None;
    playback.script(window(6, 9, "b"));
    let fresh = playback.manifest("returning").await;
    assert_eq!(manifest_media_sequence(&fresh), 0);
    assert!(!fresh.contains("#EXT-X-DISCONTINUITY\n"));
    playback.assert_media(media_uris(&fresh)[0], 2).await;
    let new = manager.sessions().get_by_key(&key).await.unwrap();
    assert!(!Arc::ptr_eq(&new, &playback.session));
    let state = new.read().await;
    assert_eq!(state.origin_epoch, 0);
    assert_eq!(state.segments[&0].origin_key.host_local_sequence, 6);
    assert!(state.origin_control.manifest_host_expiry_for_test(HOST).is_none());
    eprintln!("cold_after_gc removed=1 epoch=0 first_proxy_seq=0 first_origin_msn=6");
}
