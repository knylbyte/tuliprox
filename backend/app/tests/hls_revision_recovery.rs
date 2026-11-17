//! Real Tuliprox process + a streaming origin. No application HTTP client is stubbed.
use std::{
    fmt::Write as _,
    path::PathBuf,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    sync::watch,
};

const BODY: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../test/fixtures/hls/channel_unavailable.ts"));

#[derive(Clone, Copy, PartialEq, Eq)]
enum OriginScenario {
    Interrupted,
    Moving,
    Rotating,
}

struct Fixture {
    directory: tempfile::TempDir,
    base: String,
    process: tokio::process::Child,
    origin: tokio::task::JoinHandle<()>,
    release: watch::Sender<bool>,
    release_retry: watch::Sender<bool>,
    retry_started: watch::Receiver<bool>,
    attempts: Arc<AtomicUsize>,
    head: Arc<AtomicUsize>,
    mirror: Arc<AtomicBool>,
    requests: Arc<std::sync::Mutex<std::collections::BTreeMap<String, usize>>>,
    mirror_requests: Arc<AtomicUsize>,
    mirror_origin: tokio::task::JoinHandle<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.process.start_kill();
        self.origin.abort();
        self.mirror_origin.abort();
    }
}

impl Fixture {
    async fn start() -> Self { Self::start_with_mode("optimistic", OriginScenario::Interrupted).await }

