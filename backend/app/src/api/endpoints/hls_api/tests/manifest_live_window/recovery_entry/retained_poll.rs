use super::{media_urls, text, Playback, FIRST};
use crate::api::endpoints::hls_api::tests::{
    cache_test_m3u_hls_item, configure_default_test_server, enable_channel_unavailable_custom_response, get_response,
    manifest_media_sequence, store_test_sources_with_target, test_m3u_hls_item, test_m3u_hls_share_target,
};
use axum::{http::StatusCode, response::Response};
use std::{sync::Arc, time::Duration};
use tokio::{sync::watch, task::JoinHandle, time::timeout};
use tuliprox_hls::{
    api::{
        evaluate_lease_reserve, HlsFreshManifestRequiredReason, HlsLeasePlaybackMode, HlsLeaseReserveInput,
        HlsRecoveryTriggerBudgetMs,
    },
    HlsManifestAcceptanceTrigger,
};

const VIEWER: &str = "retained-poll-b";
const NEW_VIEWER: &str = "retained-poll-c";
const PROXY_AUTH: StatusCode = StatusCode::PROXY_AUTHENTICATION_REQUIRED;

async fn poll(playback: &Playback, lease: &str) -> Response {
    poll_route(Arc::clone(&playback.app), format!("/hls/shared/live/{}/{lease}/manifest.m3u8", playback.proxy.0)).await
}

async fn poll_route(app: Arc<super::AppState>, uri: String) -> Response { get_response(app, &uri, None).await }

async fn follow_window() -> (Playback, String) { follow_window_with_key(None).await }

async fn follow_window_with_key(key: Option<tuliprox_hls::api::HlsSessionKey>) -> (Playback, String) {
    let playback = Playback::with_session_key(super::MediaShape::Plain, Some(12), key).await;
    configure_default_test_server(&playback.app);
    let mut target = test_m3u_hls_share_target();
    target.name = "default".into();
    store_test_sources_with_target(&playback.app, playback.input.clone(), target.clone());
    cache_test_m3u_hls_item(&playback.app, &target, test_m3u_hls_item(&playback.input, 12345, "12345", &playback.url))
        .await;
    enable_channel_unavailable_custom_response(&playback.app);

    let mut config = playback.app.app_config.config.load_full().as_ref().clone();
    let cache = config.reverse_proxy.as_mut().unwrap().hls_cache.as_mut().unwrap();
    cache.strip.value = 2;
    cache.initial_manifest_wait_timeout_secs = shared::model::Secs::new(10);
    playback.app.app_config.config.store(Arc::new(config));
    playback.app.hls.proxy.update_config(&playback.app.app_config).await;
    {
        let mut script = playback.script.lock().unwrap();
        script.head = FIRST + 4;
        script.segment_count = 5;
    }
    playback.refresh(HlsManifestAcceptanceTrigger::None).await;
    playback.ready(FIRST + 4, FIRST + 9).await;
    let warm = playback.request("warm").await;
    assert_eq!(warm.status(), StatusCode::OK);
    let warm = text(warm).await;
    for uri in media_urls(&warm).into_iter().filter(|uri| uri.ends_with("000007.ts") || uri.ends_with("000008.ts")) {
        playback.assert_media(uri).await;
    }
    playback.session.write().await.render_and_store_manifest(super::current_time_millis()).unwrap();
    playback.retry_gap_with_status(PROXY_AUTH).await;
    playback.prepare_lease(VIEWER).await;
    let fallback = playback.request(VIEWER).await;
    assert_eq!(fallback.status(), StatusCode::OK);
    assert!(playback.lease(VIEWER).await.last_manifest_snapshot.unwrap().is_recovery_entry());
    let fallback = text(fallback).await;
    playback.assert_media(media_urls(&fallback)[0]).await;
    let response = poll(&playback, VIEWER).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = text(response).await;
    assert_eq!(manifest_media_sequence(&body), 4);
    assert_eq!(media_urls(&body).len(), 5);
    assert!(!body.contains("#EXT-X-ENDLIST"));
    let lease = playback.lease(VIEWER).await;
    assert!(!lease.last_manifest_snapshot.unwrap().is_recovery_entry());
    playback.exhaust_lease("warm").await;
    assert!(playback.reserve(VIEWER).await.guaranteed_reserve_ms > 0);
    (playback, body)
}

