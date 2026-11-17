use super::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

fn entries(base: &str, body: &str) -> BTreeMap<u64, String> {
    let first: u64 =
        body.lines().find_map(|line| line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:")).unwrap().parse().unwrap();
    body.lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .enumerate()
        .map(|(offset, uri)| {
            let url = reqwest::Url::parse(base).unwrap().join(uri).unwrap();
            (first + u64::try_from(offset).unwrap(), url.to_string())
        })
        .collect()
}

async fn host_change(mode: &str) {
    let mut fixture = Fixture::start_with_mode(mode, OriginScenario::Moving).await;
    let client = reqwest::Client::builder().timeout(Duration::from_secs(10)).build().unwrap();
    let entry = fixture.playback(&client).await;
    let master = client.get(&entry).send().await.unwrap();
    assert_eq!(master.status(), 200, "real entry master: {}", fixture.log());
    let variant = resolve_media(&entry, &master.text().await.unwrap(), 0);
    let first = client.get(&variant).send().await.unwrap();
    assert_eq!(first.status(), 200, "first media playlist: {}", fixture.log());
    let first = first.text().await.unwrap();
    let old = entries(&variant, &first);
    let digest = Sha256::digest(BODY);
    for uri in old.values() {
        let response = client.get(uri).send().await.unwrap();
        assert_eq!(response.status(), 200, "initial media: {}", fixture.log());
        assert_eq!(Sha256::digest(fixture.media_bytes(response).await), digest);
    }
    let before = fixture.requests.lock().unwrap().clone();
    let before_log = fixture.log().len();
    fixture.head.store(101, Ordering::Release);
    fixture.mirror.store(true, Ordering::Release);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
    let second = loop {
        let response = client.get(&variant).send().await.unwrap();
        assert_eq!(response.status(), 200, "active lease refresh: {}", fixture.log());
        let body = response.text().await.unwrap();
        if entries(&variant, &body).contains_key(&6) {
            break body;
        }
        assert!(tokio::time::Instant::now() < deadline, "mirror never advanced the playlist: {}", fixture.log());
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert!(!second.lines().any(|line| line == "#EXT-X-DISCONTINUITY"));
    assert!(!second.contains("#EXT-X-ENDLIST"));
    assert_eq!(
        first.lines().find(|line| line.starts_with("#EXT-X-DISCONTINUITY-SEQUENCE:")),
        second.lines().find(|line| line.starts_with("#EXT-X-DISCONTINUITY-SEQUENCE:"))
    );
    assert!(!first.contains("#EXT-X-BYTERANGE") && !second.contains("#EXT-X-BYTERANGE"));
    let next = entries(&variant, &second);
    let overlap = old.keys().filter(|seq| next.contains_key(seq)).copied().collect::<Vec<_>>();
    assert_ne!(overlap, [] as [u64; 0]);
    for seq in overlap {
        assert_eq!(old[&seq], next[&seq], "overlapping client MSN keeps its complete URI");
        let response = client.get(&old[&seq]).header("Range", "bytes=2-5").send().await.unwrap();
        assert_eq!(response.status(), 206);
        assert_eq!(response.headers()["content-range"], format!("bytes 2-5/{}", BODY.len()));
        assert_eq!(response.headers()["content-length"], "4");
        assert_eq!(response.bytes().await.unwrap().as_ref(), &BODY[2..6]);
        let full = client.get(&old[&seq]).send().await.unwrap();
        assert_eq!(full.status(), 200);
        assert_eq!(Sha256::digest(full.bytes().await.unwrap()), digest);
        let path = format!("/{}.ts", seq + 100);
        assert_eq!(fixture.requests.lock().unwrap()[&path], before[&path], "READY overlap never reaches origin again");
    }
    assert!(fixture.mirror_requests.load(Ordering::Acquire) > 0, "second local manifest host was requested");
    let log = fixture.log();
    let switched_log = &log[before_log..];
    assert!(
        !switched_log.contains("HLS manifest recovery started")
            && !switched_log.contains("HLS manifest acceptance full burst started"),
        "delivery host change does not open recovery"
    );
    assert!(
        !log.contains("HLS origin manifest rejected"),
        "unchanged occurrence bindings need no conflict recovery: {log}"
    );
    assert!(!log.contains("decision=qualified-binding"), "transport-only changes need no equality downloads");
    eprintln!(
        "mode={mode} first={first:?} second={second:?} origin_requests={:?} digest={digest:02x?}",
        fixture.requests.lock().unwrap()
    );
    fixture.process.kill().await.unwrap();
}

#[tokio::test]
async fn conservative_playback_keeps_ready_overlap_across_manifest_hosts() { host_change("conservative").await; }
#[tokio::test]
async fn first_ready_playback_keeps_ready_overlap_across_manifest_hosts() { host_change("first_ready").await; }
#[tokio::test]
async fn progressive_playback_keeps_ready_overlap_across_manifest_hosts() { host_change("progressive").await; }
#[tokio::test]
async fn optimistic_playback_keeps_ready_overlap_across_manifest_hosts() { host_change("optimistic").await; }

async fn wait_for_rotating_window(
    fixture: &Fixture,
    client: &reqwest::Client,
    variant: &str,
    last_proxy_seq: u64,
) -> String {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(18);
    loop {
        let response = client.get(variant).send().await.unwrap();
        let status = response.status();
        let body = response.text().await.unwrap();
        if status == 200 && entries(variant, &body).last_key_value().is_some_and(|(&last, _)| last >= last_proxy_seq) {
            return body;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "live window stalled before proxy slot {last_proxy_seq}: status={status}; origin={:?}; {}",
            fixture.requests.lock().unwrap(),
            fixture
                .log()
                .lines()
                .filter(|line| line.contains("origin-playlist-scope-changed"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
}

#[tokio::test]
async fn optimistic_playback_keeps_segment_490_across_rotating_manifest_redirects() {
    let mut fixture = Fixture::start_with_mode("optimistic", OriginScenario::Rotating).await;
    let client = reqwest::Client::builder().timeout(Duration::from_secs(8)).build().unwrap();
    let entry = fixture.playback(&client).await;
    let master = client.get(&entry).send().await.unwrap().error_for_status().unwrap().text().await.unwrap();
    let variant = resolve_media(&entry, &master, 0);
    let initial = wait_for_rotating_window(&fixture, &client, &variant, 2).await;
    assert_eq!(entries(&variant, &initial).keys().copied().collect::<Vec<_>>(), [0, 1, 2]);
    let activation = client.get(resolve_media(&variant, &initial, 0)).send().await.unwrap().error_for_status().unwrap();
    assert_eq!(fixture.media_bytes(activation).await.as_ref(), rotating_media(485));

    let first = wait_for_rotating_window(&fixture, &client, &variant, 5).await;
    let first_window = entries(&variant, &first);
    let retained_uri = &first_window[&5];
    let digest = Sha256::digest(rotating_media(490));
    let first_media = client.get(retained_uri).send().await.unwrap().error_for_status().unwrap();
    assert_eq!(first_media.content_length(), Some(u64::try_from(rotating_media(490).len()).unwrap()));
    assert_eq!(Sha256::digest(fixture.media_bytes(first_media).await), digest);
    assert_eq!(fixture.requests.lock().unwrap().get("/490.ts?media=fixture"), Some(&1));
    eprintln!("initial_ready origin_msn=490 proxy_slot=5 origin_gets=1 sha256={digest:02x?}");
    let before_rotation = fixture.log().len();

    let mut playlists = vec![(485, first)];
    for (head, last_proxy_seq) in [(487, 7), (488, 8), (489, 9)] {
        fixture.head.store(head, Ordering::Release);
        let body = wait_for_rotating_window(&fixture, &client, &variant, last_proxy_seq).await;
        let slots: Vec<_> = entries(&variant, &body).keys().copied().collect();
        eprintln!("window origin_head={head} slots={slots:?} origin_gets={:?}", fixture.requests.lock().unwrap());
        playlists.push((head, body));
    }

    let origin_gets = fixture.requests.lock().unwrap().clone();
    let log = fixture.log();
    let rotation_log = &log[before_rotation..];
    for event in rotation_log.lines().filter(|line| {
        line.contains("origin-playlist-scope-changed")
            || line.contains("HLS manifest acceptance full burst started")
            || line.contains("HLS origin epoch committed")
    }) {
        eprintln!("rotation_event {}", event.split("HLS ").nth(1).unwrap_or(event));
    }
    assert_eq!(origin_gets.get("/490.ts?media=fixture"), Some(&1), "490 never re-enters the origin work queue");
    assert!(!rotation_log.contains("HLS manifest recovery started"));
    assert!(!rotation_log.contains("HLS manifest acceptance full burst started"));
    assert!(!rotation_log.contains("HLS origin epoch committed"));
    assert!(!rotation_log.contains("HLS media sequence rollover detected"));
    assert!(!rotation_log.contains("origin-playlist-scope-changed"));
    for (head, body) in playlists {
        let window = entries(&variant, &body);
        let first_slot = u64::try_from(head - 485).unwrap();
        assert_eq!(window.keys().copied().collect::<Vec<_>>(), (first_slot..first_slot + 6).collect::<Vec<_>>());
        assert_eq!(&window[&5], retained_uri);
        assert!(body.contains("#EXT-X-DISCONTINUITY-SEQUENCE:0"));
        assert!(!body.lines().any(|line| matches!(line, "#EXT-X-DISCONTINUITY" | "#EXT-X-ENDLIST")));
        let mut slots = window.keys();
        let proxy = body
            .lines()
            .map(|line| {
                if line.is_empty() || line.starts_with('#') {
                    line.to_owned()
                } else {
                    format!("/proxy/lease-a/{:06}.ts", slots.next().unwrap())
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        eprintln!("playlist_pair origin_head={head} origin={:?} proxy={proxy:?}", rotating_manifest(head));
        for (offset, uri) in window.values().enumerate() {
            let msn = head + offset;
            let media = client.get(uri).send().await.unwrap().error_for_status().unwrap();
            assert_eq!(Sha256::digest(fixture.media_bytes(media).await), Sha256::digest(rotating_media(msn)));
        }
    }
    let after = fixture.requests.lock().unwrap().clone();
    for msn in 485..=494 {
        assert_eq!(after.get(&format!("/{msn}.ts?media=fixture")), Some(&1), "origin MSN {msn}");
    }
    for (head, host) in [(485, 'A'), (487, 'B'), (488, 'B'), (489, 'A')] {
        assert!(after.keys().any(|path| path.starts_with(&format!("{host}/delivery/{head}/"))));
    }
    fixture.process.kill().await.unwrap();
}
