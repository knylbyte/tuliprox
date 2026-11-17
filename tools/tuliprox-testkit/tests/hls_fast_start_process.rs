//! Opt-in real-process checks. Synthetic frames exercise HTTP/cache behavior, not a TS decoder.
#[path = "support/speedtest_proxy.rs"]
mod speedtest_proxy;
use std::{collections::HashMap, path::Path};
use tuliprox_testkit::{
    bootstrap::{fixture_password, FixtureInputPlan, IsolatedFixture},
    config::{BootstrapConfig, Channel, FixtureInputType, FixtureStalker, FixtureStreamOptions, OriginConfig},
    discovery::VirtualIdMap,
    hls_probe::{probe, HlsOriginProfile, ProbeOptions, StartupMode},
    protocol::RunId,
    TestkitError,
};

async fn discover_playlist(client: &reqwest::Client, url: &str) -> Result<VirtualIdMap, TestkitError> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let attempt = tokio::time::timeout_at(deadline, async {
            let playlist = client.get(url).send().await?.error_for_status()?.text().await?;
            VirtualIdMap::from_m3u(&playlist)
        })
        .await;
        if let Ok(Ok(urls)) = attempt {
            return Ok(urls);
        }
        // Readiness precedes the initial catalog commit; discovery may observe an unfinished playlist.
        if tokio::time::Instant::now() >= deadline {
            return Err(TestkitError::Protocol("fixture playlist discovery exceeded absolute deadline".to_owned()));
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "Keep the real-process scenario, origin evidence and cleanup in one lifecycle."
)]
async fn run(
    mode: StartupMode,
    missing: u32,
    abort: bool,
    unknown_length: bool,
    speedtest: bool,
) -> Result<(), TestkitError> {
    let sut = std::env::var("TULIPROX_TESTKIT_SUT_BINARY")
        .map_err(|_| TestkitError::Configuration("set TULIPROX_TESTKIT_SUT_BINARY to the built SUT".to_owned()))?;
    let testkit = env!("CARGO_BIN_EXE_tuliprox-testkit");
    let run_id = format!("hls-fast-start-{mode}-{missing}-{abort}-{unknown_length}");
    let profile = HlsOriginProfile {
        missing_head_segments: missing,
        unknown_length,
        prefix_frames: if speedtest { 7200 } else { 1 },
        target_duration_secs: if speedtest { 6 } else { 4 },
        prefix_delay_millis: if speedtest { 0 } else { 1000 },
        prefix_chunk_bytes: if speedtest { 15_000 } else { 0 },
        prefix_chunk_interval_millis: if speedtest { 10 } else { 0 },
        initial_body_delays_millis: if speedtest { vec![0, 300, 500, 0, 300, 500] } else { Vec::new() },
        abort_segments: if abort { vec![0] } else { Vec::new() },
        ..HlsOriginProfile::default()
    };
    let origin = OriginConfig {
        run_id: run_id.clone(),
        account_limit: None,
        limit_mode: None,
        connect_latency_ms: None,
        hls_profile: Some(profile),
    };
    let bootstrap = BootstrapConfig {
        command: sut,
        arguments: ["-s", "-c", "{config_file}", "-i", "{source_file}", "-a", "{api_proxy_file}"]
            .map(str::to_owned)
            .to_vec(),
        readiness_url: "{base_url}/healthcheck".to_owned(),
        readiness_timeout_millis: 30_000,
        extra_config_files: Vec::new(),
    };
    let channels = HashMap::from([("channel".to_owned(), Channel { origin_marker: 17, protocol: "hls".to_owned() })]);
    let options = FixtureStreamOptions {
        hls_startup_mode: Some(mode),
        hls_startup_strip_segments: (missing > 0 || abort).then_some(3),
        ..FixtureStreamOptions::default()
    };
    let fixture = IsolatedFixture::start(
        &bootstrap,
        Path::new(env!("CARGO_MANIFEST_DIR")),
        Path::new(testkit),
        &run_id,
        Some(&origin),
        None,
        &channels,
        &options,
        false,
        &FixtureInputPlan {
            input_type: FixtureInputType::M3u,
            stalker: &FixtureStalker::default(),
            failover_inputs: &[],
            failover_runtime: None,
        },
    )
    .await?;
    let result = async {
        let client = reqwest::Client::new();
        let playlist_url = format!("{}/m3u?username=testkit&password={}", fixture.base_url, fixture_password());
        let urls = discover_playlist(&client, &playlist_url).await?;
        let progressive = matches!(mode, StartupMode::Progressive | StartupMode::Optimistic);
        let prefix_timing = progressive && missing == 0;
        let relay = if speedtest {
            let address = fixture
                .base_url
                .strip_prefix("http://")
                .ok_or_else(|| TestkitError::Protocol("invalid fixture URL".to_owned()))?
                .parse()
                .map_err(|error| TestkitError::Protocol(format!("invalid fixture address: {error}")))?;
            Some(speedtest_proxy::start(address).await?)
        } else {
            None
        };
        let result = probe(
            urls.playback_url(17)?,
            &RunId::new(&run_id),
            17,
            &ProbeOptions {
                verify_revision: !abort && !speedtest,
                expect_body_error: abort,
                max_first_byte_millis: if speedtest {
                    None
                } else if missing > 0 {
                    Some(5_000)
                } else {
                    prefix_timing.then_some(900)
                },
                min_prefix_to_eof_millis: (!speedtest && prefix_timing).then_some(500),
                client_delivery_rate_bps: None,
                http_proxy: relay.as_ref().map(|(address, _)| format!("http://{address}")),
            },
        )
        .await;
        if let Some((_, task)) = relay {
            task.abort();
        }
        let result = result?;
        if !speedtest && !progressive && result.entry_to_first_byte_millis < 800.0 {
            return Err(TestkitError::Protocol("processed response preceded origin EOF".to_owned()));
        }
        let events: Vec<tuliprox_testkit::origin_events::OriginEvent> = client
            .get(format!("{}/v1/runs/{run_id}/events", fixture.origin_control_url))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let mut segment_counts = HashMap::new();
        for event in events {
            if let tuliprox_testkit::origin_events::OriginEventKind::RequestStarted { path, .. } = event.kind {
                if Path::new(&path).extension().is_some_and(|extension| extension.eq_ignore_ascii_case("ts")) {
                    *segment_counts.entry(path).or_insert(0_usize) += 1;
                }
            }
        }
        for sequence in 0..missing.max(u32::from(abort)) {
            if !segment_counts.contains_key(&format!("/hls/17/{sequence}.ts")) {
                return Err(TestkitError::Protocol(format!(
                    "startup fixture did not exercise faulted origin segment {sequence}: {segment_counts:?}"
                )));
            }
        }
        if !abort
            && segment_counts.iter().any(|(path, count)| {
                *count > 1 && !(missing > 0 && (path.ends_with("/0.ts") || path.ends_with("/1.ts")))
            })
        {
            return Err(TestkitError::Protocol("retry or range started a second origin segment download".to_owned()));
        }
        if speedtest {
            if !matches!(mode, StartupMode::Conservative)
                && (3..=5).any(|sequence| !segment_counts.contains_key(&format!("/hls/17/{sequence}.ts")))
            {
                return Err(TestkitError::Protocol(
                    "speedtest did not request the expected startup segments 3..5".to_owned(),
                ));
            }
            if result.entry_to_4096_millis.is_none() {
                return Err(TestkitError::Protocol("speedtest received fewer than 4096 bytes".to_owned()));
            }
            println!(
                "SHLS_SPEEDTEST_JSON={}",
                serde_json::to_string(&result).map_err(|error| TestkitError::Protocol(error.to_string()))?
            );
        }
        println!("{mode} missing={missing} abort={abort} unknown_length={unknown_length}: {result:?}");
        Ok(())
    }
    .await;
    let cleanup = fixture.stop().await;
    result?;
    cleanup
}