async fn fail_new_entry(playback: &Playback) {
    let due = playback.session.read().await.origin_refresh.next_fetch_allowed_at_ms;
    playback.app.hls.proxy.freeze_clock_for_test(playback.app.hls.proxy.now_ms().max(due));
    playback.app.hls.proxy.availability_reevaluations().cancel_session(&playback.proxy);
    playback.prepare_lease(NEW_VIEWER).await;
    let requests = playback.origin.manifest_request_count();
    let release = hold_origin(playback, PROXY_AUTH);
    let (first_response, early_retries) = timeout(Duration::from_secs(3), async {
        let mut early_retries = 0;
        loop {
            let mut request = std::pin::pin!(poll(playback, NEW_VIEWER));
            tokio::select! {
                biased;
                () = playback.recovery_at_origin.notified() => {
                    assert_eq!(playback.origin.manifest_request_count(), requests + 1);
                    release_origin(playback, &release);
                    return (request.await, early_retries);
                }
                response = request.as_mut() => {
                    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
                    early_retries += 1;
                    tokio::task::yield_now().await;
                }
            }
        }
    })
    .await
    .expect("C must start the hard 407 at the controlled origin");
    playback.wait_for_refresh_to_finish().await;
    assert!(playback.lease(NEW_VIEWER).await.manifest_start_attempt.as_ref().is_some_and(|attempt| attempt.failed()));
    let first_status = first_response.status();
    let response = if first_status == StatusCode::SERVICE_UNAVAILABLE {
        // A contended request may finish before its owner. Re-read the same
        // lease once its failed attempt is committed.
        poll(playback, NEW_VIEWER).await
    } else {
        first_response
    };
    assert_eq!(response.status(), StatusCode::OK);
    let body = text(response).await;
    assert!(body.contains("#EXT-X-ENDLIST"));
    assert!(playback.lease(NEW_VIEWER).await.last_manifest_snapshot.is_none());
    playback.app.hls.proxy.availability_reevaluations().cancel_session(&playback.proxy);
    let state = playback.session.read().await;
    assert_eq!(playback.origin.manifest_request_count(), requests + 1);
    assert_eq!(state.fresh_manifest_commit_required, Some(HlsFreshManifestRequiredReason::PreviousHardManifestFailure));
    assert_eq!(state.recovery_entry_established_leases_for_test().len(), 1);
    eprintln!(
        "new_entry origin_status=407 origin_gets={} early_503s={early_retries} first_response={} c_status=200 c_endlist=true",
        requests + 1,
        first_status.as_u16()
    );
}

fn hold_origin(playback: &Playback, status: StatusCode) -> watch::Sender<bool> {
    let (release, gate) = watch::channel(false);
    {
        let mut script = playback.script.lock().unwrap();
        script.manifest_status = status;
        script.recovery_gate = Some(gate);
    }
    release
}

async fn start_held_poll(
    playback: &Playback,
    status: StatusCode,
) -> (watch::Sender<bool>, JoinHandle<(StatusCode, String)>) {
    let release = hold_origin(playback, status);
    let now = playback.app.hls.proxy.now_ms();
    let due = playback.session.read().await.origin_refresh.next_fetch_allowed_at_ms;
    playback.app.hls.proxy.freeze_clock_for_test(now.max(due));
    let requests = playback.origin.manifest_request_count();
    let app = Arc::clone(&playback.app);
    let uri = format!("/hls/shared/live/{}/{VIEWER}/manifest.m3u8", playback.proxy.0);
    let task = tokio::spawn(async move {
        let response = get_response(app, &uri, None).await;
        (response.status(), text(response).await)
    });
    timeout(Duration::from_secs(3), playback.recovery_at_origin.notified())
        .await
        .expect("B schedules a due origin GET");
    assert_eq!(playback.origin.manifest_request_count(), requests + 1);
    assert!(playback.session.read().await.origin_refresh.in_flight);
    (release, task)
}

#[tokio::test]
async fn retained_poll_returns_before_held_origin_after_new_entry_407() {
    super::super::super::origin_restart::trace_fixture();
    let (playback, body) = follow_window().await;
    fail_new_entry(&playback).await;
    let (release, mut task) = start_held_poll(&playback, PROXY_AUTH).await;
    let ready = timeout(Duration::from_millis(500), &mut task).await;
    if ready.is_err() {
        release.send(true).unwrap();
        playback.wait_for_refresh_to_finish().await;
        let after_origin = task.is_finished();
        let reserve = playback.reserve(VIEWER).await.guaranteed_reserve_ms;
        eprintln!("control held_response=false after_origin_response={after_origin} reserve_ms={reserve}");
        playback.app.hls.proxy.freeze_clock_for_test(playback.app.hls.proxy.now_ms() + reserve);
        let terminal = timeout(Duration::from_secs(5), &mut task).await;
        if let Ok(Ok((status, body))) = terminal {
            eprintln!(
                "control at_exhaustion status={status} endlist={} terminal={}",
                body.contains("#EXT-X-ENDLIST"),
                body.contains("/terminal/")
            );
        }
        panic!("B did not receive its full manifest while the origin response was held");
    }
    let (status, received) = ready.unwrap().unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(received, body);
    assert!(!*release.borrow());
    assert!(playback.session.read().await.origin_refresh.in_flight);
    eprintln!("held_origin=true b_status=200 full_playlist_received=true\n{body}");
    playback.assert_media(media_urls(&body)[0]).await;
    release.send(true).unwrap();
    playback.wait_for_refresh_to_finish().await;
}

#[tokio::test]
async fn group_lease_retains_the_canonical_manifest_during_same_member_407_recovery() {
    let key = tuliprox_hls::api::HlsSessionKey::for_group(1, 12345);
    let (playback, body) = follow_window_with_key(Some(key)).await;
    let source = playback.session.read().await.origin_source.clone();
    fail_new_entry(&playback).await;
    let release = assert_held_poll(&playback, &body, PROXY_AUTH).await;
    playback.assert_media(media_urls(&body)[0]).await;
    assert_eq!(playback.session.read().await.origin_source, source);
    finish_origin(&playback, &release).await;
}

struct Reserve {
    horizon: u64,
    position: u64,
    remaining: u64,
}