    #[allow(clippy::too_many_lines, reason = "Keeps the isolated process and streaming origin fixture together.")]
    async fn start_with_mode(mode: &str, scenario: OriginScenario) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin_base = format!("http://{}", listener.local_addr().unwrap());
        let rotating = scenario == OriginScenario::Rotating;
        let moving = scenario != OriginScenario::Interrupted;
        let catalog = format!(
            "#EXTM3U\n#EXTINF:-1 tvg-id=\"recovery\" group-title=\"Test\",Recovery Channel\n{origin_base}/index.m3u8\n"
        );
        let head = Arc::new(AtomicUsize::new(if rotating { 485 } else { 100 }));
        let mirror = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(std::sync::Mutex::new(std::collections::BTreeMap::new()));
        let mirror_requests = Arc::new(AtomicUsize::new(0));
        let mirror_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mirror_url = format!("http://localhost:{}/index.m3u8", mirror_listener.local_addr().unwrap().port());
        let mirror_head = Arc::clone(&head);
        let media_base = origin_base.clone();
        let mirror_counted = Arc::clone(&mirror_requests);
        let mirror_router = axum::Router::new().fallback(move || {
            mirror_counted.fetch_add(1, Ordering::AcqRel);
            std::future::ready(origin_manifest(&media_base, mirror_head.load(Ordering::Acquire)))
        });
        let mirror_origin = tokio::spawn(async move {
            axum::serve(mirror_listener, mirror_router).await.unwrap();
        });
        let moving_head = Arc::clone(&head);
        let switched = Arc::clone(&mirror);
        let counted_requests = Arc::clone(&requests);
        let attempts = Arc::new(AtomicUsize::new(0));
        let (release, released) = watch::channel(moving);
        let (release_retry, retry_released) = watch::channel(false);
        let (retry_started, started_retry) = watch::channel(false);
        let counted = attempts.clone();
        let origin_manifest_base = origin_base.clone();
        let origin_port = listener.local_addr().unwrap().port();
        let redirect_serial = Arc::new(AtomicUsize::new(0));
        let origin = tokio::spawn(async move {
            let mut children = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (socket, _) = accepted.unwrap();
                        let catalog = catalog.clone();
                        let head = moving_head.load(Ordering::Acquire);
                        let manifest = origin_manifest(&origin_manifest_base, head);
                        let mirror_url = mirror_url.clone(); let switched = Arc::clone(&switched);
                        let requests = Arc::clone(&counted_requests);
                        let attempts = counted.clone(); let mut released = released.clone();
                        let mut retry_released = retry_released.clone();
                        let retry_started = retry_started.clone();
                        let redirect_serial = Arc::clone(&redirect_serial);
                        children.spawn(async move {
                            let mut reader = BufReader::new(socket);
                            let mut request = String::new();
                            reader.read_line(&mut request).await.unwrap();
                            let path = request.split_whitespace().nth(1).unwrap().to_owned();
                            loop {
                                let mut line = String::new(); reader.read_line(&mut line).await.unwrap();
                                if line == "\r\n" || line.is_empty() { break; }
                            }
                            let mut socket = reader.into_inner();
                            if rotating && path == "/index.m3u8" {
                                let host = if matches!(head, 487 | 488) { "localhost" } else { "127.0.0.1" };
                                let serial = redirect_serial.fetch_add(1, Ordering::AcqRel);
                                let location = format!("http://{host}:{origin_port}/delivery/{head}/{serial}.m3u8?turn={serial}");
                                let headers = format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                                let _ = socket.write_all(headers.as_bytes()).await;
                                return;
                            }
                            if rotating && path.starts_with("/delivery/") {
                                let head = path.split('/').nth(2).unwrap().parse().unwrap();
                                let host = if matches!(head, 487 | 488) { "B" } else { "A" };
                                *requests.lock().unwrap().entry(format!("{host}{path}")).or_insert(0) += 1;
                                let body = rotating_manifest(head);
                                let headers = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/vnd.apple.mpegurl\r\nConnection: close\r\n\r\n", body.len());
                                if socket.write_all(headers.as_bytes()).await.is_ok() {
                                    let _ = socket.write_all(body.as_bytes()).await;
                                }
                                return;
                            }
                            if path == "/index.m3u8" && switched.load(Ordering::Acquire) {
                                let headers = format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: {mirror_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                                socket.write_all(headers.as_bytes()).await.unwrap();
                                return;
                            }
                            if path == "/catalog.m3u" || path == "/index.m3u8" {
                                let body = if path == "/index.m3u8" { manifest } else { catalog };
                                let headers = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/vnd.apple.mpegurl\r\nConnection: close\r\n\r\n", body.len());
                                socket.write_all(headers.as_bytes()).await.unwrap();
                                socket.write_all(body.as_bytes()).await.unwrap();
                                return;
                            }
                            // The accelerated entry is 103,104,105. Inject the
                            // failure into its first visible successor.
                            *requests.lock().unwrap().entry(path.clone()).or_insert(0) += 1;
                            let rotating_body = rotating.then(|| {
                                let seq = path.strip_prefix('/').unwrap().split_once(".ts").unwrap().0.parse().unwrap();
                                rotating_media(seq)
                            });
                            let body = rotating_body.as_deref().unwrap_or(BODY);
                            let failed_attempt = !moving && path == "/104.ts" && attempts.fetch_add(1, Ordering::AcqRel) == 0;
                            let headers = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: video/mp2t\r\nConnection: close\r\n\r\n", body.len());
                            socket.write_all(headers.as_bytes()).await.unwrap();
                            if !moving && path == "/104.ts" && !failed_attempt {
                                retry_started.send_replace(true);
                                while !*retry_released.borrow_and_update() {
                                    if retry_released.changed().await.is_err() { return; }
                                }
                            }
                            let prefix = if failed_attempt { b"stale-prefix".as_slice() } else { &body[..188] };
                            socket.write_all(prefix).await.unwrap();
                            while !*released.borrow_and_update() {
                                if released.changed().await.is_err() { return; }
                            }
                            if failed_attempt {
                                // The client controls when this HTTP 200 ends short of its declared length.
                                return;
                            }
                            let _ = socket.write_all(&body[188..]).await;
                        });
                    }
                    _ = children.join_next(), if !children.is_empty() => {}
                }
            }
        });
        let reserved = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = reserved.local_addr().unwrap().port();
        drop(reserved);
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        for name in ["web", "storage", "backup"] {
            std::fs::create_dir(root.join(name)).unwrap();
        }
        let strip = if rotating { 3 } else { 0 };
        let config = format!("api:\n  host: 127.0.0.1\n  port: {port}\n  web_root: '{}'\nworking_dir: '{}'\nbackup_dir: '{}'\nupdate_on_boot: true\nweb_ui:\n  enabled: false\nlog:\n  log_level: debug\nreverse_proxy:\n  rewrite_secret: 0123456789abcdef0123456789abcdef\n  stream:\n    hls_session_ttl_secs: 120\n  hls_cache:\n    cache_path: '{}'\n    strip:\n      mode: segments\n      value: {strip}\n    origin_segment_timeout_ms: 4000\n    max_concurrent_segment_fetches_per_session: 3\n    initial_manifest_wait_timeout_secs: 3\n    startup:\n      mode: {mode}\n    segment_repair:\n      max_level: off\n", root.join("web").display(), root.join("storage").display(), root.join("backup").display(), root.join("cache").display());
        let sources = format!("inputs:\n  - name: recovery-origin\n    type: m3u\n    url: {origin_base}/catalog.m3u\n    max_connections: 8\nsources:\n  - inputs: [recovery-origin]\n    targets:\n      - name: recovery\n        output:\n          - type: m3u\n        options:\n          share_live_streams:\n            hls: true\n");
        let users = format!("server:\n  - name: local\n    protocol: http\n    host: 127.0.0.1\n    port: '{port}'\n    timezone: UTC\n    message: Recovery test\nuser:\n  - target: recovery\n    credentials:\n      - username: viewer\n        password: test-password\n        proxy: reverse\n        server: local\n        status: Active\n        max_connections: 1\n");
        for (name, body) in [("config.yml", config), ("source.yml", sources), ("api-proxy.yml", users)] {
            std::fs::write(root.join(name), body).unwrap();
        }
        let log = std::fs::File::create(root.join("sut.log")).unwrap();
        let process = tokio::process::Command::new(env!("CARGO_BIN_EXE_tuliprox"))
            .args(["-s", "-c", "config.yml", "-i", "source.yml", "-a", "api-proxy.yml"])
            .current_dir(root)
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        Self {
            directory,
            base: format!("http://127.0.0.1:{port}"),
            process,
            origin,
            release,
            release_retry,
            retry_started: started_retry,
            attempts,
            head,
            mirror,
            requests,
            mirror_requests,
            mirror_origin,
        }
    }

    fn log(&self) -> String { std::fs::read_to_string(self.directory.path().join("sut.log")).unwrap() }

    async fn media_bytes(&self, response: reqwest::Response) -> bytes::Bytes {
        response.bytes().await.unwrap_or_else(|error| panic!("media body failed: {error:?}\n{}", self.log()))
    }

    async fn playback(&mut self, client: &reqwest::Client) -> String {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            assert!(self.process.try_wait().unwrap().is_none(), "SUT exited: {}", self.log());
            if let Ok(response) =
                client.get(format!("{}/m3u?username=viewer&password=test-password", self.base)).send().await
            {
                if let Ok(body) = response.text().await {
                    if let Some(url) = body.lines().find(|line| line.starts_with("http")) {
                        return url.to_owned();
                    }
                }
            }
            assert!(tokio::time::Instant::now() < deadline, "catalog unavailable: {}", self.log());
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
}