#[tokio::test]
#[ignore = "requires built SUT via TULIPROX_TESTKIT_SUT_BINARY"]
async fn conservative_waits_for_complete_origin_and_preserves_ranges() -> Result<(), TestkitError> {
    run(StartupMode::Conservative, 0, false, false, false).await
}
#[tokio::test]
#[ignore = "requires built SUT via TULIPROX_TESTKIT_SUT_BINARY"]
async fn first_ready_delivers_processed_revision() -> Result<(), TestkitError> {
    run(StartupMode::FirstReady, 0, false, false, false).await
}
#[tokio::test]
#[ignore = "requires built SUT via TULIPROX_TESTKIT_SUT_BINARY"]
async fn progressive_delivers_prefix_before_origin_eof() -> Result<(), TestkitError> {
    run(StartupMode::Progressive, 0, false, false, false).await
}
#[tokio::test]
#[ignore = "requires built SUT via TULIPROX_TESTKIT_SUT_BINARY"]
async fn conservative_skips_404_and_410_head() -> Result<(), TestkitError> {
    run(StartupMode::Conservative, 2, false, false, false).await
}
#[tokio::test]
#[ignore = "requires built SUT via TULIPROX_TESTKIT_SUT_BINARY"]
async fn first_ready_skips_404_and_410_head() -> Result<(), TestkitError> {
    run(StartupMode::FirstReady, 2, false, false, false).await
}
#[tokio::test]
#[ignore = "requires built SUT via TULIPROX_TESTKIT_SUT_BINARY"]
async fn progressive_skips_404_and_410_head() -> Result<(), TestkitError> {
    run(StartupMode::Progressive, 2, false, false, false).await
}
#[tokio::test]
#[ignore = "requires built SUT via TULIPROX_TESTKIT_SUT_BINARY"]
async fn progressive_known_length_drop_is_not_successful_eof() -> Result<(), TestkitError> {
    run(StartupMode::Progressive, 0, true, false, false).await
}
#[tokio::test]
#[ignore = "requires built SUT via TULIPROX_TESTKIT_SUT_BINARY"]
async fn progressive_chunked_drop_is_not_successful_eof() -> Result<(), TestkitError> {
    run(StartupMode::Progressive, 0, true, true, false).await
}
#[tokio::test]
#[ignore = "requires built SUT via TULIPROX_TESTKIT_SUT_BINARY"]
async fn optimistic_delivers_prefix_before_origin_eof() -> Result<(), TestkitError> {
    run(StartupMode::Optimistic, 0, false, false, false).await
}
#[tokio::test]
#[ignore = "requires built SUT via TULIPROX_TESTKIT_SUT_BINARY"]
async fn optimistic_skips_404_and_410_head() -> Result<(), TestkitError> {
    run(StartupMode::Optimistic, 2, false, false, false).await
}

#[tokio::test]
#[ignore = "requires built SUT and SHLS_SPEEDTEST_MODE"]
async fn measure_speedtest_fixture() -> Result<(), TestkitError> {
    let mode = match std::env::var("SHLS_SPEEDTEST_MODE").as_deref() {
        Ok("conservative") => StartupMode::Conservative,
        Ok("first_ready") => StartupMode::FirstReady,
        Ok("progressive") => StartupMode::Progressive,
        Ok("optimistic") => StartupMode::Optimistic,
        _ => return Err(TestkitError::Configuration("invalid SHLS_SPEEDTEST_MODE".to_owned())),
    };
    run(mode, 0, false, false, true).await
}