async fn reserve_snapshot(playback: &Playback) -> Reserve {
    let lease = playback.lease(VIEWER).await;
    let snapshot = lease.last_manifest_snapshot.as_ref().unwrap();
    let now_ms = playback.app.hls.proxy.now_ms();
    let timeline = playback.session.read().await.ready_timeline_snapshot(
        lease.playback_cursor.ready_timeline_start_proxy_seq(snapshot.first_proxy_seq),
        now_ms,
    );
    let reserve = evaluate_lease_reserve(HlsLeaseReserveInput {
        manifest: snapshot,
        cursor: &lease.playback_cursor,
        ready_timeline: &timeline,
        now_ms,
        recovery_trigger_budget: HlsRecoveryTriggerBudgetMs::from_millis(0),
        origin_path_degraded: true,
        recovery_committed: false,
    });
    assert!(reserve.playback_position_known);
    Reserve {
        horizon: reserve.guaranteed_media_horizon_ms,
        position: reserve.conservative_playback_position_ms,
        remaining: reserve.guaranteed_reserve_ms,
    }
}

async fn finish_origin(playback: &Playback, release: &watch::Sender<bool>) {
    release_origin(playback, release);
    playback.wait_for_refresh_to_finish().await;
    playback.app.hls.proxy.availability_reevaluations().cancel_session(&playback.proxy);
}

fn release_origin(playback: &Playback, release: &watch::Sender<bool>) {
    // Only the held GET signals the barrier. Subsequent HTTP retries use the
    // same status without leaving a notification for the next held attempt.
    playback.script.lock().unwrap().recovery_gate = None;
    release.send(true).unwrap();
}

async fn assert_held_poll(playback: &Playback, expected: &str, status: StatusCode) -> watch::Sender<bool> {
    let (release, task) = start_held_poll(playback, status).await;
    let (response_status, body) = timeout(Duration::from_millis(500), task).await.unwrap().unwrap();
    assert_eq!(response_status, StatusCode::OK);
    assert_eq!(body, expected);
    assert!(!*release.borrow());
    eprintln!(
        "origin_status={} origin_gets={} response_status=200 release_pending=true",
        status.as_u16(),
        playback.origin.manifest_request_count()
    );
    release
}

async fn wait_for_terminal_bundle(playback: &Playback, target: u64) {
    let responses = playback.app.app_config.custom_stream_response.load_full().unwrap();
    let asset = tuliprox_hls::snapshot_terminal_media_asset(responses.channel_unavailable.as_ref().unwrap()).unwrap();
    let count = tuliprox_hls::HLS_TERMINAL_TAIL_SEGMENT_COUNT;
    let key = tuliprox_hls::prepared_terminal_bundle_key(&asset, target, count);
    let proxy = &playback.app.hls.proxy;
    let state = proxy.start_prepared_terminal_bundle(asset, target, count);
    let state = match state {
        tuliprox_hls::HlsPreparedTerminalBundleState::Preparing { .. } => {
            proxy.wait_for_prepared_terminal_bundle(key).await.unwrap()
        }
        state => state,
    };
    assert!(matches!(state, tuliprox_hls::HlsPreparedTerminalBundleState::Ready { .. }));
}