fn origin_manifest(base: &str, head: usize) -> String {
    let mut manifest = format!("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:{head}\n");
    for seq in head..head + 6 {
        writeln!(manifest, "#EXTINF:4,\n{base}/{seq}.ts").unwrap();
    }
    manifest
}

fn rotating_manifest(head: usize) -> String {
    let mut manifest = format!("#EXTM3U\n#EXT-X-TARGETDURATION:11\n#EXT-X-MEDIA-SEQUENCE:{head}\n");
    for seq in head..head + 6 {
        writeln!(manifest, "#EXTINF:11,\n/{seq}.ts?media=fixture").unwrap();
    }
    manifest
}

fn rotating_media(seq: usize) -> Vec<u8> {
    let mut bytes = BODY.to_vec();
    bytes.extend_from_slice(&[0x47, 0x1f, 0xff, 0x10]);
    bytes.extend_from_slice(&[seq.to_le_bytes()[0]; 184]);
    bytes
}
#[path = "hls_revision_recovery/segment_contract.rs"]
mod segment_contract;

fn resolve_media(base: &str, body: &str, index: usize) -> String {
    let uri = body.lines().filter(|line| !line.is_empty() && !line.starts_with('#')).nth(index).expect("media URI");
    reqwest::Url::parse(base).unwrap().join(uri).unwrap().to_string()
}

