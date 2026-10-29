//! Native Finding 2 matrix: four startup modes, provider limits 1/3, four lifecycle cases.
//!
//! Every case starts a real Tuliprox process and a loopback origin. At limit 3,
//! two continuously drained TS streams occupy the other slots. An independent
//! HLS owner competes around an actual transfer timeout and jittered backoff.
//! No clock is paused or advanced. Media assertions validate framed origin bytes.
//!
//! Build with `cargo +stable build --profile dev-slim -p tuliprox` and run from
//! the repository root with the built binary selected at runtime:
//!
//! ```sh
//! TMPDIR=/tmp TULIPROX_TESTKIT_SUT_BINARY="$PWD/target/dev-slim/tuliprox" \
//!   ./bin/test.sh --profile dev-slim -p tuliprox-testkit \
//!   --test hls_provider_reservation_process -- --ignored --nocapture --test-threads=2
//! ```
//!
//! Generated configuration, logs and processes belong to one temporary run.
//! Reports contain generic labels/counters; failure log excerpts remove locations.
//! CVR is disabled and the configured failure status is 503. Terminal failures
//! must free capacity and stay terminal when the same access URL is polled again.
#[path = "support/reservation_fixture.rs"]
mod fixture;

use fixture::{client, Case, Fixture, COMPETITOR, SUBJECT};
use reqwest::{StatusCode, Url};
use std::time::{Duration, Instant};
use tuliprox_testkit::{
    frame::{FrameDecoder, FrameValidator},
    hls_probe::StartupMode,
    TestkitError,
};