#[tokio::test]
async fn retained_polls_keep_the_original_reserve_and_entry_boundary() {
    super::super::super::origin_restart::trace_fixture();
    let (playback, body) = follow_window().await;
    fail_new_entry(&playback).await;
    let original = playback.lease(VIEWER).await;
    wait_for_terminal_bundle(&playback, original.last_manifest_snapshot.as_ref().unwrap().target_duration_ms).await;
    let base = playback.app.hls.proxy.now_ms().max(original.playback_cursor.first_segment_started_at_ms.unwrap());
    playback.app.hls.proxy.freeze_clock_for_test(base + 1);
    let initial = reserve_snapshot(&playback).await;
    let deadline = playback.app.hls.proxy.now_ms() + initial.remaining;
    let commits = playback.session.read().await.origin_manifest_commits();
    let progress = playback.session.read().await.origin_control.progress_generation;
    let mut previous_reserve = initial.remaining;

    for status in [PROXY_AUTH, PROXY_AUTH, StatusCode::OK] {
        let release = assert_held_poll(&playback, &body, status).await;
        playback.app.hls.proxy.freeze_clock_for_test(playback.app.hls.proxy.now_ms() + 1);
        let requests = playback.origin.manifest_request_count();
        let response = timeout(Duration::from_millis(500), poll(&playback, VIEWER)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(text(response).await, body);
        assert_eq!(playback.origin.manifest_request_count(), requests);
        let reserve = reserve_snapshot(&playback).await;
        assert_eq!(reserve.horizon, initial.horizon);
        eprintln!(
            "now_ms={} position_ms={} horizon_ms={} reserve_ms={} exhaustion_at_ms={deadline}",
            playback.app.hls.proxy.now_ms(),
            reserve.position,
            reserve.horizon,
            reserve.remaining
        );
        assert!(reserve.remaining < previous_reserve);
        previous_reserve = reserve.remaining;
        assert_eq!(playback.app.hls.proxy.now_ms() + reserve.remaining, deadline);
        let current = playback.lease(VIEWER).await;
        assert_eq!(current.playback_cursor, original.playback_cursor);
        assert_eq!(current.last_manifest_snapshot, original.last_manifest_snapshot);
        finish_origin(&playback, &release).await;
        let state = playback.session.read().await;
        if status == PROXY_AUTH {
            assert_eq!(state.origin_refresh.last_error_at_ms, Some(playback.app.hls.proxy.now_ms()));
            assert_eq!(
                state.origin_control.path_condition,
                tuliprox_hls::api::HlsOriginPathCondition::RetryableFetchFailure
            );
            assert_eq!(state.origin_manifest_commits(), commits);
        } else {
            assert_eq!(state.origin_control.progress_generation, progress);
            assert_eq!(playback.origin.manifest_request_count(), requests);
            eprintln!(
                "unchanged_origin_response=200 origin_commits_before={commits} origin_commits_after={}",
                state.origin_manifest_commits()
            );
        }
    }

    let before_deadline = deadline - 1;
    // Jump the playback clock without expiring the lease's activity window.
    let mut lease = playback.lease(VIEWER).await;
    lease.active_until_ms = Some(deadline + 5_000);
    playback.app.hls.proxy.prepare_access_lease(lease).await;
    playback.app.hls.proxy.freeze_clock_for_test(before_deadline);
    assert_eq!(reserve_snapshot(&playback).await.remaining, 1);
    let (release, task) = start_held_poll(&playback, PROXY_AUTH).await;
    let (status, last_live) = timeout(Duration::from_millis(500), task).await.unwrap().unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(last_live, body);
    let late = "retained-poll-d";
    playback.prepare_lease(late).await;
    let response = poll_late_entry(&playback, late, &release).await;
    let status = response.status();
    assert!(!text(response).await.contains("/hls/shared/live/"));
    assert!(playback.lease(late).await.last_manifest_snapshot.is_none());
    eprintln!("late_entry status={status} live_publication=false b_reserve_ms=1");
    playback.app.hls.proxy.freeze_clock_for_test(deadline);
    assert_eq!(reserve_snapshot(&playback).await.remaining, 0);

    let response = timeout(Duration::from_secs(12), poll(&playback, VIEWER)).await.unwrap();
    let status = response.status();
    let terminal = text(response).await;
    assert!(matches!(playback.lease(VIEWER).await.playback_mode, HlsLeasePlaybackMode::TerminalTail(_)));
    assert_eq!(status, StatusCode::OK);
    assert!(terminal.contains("/terminal/") && terminal.ends_with("#EXT-X-ENDLIST\n"));
    assert_ne!(terminal, body);
    let requests = playback.origin.manifest_request_count();
    let repeat = poll(&playback, VIEWER).await;
    assert_eq!(repeat.status(), status);
    assert_eq!(text(repeat).await, terminal);
    assert_eq!(playback.origin.manifest_request_count(), requests);
    // Rewind only the test clock to prove the committed decision wins over
    // the same cache and cursor evidence even when they show positive reserve.
    playback.app.hls.proxy.freeze_clock_for_test(before_deadline);
    assert_eq!(reserve_snapshot(&playback).await.remaining, 1);
    let repeat_with_reserve = poll(&playback, VIEWER).await;
    assert_eq!(repeat_with_reserve.status(), status);
    assert_eq!(text(repeat_with_reserve).await, terminal);
    assert_eq!(playback.origin.manifest_request_count(), requests);
    eprintln!(
        "exhaustion_at_ms={deadline} reserve_ms=0 terminal_status={status} endlist={} repeat_identical=true terminal_wins_with_positive_reserve=true",
        terminal.contains("#EXT-X-ENDLIST")
    );
}

async fn poll_late_entry(playback: &Playback, lease: &str, release: &watch::Sender<bool>) -> Response {
    let mut request = std::pin::pin!(poll(playback, lease));
    let held_response = timeout(Duration::from_millis(250), request.as_mut()).await;
    assert!(playback.lease(lease).await.last_manifest_snapshot.is_none());
    finish_origin(playback, release).await;
    match held_response {
        Ok(response) => response,
        Err(_) => timeout(Duration::from_secs(12), request).await.unwrap(),
    }
}

#[tokio::test]
async fn retained_poll_rejects_invalid_binding_with_positive_reserve() {
    let (playback, body) = follow_window().await;
    fail_new_entry(&playback).await;
    {
        let state = playback.session.read().await;
        let revision = state.startup.as_ref().unwrap().revisions[&8].revision();
        let tuliprox_hls::SegmentRevisionState::Complete(metadata) = revision.state() else {
            panic!("published segment is complete");
        };
        std::fs::remove_file(&metadata.path).unwrap();
    }
    let reserve = reserve_snapshot(&playback).await.remaining;
    assert!(reserve > 0, "only the local binding is invalid");
    let (release, mut task) = start_held_poll(&playback, PROXY_AUTH).await;
    assert!(timeout(Duration::from_millis(250), &mut task).await.is_err());
    finish_origin(&playback, &release).await;
    let (status, received) = timeout(Duration::from_secs(12), task).await.unwrap().unwrap();
    assert_ne!(received, body);
    eprintln!("invalid_binding=true reserve_ms={reserve} status={status} reused=false");
}

#[tokio::test]
async fn retained_poll_prefers_a_playable_continuation() {
    let (playback, body) = follow_window().await;
    fail_new_entry(&playback).await;
    {
        let mut script = playback.script.lock().unwrap();
        script.head = FIRST + 5;
    }
    let release = assert_held_poll(&playback, &body, StatusCode::OK).await;
    finish_origin(&playback, &release).await;
    playback.ready(FIRST + 5, FIRST + 10).await;
    let response = timeout(Duration::from_secs(2), poll(&playback, VIEWER)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let new = text(response).await;
    assert_ne!(new, body);
    assert_eq!(manifest_media_sequence(&new), 5);
    assert_eq!(media_urls(&new).len(), 5);
    assert_eq!(media_urls(&new)[..4], media_urls(&body)[1..]);
    assert!(!new.contains("#EXT-X-ENDLIST"));
    playback.assert_media(media_urls(&new)[4]).await;
    eprintln!("new_window=5-9 status=200 overlap_unchanged=true\n{new}");
}

async fn stage_continuation(playback: &Playback) {
    {
        let mut script = playback.script.lock().unwrap();
        script.head = FIRST + 5;
        script.manifest_status = StatusCode::OK;
        script.recovery_gate = None;
    }
    playback.refresh(HlsManifestAcceptanceTrigger::None).await;
    playback.ready(FIRST + 5, FIRST + 10).await;
    let mut state = playback.session.write().await;
    let now = playback.app.hls.proxy.now_ms();
    state.render_and_store_manifest(now).unwrap();
    assert_eq!(state.last_rendered_manifest.as_ref().unwrap().last_proxy_seq, 9);
}

#[tokio::test]
async fn retained_poll_keeps_its_window_when_freshness_blocks_the_ready_successor() {
    let (playback, body) = follow_window().await;
    stage_continuation(&playback).await;
    let now = playback.app.hls.proxy.now_ms();
    {
        let mut state = playback.session.write().await;
        state.origin_refresh.last_success_at_ms = Some(now - 8_000);
        state.origin_control.last_media_progress_at_ms = Some(now - 45_000);
    }
    fail_new_entry(&playback).await;
    assert!(reserve_snapshot(&playback).await.remaining > 0);
    let release = assert_held_poll(&playback, &body, PROXY_AUTH).await;
    assert_eq!(playback.lease(VIEWER).await.last_manifest_snapshot.unwrap().first_proxy_seq, 4);
    assert_eq!(playback.session.read().await.last_rendered_manifest.as_ref().unwrap().last_proxy_seq, 9);
    eprintln!("newer_cache_ready=true last_proxy=9 fresh_commit_required=true held_origin=true b_window=4-8");
    finish_origin(&playback, &release).await;
}

use crate::api::endpoints::hls_api::manifest::{
    HlsManifestPublicationBarrier, HlsManifestPublicationStage, HLS_MANIFEST_PUBLICATION_BARRIER,
};

fn poll_at_stage(
    playback: &Playback,
    stage: HlsManifestPublicationStage,
) -> (Arc<HlsManifestPublicationBarrier>, JoinHandle<(StatusCode, String)>) {
    let barrier = Arc::new(HlsManifestPublicationBarrier { stage, ..Default::default() });
    let scoped_barrier = Arc::clone(&barrier);
    let app = Arc::clone(&playback.app);
    let uri = format!("/hls/shared/live/{}/{VIEWER}/manifest.m3u8", playback.proxy.0);
    let task = tokio::spawn(HLS_MANIFEST_PUBLICATION_BARRIER.scope(
        std::cell::RefCell::new(Some(scoped_barrier)),
        async move {
            let response = get_response(app, &uri, None).await;
            (response.status(), text(response).await)
        },
    ));
    (barrier, task)
}

#[tokio::test]
async fn retained_poll_reselects_after_a_concurrent_newer_publication() {
    let (playback, body) = follow_window().await;
    fail_new_entry(&playback).await;
    let (barrier, pending) = poll_at_stage(&playback, HlsManifestPublicationStage::RetainedPoll);
    timeout(Duration::from_secs(2), barrier.prepared.notified()).await.unwrap();
    playback.script.lock().unwrap().head = FIRST + 5;
    let release = assert_held_poll(&playback, &body, StatusCode::OK).await;
    finish_origin(&playback, &release).await;
    playback.ready(FIRST + 5, FIRST + 10).await;
    let response = poll(&playback, VIEWER).await;
    assert_eq!(response.status(), StatusCode::OK);
    let newer = text(response).await;
    assert_ne!(newer, body);
    assert_eq!(manifest_media_sequence(&newer), 5);
    let committed = playback.lease(VIEWER).await.last_manifest_snapshot.unwrap();
    barrier.resume.notify_one();
    let (status, received) = timeout(Duration::from_secs(2), pending).await.unwrap().unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(received, newer);
    assert_eq!(playback.lease(VIEWER).await.last_manifest_snapshot.unwrap().first_proxy_seq, 5);
    eprintln!("concurrent_snapshot={} b_status=200 returned_new_window=5-9", committed.snapshot_generation);
}

#[tokio::test]
async fn retained_poll_reselects_after_a_concurrent_terminal_commit() {
    let (playback, body) = follow_window().await;
    fail_new_entry(&playback).await;
    let mut lease = playback.lease(VIEWER).await;
    wait_for_terminal_bundle(&playback, lease.last_manifest_snapshot.as_ref().unwrap().target_duration_ms).await;
    let release = assert_held_poll(&playback, &body, PROXY_AUTH).await;
    finish_origin(&playback, &release).await;
    let proxy = &playback.app.hls.proxy;
    let (barrier, pending) = poll_at_stage(&playback, HlsManifestPublicationStage::RetainedPoll);
    timeout(Duration::from_secs(2), barrier.prepared.notified()).await.unwrap();
    let started_at = lease.playback_cursor.first_segment_started_at_ms.unwrap();
    let horizon = reserve_snapshot(&playback).await.horizon;
    let base_media_start = playback.session.read().await.segments[&4].media_start_ms;
    let deadline = started_at + horizon - base_media_start;
    lease = playback.lease(VIEWER).await;
    lease.active_until_ms = Some(deadline + 5_000);
    proxy.prepare_access_lease(lease.clone()).await;
    proxy.freeze_clock_for_test(deadline);
    assert_eq!(reserve_snapshot(&playback).await.remaining, 0);
    assert_eq!(
        tuliprox_hls::api::commit_terminal_tail_if_lease_reserve_requires_cutover(
            &playback.app.hls_ctx(),
            &playback.session,
            &playback.proxy,
            &lease,
            deadline,
        )
        .await,
        tuliprox_hls::api::HlsTerminalResolution::Committed
    );
    let response = timeout(Duration::from_secs(12), poll(&playback, VIEWER)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let terminal = text(response).await;
    assert!(terminal.contains("/terminal/") && terminal.ends_with("#EXT-X-ENDLIST\n"));
    assert_ne!(terminal, body);
    proxy.freeze_clock_for_test(deadline - 1);
    assert_eq!(reserve_snapshot(&playback).await.remaining, 1);
    let origin_gets = playback.origin.manifest_request_count();
    barrier.resume.notify_one();
    let (status, received) = timeout(Duration::from_secs(2), pending).await.unwrap().unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(received, terminal);
    assert_eq!(playback.origin.manifest_request_count(), origin_gets);
    eprintln!("concurrent_terminal_commit=true b_status=200 returned_endlist=true reserve_ms=1");
}

async fn exhaust_provider_with_provisioning(playback: &Playback) {
    use shared::model::{PanelApiConfigDto, PanelApiQueryParamDto};

    let mut input = playback.input.clone();
    let mut panel = PanelApiConfigDto {
        enabled: true,
        url: format!("{}/panel", playback.origin.base_url),
        api_key: Some(Arc::from("local-test")),
        ..Default::default()
    };
    let api_key = PanelApiQueryParamDto { key: Arc::from("api_key"), value: Arc::from("auto") };
    panel.query_parameter.client_info = vec![
        api_key.clone(),
        PanelApiQueryParamDto { key: Arc::from("username"), value: Arc::from("auto") },
        PanelApiQueryParamDto { key: Arc::from("password"), value: Arc::from("auto") },
    ];
    panel.query_parameter.client_new =
        vec![api_key, PanelApiQueryParamDto { key: Arc::from("type"), value: Arc::from("m3u") }];
    input.panel_api = Some(crate::model::PanelApiConfig::from(&panel));
    let mut target = test_m3u_hls_share_target();
    target.name = "default".into();
    store_test_sources_with_target(&playback.app, input.clone(), target);
    super::super::super::owner_handoff::enable_hls_provisioning_custom_response(&playback.app);
    assert!(crate::api::panel_api::can_provision_on_exhausted(&playback.app, &input));

    let binding = playback.session.read().await.origin_account_binding.clone().unwrap();
    let provider = &playback.app.active_provider;
    provider.clear_identified_provider_reservation(&binding.session_owner, &input.name, binding.provider_binding_tag);
    provider.refresh_provider_reservation(&input.name, "retained-poll-foreign", 60);
    assert!(provider.confirm_playback_activity("retained-poll-foreign").is_some());
    let now = playback.app.hls.proxy.now_ms();
    let mut detached = binding;
    detached.detach(tuliprox_hls::api::HlsOriginAccountDetachedReason::Cleanup, now);
    playback.session.write().await.replace_origin_account_binding(Some(detached));
}

#[tokio::test]
async fn retained_poll_provider_exhaustion_does_not_publish_provisioning() {
    let (playback, body) = follow_window().await;
    fail_new_entry(&playback).await;
    exhaust_provider_with_provisioning(&playback).await;
    let original = playback.lease(VIEWER).await.last_manifest_snapshot;
    let (segments, next, attempt) = {
        let state = playback.session.read().await;
        (state.segments.keys().copied().collect::<Vec<_>>(), state.proxy_next_seq, state.origin_refresh.attempt.clone())
    };
    let proxy = &playback.app.hls.proxy;
    let due = playback.session.read().await.origin_refresh.next_fetch_allowed_at_ms;
    proxy.freeze_clock_for_test(proxy.now_ms().max(due));
    let gets = playback.origin.manifest_request_count();
    let (barrier, response) = poll_at_stage(&playback, HlsManifestPublicationStage::RetainedOriginExhausted);
    let (status, returned) = timeout(Duration::from_secs(2), response).await.unwrap().unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(returned, body);
    timeout(Duration::from_secs(2), barrier.prepared.notified()).await.unwrap();
    let state = playback.session.read().await;
    assert!(state.origin_account_binding.as_ref().unwrap().is_detached());
    assert_eq!(state.origin_refresh.attempt, attempt);
    assert_eq!(state.segments.keys().copied().collect::<Vec<_>>(), segments);
    assert_eq!(state.proxy_next_seq, next);
    drop(state);
    assert_eq!(playback.lease(VIEWER).await.last_manifest_snapshot, original);
    assert!(!playback.app.hls.provisioning.has_consumer(&playback.input.name, 12345, proxy.now_ms()));
    assert_eq!(playback.origin.manifest_request_count(), gets);
    barrier.resume.notify_one();
    eprintln!("provider_exhausted=true provisioning_enabled=true new_segments=0 lease_snapshot_unchanged=true");
}

async fn detach_provider_for_retained_poll(playback: &Playback) -> String {
    let proxy = &playback.app.hls.proxy;
    let mut session = playback.session.write().await;
    let mut binding = session.origin_account_binding.clone().unwrap();
    let owner = binding.session_owner.clone();
    playback.app.active_provider.clear_identified_provider_reservation(
        &owner,
        &binding.account_name,
        binding.provider_binding_tag,
    );
    binding.detach(tuliprox_hls::api::HlsOriginAccountDetachedReason::Cleanup, proxy.now_ms());
    session.replace_origin_account_binding(Some(binding));
    proxy.freeze_clock_for_test(proxy.now_ms().max(session.origin_refresh.next_fetch_allowed_at_ms));
    owner
}

async fn remove_viewers(playback: &Playback, viewers: &[&str]) {
    let proxy = &playback.app.hls.proxy;
    {
        let mut leases = proxy.access_leases().write().await;
        for viewer in viewers {
            assert!(leases.remove_access_lease(&tuliprox_hls::api::HlsAccessLeaseId((*viewer).into())).is_some());
        }
    }
    proxy
        .sync_session_access_lease_count_and_detach_if_needed(
            &playback.app.active_users,
            &playback.app.active_provider,
            &playback.session,
            &playback.proxy,
            proxy.now_ms(),
        )
        .await;
}

async fn complete_viewers(playback: &Playback, viewers: &[&str]) {
    let proxy = &playback.app.hls.proxy;
    for viewer in viewers {
        let id = tuliprox_hls::api::HlsAccessLeaseId((*viewer).into());
        proxy.remove_access_lease_for_test(&id).await;
        assert!(proxy.access_lease_response_snapshot(&id, &playback.proxy, proxy.now_ms()).await.is_none());
    }
    proxy
        .sync_session_access_lease_count_and_detach_if_needed(
            &playback.app.active_users,
            &playback.app.active_provider,
            &playback.session,
            &playback.proxy,
            proxy.now_ms(),
        )
        .await;
}

async fn assert_poll_before_origin_checkpoint(
    playback: &Playback,
    expected: &str,
    stage: HlsManifestPublicationStage,
) -> Arc<HlsManifestPublicationBarrier> {
    let (barrier, response) = poll_at_stage(playback, stage);
    let (status, body) = timeout(Duration::from_secs(2), response).await.unwrap().unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, expected);
    timeout(Duration::from_secs(2), barrier.prepared.notified()).await.unwrap();
    barrier
}

async fn assert_origin_work_released(playback: &Playback, owner: &str, gets: usize, count: usize) {
    let state = playback.session.read().await;
    assert_eq!(state.activity.active_access_lease_count, 0);
    assert!(state.origin_account_binding.as_ref().unwrap().is_detached());
    assert!(!state.origin_refresh.in_flight);
    assert_eq!(state.activity.active_origin_work_count.load(std::sync::atomic::Ordering::Acquire), 0);
    assert_eq!(playback.origin.manifest_request_count(), gets);
    assert_eq!(playback.app.active_provider.get_provider_connections_count(), count);
    assert!(playback.app.active_provider.binding_tag_for_owner(owner).is_none());
}

#[tokio::test]
async fn retained_poll_cannot_reacquire_after_last_viewer_leaves_during_prepare_lock() {
    let (playback, body) = follow_window().await;
    fail_new_entry(&playback).await;
    let owner = detach_provider_for_retained_poll(&playback).await;
    let count = playback.app.active_provider.get_provider_connections_count();
    let gets = playback.origin.manifest_request_count();
    let lock = Arc::clone(&playback.session.read().await.origin_account_prepare_lock);
    let guard = lock.lock().await;
    let barrier =
        assert_poll_before_origin_checkpoint(&playback, &body, HlsManifestPublicationStage::RetainedOriginPrepareLock)
            .await;
    remove_viewers(&playback, &[VIEWER, NEW_VIEWER, "warm"]).await;
    assert_origin_work_released(&playback, &owner, gets, count).await;
    drop(guard);
    timeout(Duration::from_secs(2), barrier.finished.notified()).await.unwrap();
    assert_origin_work_released(&playback, &owner, gets, count).await;
    eprintln!("prepare_lock_waited=true last_viewer_removed=true allocation=false origin_get=false");
}

#[tokio::test]
async fn retained_poll_releases_stale_provider_allocation_before_publication_or_refresh() {
    for stage in
        [HlsManifestPublicationStage::RetainedOriginAllocated, HlsManifestPublicationStage::RetainedOriginBeforeRefresh]
    {
        let (playback, body) = follow_window().await;
        fail_new_entry(&playback).await;
        let owner = detach_provider_for_retained_poll(&playback).await;
        let count = playback.app.active_provider.get_provider_connections_count();
        let gets = playback.origin.manifest_request_count();
        let barrier = assert_poll_before_origin_checkpoint(&playback, &body, stage).await;
        assert_eq!(playback.origin.manifest_request_count(), gets);
        assert_eq!(playback.app.active_provider.get_provider_connections_count(), count + 1);
        assert!(playback.app.active_provider.binding_tag_for_owner(&owner).is_some());
        assert_eq!(
            playback.session.read().await.origin_account_binding.as_ref().unwrap().is_active(),
            stage == HlsManifestPublicationStage::RetainedOriginBeforeRefresh,
        );
        remove_viewers(&playback, &[VIEWER, NEW_VIEWER, "warm"]).await;
        barrier.resume.notify_one();
        timeout(Duration::from_secs(2), barrier.finished.notified()).await.unwrap();
        assert_origin_work_released(&playback, &owner, gets, count).await;
        eprintln!("checkpoint={stage:?} stale_allocation_released=true origin_get=false");
    }
}

#[tokio::test]
async fn retained_refresh_stops_after_last_viewer_leaves_during_origin_get() {
    let (playback, body) = follow_window().await;
    fail_new_entry(&playback).await;
    let owner = detach_provider_for_retained_poll(&playback).await;
    let provider = &playback.app.active_provider;
    let before = provider.get_provider_connections_count();
    let release = assert_held_poll(&playback, &body, PROXY_AUTH).await;
    let gets = playback.origin.manifest_request_count();
    assert_eq!(provider.get_provider_connections_count(), before + 1);
    assert!(provider.binding_tag_for_owner(&owner).is_some());
    let attempt = playback.session.read().await.origin_refresh.attempt.clone().unwrap();
    let attempts = attempt.logical_attempts();
    let failures = playback.session.read().await.origin_refresh.consecutive_failures;
    complete_viewers(&playback, &[NEW_VIEWER, "warm", VIEWER]).await;
    assert_eq!(playback.session.read().await.activity.active_access_lease_count, 0);
    assert_eq!(playback.origin.manifest_request_count(), gets);
    assert!(!*release.borrow());
    finish_origin(&playback, &release).await;
    timeout(Duration::from_secs(2), async {
        while provider.get_provider_connections_count() != before {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_origin_work_released(&playback, &owner, gets, before).await;
    assert!(playback.session.read().await.origin_account_io_lease.is_none());
    assert_eq!(playback.session.read().await.origin_refresh.consecutive_failures, failures);
    assert_eq!(attempt.logical_attempts(), attempts);
    eprintln!(
        "last_viewer_removed=true gets_at_removal={gets} gets_after_release={} claims_after=0 provider_connections_after={before} retry_attempts_after={attempts}",
        playback.origin.manifest_request_count()
    );
}

#[tokio::test]
async fn retained_poll_leaves_in_flight_origin_work_available_to_remaining_viewer() {
    let (playback, body) = follow_window().await;
    fail_new_entry(&playback).await;
    let owner = detach_provider_for_retained_poll(&playback).await;
    let provider = &playback.app.active_provider;
    let before = provider.get_provider_connections_count();
    let release = assert_held_poll(&playback, &body, PROXY_AUTH).await;
    let gets = playback.origin.manifest_request_count();
    let binding = playback.session.read().await.origin_account_binding.clone().unwrap();
    // A second established viewer has the same live window and playback
    // position while the first viewer's Origin GET is already in flight.
    let mut other = playback.lease(VIEWER).await;
    other.lease_id = tuliprox_hls::api::HlsAccessLeaseId("retained-poll-other".into());
    playback.app.hls.proxy.prepare_access_lease(other).await;
    // The fixture leases initially share a user token. Hold the real initiating
    // lease until the others have been removed through the lifecycle path.
    complete_viewers(&playback, &[NEW_VIEWER, "warm", VIEWER]).await;
    assert_eq!(playback.session.read().await.activity.active_access_lease_count, 1);
    assert!(playback.session.read().await.origin_refresh.in_flight);
    let other = playback.lease("retained-poll-other").await;
    assert!(playback.reserve("retained-poll-other").await.guaranteed_reserve_ms > 0);
    assert_eq!(other.playback_mode, HlsLeasePlaybackMode::Live);
    assert_eq!(playback.origin.manifest_request_count(), gets);
    assert_eq!(provider.get_provider_connections_count(), before + 1);
    assert!(provider.binding_tag_for_owner(&owner).is_some());
    assert_eq!(
        playback.session.read().await.origin_account_binding.as_ref().unwrap().provider_binding_tag,
        binding.provider_binding_tag
    );
    finish_origin(&playback, &release).await;
    assert_eq!(
        playback.session.read().await.origin_control.path_condition,
        tuliprox_hls::api::HlsOriginPathCondition::RetryableFetchFailure,
    );
    let after = playback.origin.manifest_request_count();
    assert!(after > gets, "the remaining viewer authorizes retries on the shared refresh");
    assert_eq!(provider.get_provider_connections_count(), before);
    eprintln!("initiator_removed=true remaining_viewer=true shared_get_preserved=true extra_get_while_held=false gets_at_removal={gets} gets_after_release={after}");
}