#[tokio::test]
async fn incomplete_origin_200_recovers_same_segment_uri_without_a_playlist_refresh() {
    let mut fixture = Fixture::start().await;
    let client = reqwest::Client::builder().timeout(Duration::from_secs(10)).build().unwrap();
    let entry = fixture.playback(&client).await;
    let master = client.get(&entry).send().await.unwrap();
    assert!(master.status().is_success(), "entry: {}", fixture.log());
    let master_body = master.text().await.unwrap();
    let variant = resolve_media(&entry, &master_body, 0);
    let playlist = client.get(&variant).send().await.unwrap();
    assert_eq!(playlist.status(), 200, "startup: {}", fixture.log());
    let playlist = playlist.text().await.unwrap();
    assert!(playlist.contains("#EXT-X-MEDIA-SEQUENCE:3\n"), "entry follows the current live edge");
    let segment = resolve_media(&variant, &playlist, 1);
    assert!(fixture.attempts.load(Ordering::Acquire) <= 1, "no retry before releasing the incomplete response");
    // Exercise unexposed successor recovery after consuming this lease's one
    // progressive request. A successor selected as the first GET may now pipe
    // its prefix and therefore cannot silently switch bytes after a body drop.
    let head = client.get(resolve_media(&variant, &playlist, 0)).send().await.unwrap();
    assert_eq!(head.status(), 200, "head must stream before EOF: {}", fixture.log());
    fixture.release.send_replace(true);
    assert_eq!(fixture.media_bytes(head).await.as_ref(), BODY);
    let waiting = client.get(&segment).send();
    tokio::pin!(waiting);
    let mut retry_started = fixture.retry_started.clone();
    // The request may wait through the failed attempt and its backoff. Release
    // the retry at a server barrier instead of expecting the old 300ms startup cap.
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::select! {
            response = &mut waiting => panic!("GET finished before the retry origin was released: {response:?}: {}", fixture.log()),
            started = retry_started.wait_for(|started| *started) => { started.unwrap(); }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("retry origin was never requested: {}", fixture.log()));
    fixture.release_retry.send_replace(true);
    let recovered = waiting.await.unwrap();
    assert_eq!(recovered.status(), 200, "same GET must recover after the origin retry: {}", fixture.log());
    assert_eq!(recovered.bytes().await.unwrap().as_ref(), BODY);
    assert!(fixture.attempts.load(Ordering::Acquire) >= 2);
    let refreshed = client.get(&variant).send().await.unwrap();
    assert_eq!(refreshed.status(), 200, "recovered binding must not conflict: {}", fixture.log());
    assert!(refreshed.text().await.unwrap().contains(reqwest::Url::parse(&segment).unwrap().path()));
    for _ in 0..3 {
        let replay = client.get(&segment).send().await.unwrap();
        assert_eq!(replay.status(), 200);
        assert_eq!(replay.bytes().await.unwrap().as_ref(), BODY);
    }
    assert_eq!(fixture.attempts.load(Ordering::Acquire), 2, "repaired GETs reuse the committed revision");
    assert!(fixture.log().contains("outcome=replacement_bound"));
    // Retain run evidence only when requested; test configuration and credentials are synthetic.
    if let Ok(destination) = std::env::var("TULIPROX_HLS_RECOVERY_LOG") {
        std::fs::copy(fixture.directory.path().join("sut.log"), PathBuf::from(destination)).unwrap();
    }
    fixture.process.kill().await.unwrap();
}

#[tokio::test]
async fn first_requested_successor_streams_before_origin_eof() {
    let mut fixture = Fixture::start().await;
    let client = reqwest::Client::builder().timeout(Duration::from_secs(10)).build().unwrap();
    let entry = fixture.playback(&client).await;
    let master = client.get(&entry).send().await.unwrap().text().await.unwrap();
    let variant = resolve_media(&entry, &master, 0);
    let playlist = client.get(&variant).send().await.unwrap();
    assert_eq!(playlist.status(), 200, "startup: {}", fixture.log());
    let playlist = playlist.text().await.unwrap();
    // Skip both the playlist head and the deliberately failing first successor.
    let segment = resolve_media(&variant, &playlist, 2);
    let mut response = tokio::time::timeout(Duration::from_secs(2), client.get(&segment).send())
        .await
        .expect("first successor GET must not wait for origin EOF")
        .unwrap();
    assert_eq!(response.status(), 200, "successor: {}", fixture.log());
    assert!(!response.headers().contains_key("content-length"));
    let prefix = response.chunk().await.unwrap().expect("progressive prefix");
    assert_eq!(prefix.as_ref(), &BODY[..188]);
    assert!(!*fixture.release.borrow(), "origin EOF is still held");
    fixture.release.send_replace(true);
    fixture.release_retry.send_replace(true);
    let mut bytes = prefix.to_vec();
    while let Some(chunk) =
        response.chunk().await.unwrap_or_else(|error| panic!("progressive body failed: {error:?}\n{}", fixture.log()))
    {
        bytes.extend_from_slice(&chunk);
    }
    assert_eq!(bytes, BODY);
    let replay = client.get(&segment).send().await.unwrap();
    assert_eq!(replay.status(), 200);
    assert_eq!(replay.bytes().await.unwrap().as_ref(), BODY);
    if let Ok(destination) = std::env::var("TULIPROX_HLS_FIRST_REQUEST_LOG") {
        std::fs::copy(fixture.directory.path().join("sut.log"), PathBuf::from(destination)).unwrap();
    }
    fixture.process.kill().await.unwrap();
}