async fn wait_for(check: impl Fn() -> bool, timeout: Duration, label: &str) {
    let deadline = Instant::now() + timeout;
    while !check() {
        assert!(Instant::now() < deadline, "deadline exceeded: {label}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn media_urls(base: &Url, manifest: &str) -> Vec<Url> {
    manifest
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| base.join(line).expect("local fixture media URI"))
        .collect()
}

async fn master(fixture: &Fixture, player: &reqwest::Client, user: &str, marker: u32) -> Result<Url, TestkitError> {
    let response = player.get(fixture.entry(user, marker)).send().await?;
    assert_eq!(response.status(), StatusCode::OK, "local master response");
    let base = response.url().clone();
    let body = response.text().await?;
    assert!(body.contains("#EXT-X-STREAM-INF"), "normal master must be returned");
    Ok(media_urls(&base, &body).into_iter().next().expect("session URL"))
}

async fn verify_media(
    fixture: &Fixture,
    player: &reqwest::Client,
    variant: &Url,
    body: &str,
    marker: u32,
) -> Result<(), TestkitError> {
    let urls = media_urls(variant, body);
    assert!(!body.contains("#EXT-X-ENDLIST"), "live media must not be a custom-video fallback");
    assert_eq!(urls.len(), 3, "all three successful origin objects must be published");
    let mut validator = FrameValidator::new(&fixture.run, marker);
    let mut frames = 0;
    for url in &urls {
        let response = player.get(url.clone()).send().await?;
        assert_eq!(response.status(), StatusCode::OK, "media delivery");
        let bytes = response.bytes().await?;
        for frame in FrameDecoder::default().push(&bytes)? {
            validator.validate(&frame)?;
            frames += 1;
        }
    }
    assert_eq!(frames, 6, "complete synthetic origin data");
    let first = player.get(urls[0].clone()).send().await?.bytes().await?;
    let retry = player.get(urls[0].clone()).send().await?.bytes().await?;
    assert_eq!(first, retry, "retry identity");
    let range = player.get(urls[0].clone()).header("Range", "bytes=0-63").send().await?;
    assert_eq!(range.status(), StatusCode::PARTIAL_CONTENT, "range response");
    assert_eq!(range.bytes().await?.as_ref(), &first[..64], "range identity");
    Ok(())
}

async fn competitor_probe(fixture: &Fixture, identity: usize) -> Result<StatusCode, TestkitError> {
    let player = client(&format!("finding-2-competitor-{identity}"));
    let variant = master(fixture, &player, "other", COMPETITOR).await?;
    Ok(player.get(variant).send().await?.status())
}

async fn compete_during_retries(fixture: &Fixture, timeout_ms: u64, limit: u16) -> Result<usize, TestkitError> {
    let first = fixture
        .origin
        .subject_segments()
        .into_iter()
        .find(|request| request.segment == Some(0) && request.attempt == 1)
        .unwrap()
        .at;
    tokio::time::sleep_until((first + Duration::from_millis(timeout_ms - 350)).into()).await;
    let mut probes = Vec::new();
    for identity in 0..20 {
        probes.push(competitor_probe(fixture, identity));
        // All futures are polled together below; their start offsets cover both retry backoffs.
    }
    let results = futures::future::join_all(probes.into_iter().enumerate().map(|(offset, probe)| async move {
        tokio::time::sleep(Duration::from_millis(u64::try_from(offset).unwrap() * 50)).await;
        probe.await
    }))
    .await;
    for result in results {
        assert_eq!(result?, StatusCode::SERVICE_UNAVAILABLE, "competitor must get the configured capacity status");
    }
    assert!(
        fixture.origin.snapshot().iter().all(|request| request.marker != COMPETITOR),
        "a denied competitor must not reach the origin"
    );
    let denials = fixture.reserved_without_io(limit);
    assert!(denials > 0, "must observe ReservedForOther while only the other owners hold I/O allocations");
    assert!(fixture.holders_alive(), "other owners must stay connected throughout the retry");
    Ok(denials)
}

async fn verify_start(
    fixture: &Fixture,
    player: &reqwest::Client,
    variant: &Url,
    limit: u16,
) -> Result<(), TestkitError> {
    let response = player.get(variant.clone()).send().await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await?;
    assert_eq!(fixture.origin.subject_segments().len(), 3);
    assert_eq!(competitor_probe(fixture, 100).await?, StatusCode::SERVICE_UNAVAILABLE);
    assert!(fixture.reserved_without_io(limit) > 0, "accepted manifest must reserve the last slot before client media");
    verify_media(fixture, player, variant, &body, SUBJECT).await?;
    let join = client("finding-2-shared-viewer");
    let joined_variant = master(fixture, &join, "join", SUBJECT).await?;
    assert_eq!(
        variant.path_segments().unwrap().nth(3),
        joined_variant.path_segments().unwrap().nth(3),
        "both viewers must use the same shared session"
    );
    let joined = join.get(joined_variant.clone()).send().await?;
    assert_eq!(joined.status(), StatusCode::OK, "second viewer must not consume another provider slot");
    verify_media(fixture, &join, &joined_variant, &joined.text().await?, SUBJECT).await?;
    assert_eq!(fixture.origin.subject_segments().len(), 3, "cache joins and ranges must not redownload media");
    Ok(())
}

async fn competitor_after_release(fixture: &Fixture) -> Result<(), TestkitError> {
    let player = client("finding-2-after-release");
    let started = Instant::now();
    let variant = master(fixture, &player, "other", COMPETITOR).await?;
    let response = player.get(variant.clone()).send().await?;
    assert_eq!(response.status(), StatusCode::OK, "terminal cleanup must release the slot without lease/GC waiting");
    assert!(started.elapsed() < Duration::from_secs(2), "capacity release must be immediate");
    verify_media(fixture, &player, &variant, &response.text().await?, COMPETITOR).await
}

async fn verify_exhaustion(
    fixture: &Fixture,
    player: &reqwest::Client,
    variant: &Url,
    terminal_status: StatusCode,
) -> Result<(), TestkitError> {
    let attempts: Vec<_> = (0..3)
        .map(|segment| {
            fixture.origin.subject_segments().iter().filter(|request| request.segment == Some(segment)).count()
        })
        .collect();
    // Use a finite TS response so this check itself does not leave an HLS
    // reservation behind and mask a restart of the failed lease below.
    let started = Instant::now();
    let probe_client = client("finding-2-capacity-release");
    let (capacity_status, probe_bytes) = loop {
        let probe = probe_client.get(fixture.entry("other", 103)).send().await?;
        let status = probe.status();
        let bytes = probe.bytes().await?;
        if status == StatusCode::OK || started.elapsed() >= Duration::from_secs(2) {
            break (status, bytes);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    let release_ms = started.elapsed().as_millis();
    let before = fixture.origin.subject_segments().len();
    let mut repeated_statuses = Vec::new();
    for _ in 0..2 {
        repeated_statuses.push(player.get(variant.clone()).send().await?.status());
    }
    let after = fixture.origin.subject_segments().len();
    println!("TERMINAL run={} status={terminal_status} attempts={attempts:?} capacity_status={capacity_status} release_ms={release_ms} repeated_statuses={repeated_statuses:?} origin_before={before} origin_after={after}", fixture.run.0);
    assert_eq!(capacity_status, StatusCode::OK, "terminal failure must immediately release provider capacity");
    assert_eq!(probe_bytes.len(), 16_384, "capacity probe must receive real origin bytes");
    assert!(release_ms < 2_000, "capacity release must not wait for lease expiry or GC");
    // A temporary fetch failure may legitimately be retried by the existing
    // segment failure policy. Require a full resource budget, then stable
    // terminal behavior; do not confuse independent fetches with one retry loop.
    assert!(attempts[0] >= 5, "the critical resource must exhaust a complete retry operation");
    assert_eq!(after, before, "failed-lease polling must not restart origin work");
    assert_eq!(terminal_status, StatusCode::SERVICE_UNAVAILABLE, "configured channel failure status");
    assert!(repeated_statuses.iter().all(|status| *status == StatusCode::SERVICE_UNAVAILABLE));
    Ok(())
}

async fn verify_live_retry_right(fixture: &Fixture, case: Case) {
    wait_for(
        || fixture.origin.subject_segments().iter().any(|request| request.segment == Some(0) && request.attempt == 3),
        Duration::from_secs(2),
        "third attempt retained across backoff",
    )
    .await;
    let critical: Vec<_> =
        fixture.origin.subject_segments().into_iter().filter(|request| request.segment == Some(0)).collect();
    assert!(
        critical[1].at.duration_since(critical[0].at) >= Duration::from_millis(case.timeout_ms() - 100),
        "the first transfer must actually time out"
    );
    assert!(
        critical[2].at.duration_since(critical[1].at) >= Duration::from_millis(150),
        "must observe the real retry backoff before the third attempt"
    );
    // Inspect while the third attempt still owns its work right. After it
    // settles, normal expiry may legitimately detach an otherwise idle binding.
    assert!(!fixture.subject_log_contains("StoreRace"), "retry must retain its generation across backoff");
    assert!(!fixture.subject_log_contains("reason=soft-window-elapsed"), "active retry must retain its binding");
}

async fn verify_retry(
    fixture: &Fixture,
    player: &reqwest::Client,
    variant: &Url,
    case: Case,
    limit: u16,
) -> Result<(), TestkitError> {
    let waiting_player = player.clone();
    let waiting_variant = variant.clone();
    let request = tokio::spawn(async move { waiting_player.get(waiting_variant).send().await });
    wait_for(
        || fixture.origin.subject_segments().iter().any(|request| request.segment == Some(0)),
        Duration::from_secs(3),
        "first origin transfer",
    )
    .await;
    let first = fixture.origin.manifest_at();
    let competition = compete_during_retries(fixture, case.timeout_ms(), limit);
    tokio::pin!(competition);
    if matches!(case, Case::PendingExpiry) {
        let expiry_probe = async {
            tokio::time::sleep_until((first + Duration::from_millis(10_200)).into()).await;
            let response = player.get(variant.clone()).send().await?;
            assert_eq!(
                response.status(),
                StatusCode::NOT_FOUND,
                "expired Pending access must be refused before retry success"
            );
            Ok::<(), TestkitError>(())
        };
        let ((), denied) = tokio::try_join!(expiry_probe, &mut competition)?;
        assert!(denied > 0);
        assert!(first.elapsed() >= Duration::from_secs(10));
    } else {
        competition.await?;
        if matches!(case, Case::Success) {
            assert!(first.elapsed() > Duration::from_secs(4), "normal 3T+1 reservation must have expired");
        }
        assert!(first.elapsed() < Duration::from_secs(9), "leave room for the real Pending deadline");
    }
    if !matches!(case, Case::PendingExpiry) {
        verify_live_retry_right(fixture, case).await;
    }
    fixture.origin.release_retry.send_replace(true);
    let settle_secs = if matches!(case, Case::Exhaustion) { 10 } else { 5 };
    let response = tokio::time::timeout(Duration::from_secs(settle_secs), request)
        .await
        .expect("original manifest waiter must terminate")
        .unwrap()?;
    match case {
        Case::Success => {
            assert_eq!(response.status(), StatusCode::OK, "successful retry must publish the original Pending access");
            assert!(first.elapsed() < Duration::from_secs(10), "must finish before the real Pending deadline");
            verify_media(fixture, player, variant, &response.text().await?, SUBJECT).await?;
            assert_eq!(
                fixture.origin.subject_segments().iter().filter(|request| request.segment == Some(0)).count(),
                3
            );
        }
        Case::Exhaustion => {
            assert!(first.elapsed() < Duration::from_secs(16), "failure must settle before Pending expiry");
            verify_exhaustion(fixture, player, variant, response.status()).await?;
        }
        Case::PendingExpiry => {
            // An already-waiting request may fail closed at its deadline (503)
            // or observe the expired lease (404). Neither may publish live media.
            assert!(
                matches!(response.status(), StatusCode::NOT_FOUND | StatusCode::SERVICE_UNAVAILABLE),
                "expired access must not be revived by its pending waiter"
            );
            let before = fixture.origin.subject_segments().len();
            assert_eq!(player.get(variant.clone()).send().await?.status(), StatusCode::NOT_FOUND);
            assert_eq!(
                fixture.origin.subject_segments().len(),
                before,
                "expired-lease polling must not start a new budget"
            );
            wait_for(
                || {
                    fixture.log_contains("Segment '000000' failed:")
                        || fixture.log_contains("Segment '000000' success:")
                },
                Duration::from_secs(5),
                "expired access must let its bounded operation settle",
            )
            .await;
            competitor_after_release(fixture).await?;
        }
        Case::Start => unreachable!(),
    }
    assert!(!fixture.log_contains("HLS origin account rebound:"), "no account rebind is needed");
    Ok(())
}

async fn run(case: Case, mode: StartupMode, limit: u16) -> Result<(), TestkitError> {
    let fixture = Fixture::start(case, mode, limit).await?;
    let started = Instant::now();
    let result = async {
        let player = client("finding-2-subject");
        let variant = master(&fixture, &player, "viewer", SUBJECT).await?;
        assert!(fixture.origin.snapshot().is_empty(), "master alone must not fetch origin manifests or media");
        match case {
            Case::Start => verify_start(&fixture, &player, &variant, limit).await?,
            Case::Success | Case::Exhaustion | Case::PendingExpiry => verify_retry(&fixture, &player, &variant, case, limit).await?,
        }
        assert!(fixture.holders_alive(), "all preoccupied provider slots must remain occupied");
        println!("FINDING2 mode={mode} limit={limit} case={case:?} passed elapsed_ms={} subject_requests={} reserved_without_io={}",
            started.elapsed().as_millis(), fixture.origin.subject_segments().len(), fixture.reserved_without_io(limit));
        Ok::<(), TestkitError>(())
    }.await;
    let cleanup = fixture.stop().await;
    result?;
    cleanup
}

macro_rules! matrix {
    ($module:ident, $mode:ident, $limit:literal) => {
        mod $module {
            use super::*;
            #[tokio::test]
            #[ignore = "requires built SUT via TULIPROX_TESTKIT_SUT_BINARY"]
            async fn start_protection() -> Result<(), TestkitError> {
                run(Case::Start, StartupMode::$mode, $limit).await
            }
            #[tokio::test]
            #[ignore = "requires built SUT via TULIPROX_TESTKIT_SUT_BINARY"]
            async fn retry_success() -> Result<(), TestkitError> {
                run(Case::Success, StartupMode::$mode, $limit).await
            }
            #[tokio::test]
            #[ignore = "requires built SUT via TULIPROX_TESTKIT_SUT_BINARY"]
            async fn retry_exhaustion() -> Result<(), TestkitError> {
                run(Case::Exhaustion, StartupMode::$mode, $limit).await
            }
            #[tokio::test]
            #[ignore = "requires built SUT via TULIPROX_TESTKIT_SUT_BINARY"]
            async fn pending_expiry() -> Result<(), TestkitError> {
                run(Case::PendingExpiry, StartupMode::$mode, $limit).await
            }
        }
    };
}

matrix!(conservative_limit_1, Conservative, 1);
matrix!(conservative_limit_3, Conservative, 3);
matrix!(first_ready_limit_1, FirstReady, 1);
matrix!(first_ready_limit_3, FirstReady, 3);
matrix!(progressive_limit_1, Progressive, 1);
matrix!(progressive_limit_3, Progressive, 3);
matrix!(optimistic_limit_1, Optimistic, 1);
matrix!(optimistic_limit_3, Optimistic, 3);