#[tokio::test]
async fn exposed_failure_allows_a_new_player_with_user_connection_limit_one() {
    let mut fixture = Fixture::start().await;
    let client = reqwest::Client::builder().timeout(Duration::from_secs(10)).build().unwrap();
    let entry = fixture.playback(&client).await;
    let master = client.get(&entry).send().await.unwrap().error_for_status().unwrap().text().await.unwrap();
    let variant = resolve_media(&entry, &master, 0);
    let playlist = client.get(&variant).send().await.unwrap().error_for_status().unwrap().text().await.unwrap();
    let segment = resolve_media(&variant, &playlist, 1);
    // The first full GET deliberately selects the successor, exercising the
    // integrated piping policy and exposing this attempt's prefix.
    let exposed = client.get(&segment).send().await.unwrap();
    assert_eq!(exposed.status(), 200, "first successor GET must pipe: {}", fixture.log());
    fixture.release.send_replace(true);
    assert!(exposed.bytes().await.is_err(), "exposed truncated body must not become successful EOF");
    let ended = client.get(&segment).send().await.unwrap();
    assert_eq!(ended.status(), 404, "old lease must end: {}", fixture.log());
    fixture.release_retry.send_replace(true);
    let new_player =
        reqwest::Client::builder().user_agent("new-reentry-player").timeout(Duration::from_secs(10)).build().unwrap();
    let master = new_player.get(&entry).send().await.unwrap();
    assert_eq!(master.status(), 200, "counted user reservation blocked reentry: {}", fixture.log());
    let body = master.text().await.unwrap();
    assert!(body.contains("#EXT-X-STREAM-INF"), "new entry must reach live playback: {body}");
    let new_variant = resolve_media(&entry, &body, 0);
    assert_ne!(new_variant, variant);
    let previous_session = reqwest::Url::parse(&variant).unwrap().path_segments().unwrap().nth(3).unwrap().to_owned();
    let current_session =
        reqwest::Url::parse(&new_variant).unwrap().path_segments().unwrap().nth(3).unwrap().to_owned();
    assert_eq!(previous_session, current_session, "the shared origin session stays alive");
    // Reentry releases the user slot before the origin retry finishes. The
    // pending lease may still wait for that retry's publishable media.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let playlist = loop {
        let response = new_player.get(&new_variant).send().await.unwrap();
        if response.status() == 200 {
            break response;
        }
        // Before its first publication, a lease can use the unavailable-channel
        // fallback (404 without a configured custom stream) or a temporary 503.
        assert!(
            matches!(response.status().as_u16(), 404 | 503),
            "new live playlist rejected with {}: {}",
            response.status(),
            fixture.log()
        );
        assert!(tokio::time::Instant::now() < deadline, "new live playlist never recovered: {}", fixture.log());
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    let new_segment = resolve_media(&new_variant, &playlist.text().await.unwrap(), 1);
    let repaired = new_player.get(new_segment).send().await.unwrap();
    assert_eq!(repaired.status(), 200, "new player cannot use the successful revision: {}", fixture.log());
    assert_eq!(repaired.bytes().await.unwrap().as_ref(), BODY);
    assert_eq!(client.get(&segment).send().await.unwrap().status(), 404);
    fixture.process.kill().await.unwrap();
}
