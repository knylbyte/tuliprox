use super::{
    get_response, manifest, manifest_media_sequence, refresh_playing_lease_with_directive_for, response_body, text,
    HlsManifestAcceptanceDirective, MediaShape, Playback, SegmentCacheStatus, FIRST,
};
use axum::http::StatusCode;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::sync::watch;
use tuliprox_core::utils::current_time_millis;
use tuliprox_hls::{api::HlsLeasePlaybackMode, SegmentRevisionGuard};
use tuliprox_mpegts::transport_stream_buffer::{
    HlsFiniteTsRenderSpec, HlsTsTimestampProfileScanner, TransportStreamBuffer,
};

pub(super) const TARGET: u64 = 10;
const TICKS: u64 = TARGET * 90_000;
const VIEWER: &str = "forward-jump-b";
const DISCONTINUITY: &str = "#EXT-X-DISCONTINUITY";

pub(super) fn transport_fixture() -> TransportStreamBuffer {
    TransportStreamBuffer::new(
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../test/fixtures/hls/channel_unavailable.ts")).to_vec(),
    )
}

pub(super) fn media(asset: &TransportStreamBuffer, msn: u64) -> Vec<u8> {
    asset
        .render_finite_hls_segment(HlsFiniteTsRenderSpec {
            timestamp_offset_ticks_90khz: (msn - FIRST) * TICKS,
            continuity_seed: 0,
            logical_segment_index: u16::try_from(msn - FIRST).unwrap(),
        })
        .unwrap()
        .to_vec()
}

pub(super) async fn assert_media(playback: &Playback, uri: &str, msn: u64) -> u64 {
    let response = get_response(Arc::clone(&playback.app), uri, None).await;
    assert_eq!(response.status(), StatusCode::OK, "{uri}");
    let bytes = response_body(response).await;
    assert_eq!(bytes.as_ref(), media(&transport_fixture(), msn), "{uri} must serve origin MSN {msn}");
    let mut profile = HlsTsTimestampProfileScanner::new(TICKS);
    let (packets, remainder) = bytes.as_chunks::<188>();
    assert_eq!(remainder, [0_u8; 0]);
    for packet in packets {
        profile.push_aligned_packet(packet);
    }
    let timestamps = profile.finish().unwrap();
    assert!(timestamps.observed_pts_or_dts && timestamps.observed_pcr);
    let first = timestamps.first_clock_90khz;
    eprintln!("media_status=200 origin_msn={msn} first_clock_90khz={first} uri={uri}");
    first
}

struct Window {
    head: u64,
    discontinuity_sequence: u64,
    boundaries: Vec<u64>,
    segments: BTreeMap<u64, (String, u64)>,
}

impl Window {
    async fn request(playback: &Playback, lease: &str) -> Self {
        let response = playback.request(lease).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = text(response).await;
        assert!(!body.contains("#EXT-X-ENDLIST"));
        let head = manifest_media_sequence(&body);
        let discontinuity_sequence = body
            .lines()
            .find_map(|line| line.strip_prefix("#EXT-X-DISCONTINUITY-SEQUENCE:"))
            .unwrap()
            .parse::<u64>()
            .unwrap();
        let mut number = discontinuity_sequence;
        let mut segments = BTreeMap::new();
        let mut boundaries = Vec::new();
        for line in body.lines() {
            let seq = head + u64::try_from(segments.len()).unwrap();
            if line == DISCONTINUITY {
                boundaries.push(seq);
                number += 1;
            } else if line.starts_with("/hls/") {
                assert!(line.contains(lease) && line.ends_with(&format!("/{seq:06}.ts")), "{line}");
                segments.insert(seq, (line.to_owned(), number));
            }
        }
        eprintln!("proxy_playlist lease={lease} status=200 head={head} discontinuity_sequence={discontinuity_sequence} tags={boundaries:?}\n{body}");
        Self { head, discontinuity_sequence, boundaries, segments }
    }

    fn assert_range(&self, first: u64, last: u64) {
        assert_eq!(self.head, first);
        assert_eq!(self.segments.keys().copied().collect::<Vec<_>>(), (first..=last).collect::<Vec<_>>());
    }

    fn assert_overlap(&self, next: &Self) {
        let mut overlap = 0;
        for (seq, previous) in &self.segments {
            if let Some(current) = next.segments.get(seq) {
                assert_eq!(current, previous, "URI and discontinuity number at proxy sequence {seq}");
                overlap += 1;
            }
        }
        assert!(overlap > 0);
    }

    fn uri(&self, seq: u64) -> &str { &self.segments[&seq].0 }
}

async fn advance(playback: &Playback, first_msn: u64, first_proxy: u64, marked_msn: Option<u64>, lease: &str) {
    accept_window(playback, first_msn, first_proxy, marked_msn, lease).await;
    let count = playback.script.lock().unwrap().segment_count;
    wait_for_segment(playback, |state| {
        (first_proxy + count.saturating_sub(6)..first_proxy + count).all(|seq| {
            state.segments.get(&seq).is_some_and(|entry| matches!(entry.status, SegmentCacheStatus::Ready { .. }))
        })
    })
    .await;
}

async fn accept_window(playback: &Playback, first_msn: u64, first_proxy: u64, marked_msn: Option<u64>, lease: &str) {
    let accepted = playback.session.read().await.origin_manifest_commits();
    let requests = playback.origin.manifest_request_count();
    let count = {
        let mut script = playback.script.lock().unwrap();
        script.head = first_msn;
        script.discontinuity_at = marked_msn;
        script.manifest_status = StatusCode::OK;
        script.segment_count
    };
    playback.app.hls.proxy.freeze_clock_for_test(current_time_millis());
    playback.session.write().await.origin_refresh.next_fetch_allowed_at_ms = 0;
    refresh_playing_lease_with_directive_for(
        &playback.app,
        &playback.session,
        &playback.url,
        lease,
        HlsManifestAcceptanceDirective::none(),
    )
    .await;
    let mut state = playback.session.write().await;
    assert_eq!(state.origin_manifest_commits(), accepted + 1);
    assert_eq!(state.origin_epoch, 0, "the same origin sequence space remains active");
    assert_eq!(state.origin_seq_highwater, Some(first_msn + count - 1));
    for offset in 0..count {
        assert_eq!(state.segments[&(first_proxy + offset)].origin_key.host_local_sequence, first_msn + offset);
    }
    state.origin_refresh.next_fetch_allowed_at_ms = u64::MAX;
    assert!(playback.origin.manifest_request_count() > requests);
    eprintln!(
        "accepted_origin first_msn={first_msn} first_proxy={first_proxy} origin_commits={} origin_gets_before={requests} origin_gets_after={}\n{}",
        state.origin_manifest_commits(),
        playback.origin.manifest_request_count(),
        manifest(first_msn, count, MediaShape::Timestamped, marked_msn),
    );
}

async fn wait_for_segment(playback: &Playback, ready: impl Fn(&tuliprox_hls::api::HlsSession) -> bool) {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if ready(&*playback.session.read().await) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the fixture must reach the expected cache state");
}

async fn bound_revision(playback: &Playback, seq: u64) -> SegmentRevisionGuard {
    let lease = playback.lease(VIEWER).await;
    playback.app.hls.proxy.access_leases().read().await.published_segment_revision(&lease.lease_id, seq).unwrap()
}

async fn exercise_recovery(gap: u64, origin_marked: bool) {
    super::super::super::origin_restart::trace_fixture();
    let playback = Playback::with_media(MediaShape::Timestamped).await;
    advance(&playback, FIRST + 3, 3, None, "warm").await;
    playback.prepare_lease(VIEWER).await;
    let entry = Window::request(&playback, VIEWER).await;
    assert_media(&playback, entry.uri(entry.head), FIRST + entry.head).await;
    let before = Window::request(&playback, VIEWER).await;
    before.assert_range(3, 8);
    assert_eq!(before.discontinuity_sequence, 0);
    assert_eq!(before.boundaries, [0_u64; 0]);
    let old_revision = bound_revision(&playback, 8).await;
    let old_clock = assert_media(&playback, before.uri(8), FIRST + 8).await;
    let old_commits = playback.session.read().await.origin_manifest_commits();
    playback.retry_gap_with_status(StatusCode::PROXY_AUTHENTICATION_REQUIRED).await;
    assert_eq!(playback.session.read().await.origin_manifest_commits(), old_commits);
    assert!(playback.reserve(VIEWER).await.guaranteed_reserve_ms > 0);
    assert!(matches!(playback.lease(VIEWER).await.playback_mode, HlsLeasePlaybackMode::Live));

    let new_msn = FIRST + 9 + gap;
    let origin_tag = origin_marked.then_some(new_msn);
    advance(&playback, new_msn, 9, origin_tag, VIEWER).await;
    if gap > 0 {
        let state = playback.session.read().await;
        assert!(!state.segments.values().any(|entry| entry.origin_key.host_local_sequence == FIRST + 9));
    }
    let recovered = Window::request(&playback, VIEWER).await;
    recovered.assert_range(9, 14);
    let new_clock = assert_media(&playback, recovered.uri(9), new_msn).await;
    let gap_ticks = new_clock - old_clock - TICKS;
    assert_eq!(gap_ticks, gap * TICKS);
    assert_ne!(recovered.uri(9), before.uri(8));
    assert_eq!(bound_revision(&playback, 8).await, old_revision);
    assert_eq!(assert_media(&playback, before.uri(8), FIRST + 8).await, old_clock);
    eprintln!("recovery_boundary previous_proxy=8 previous_origin_msn={} new_proxy=9 new_origin_msn={new_msn} gap_90khz={gap_ticks} origin_tag={origin_marked} proxy_tags={:?}", FIRST + 8, recovered.boundaries);
    assert_eq!(recovered.discontinuity_sequence, 0);
    let expected_tags = if gap > 0 { &[9][..] } else { &[][..] };
    assert_eq!(recovered.boundaries, expected_tags);

    let repeat = Window::request(&playback, VIEWER).await;
    repeat.assert_range(9, 14);
    assert_eq!(repeat.boundaries, expected_tags);
    assert_eq!(repeat.discontinuity_sequence, 0);
    recovered.assert_overlap(&repeat);
    for slide in 1..=2 {
        advance(&playback, new_msn + slide, 9 + slide, origin_tag, VIEWER).await;
        let next = Window::request(&playback, VIEWER).await;
        next.assert_range(9 + slide, 14 + slide);
        assert_eq!(next.boundaries, [0_u64; 0]);
        assert_eq!(next.discontinuity_sequence, gap);
        recovered.assert_overlap(&next);
    }
}

#[tokio::test]
async fn forward_jump_marks_first_recovered_segment_and_rolls_its_discontinuity_sequence() {
    exercise_recovery(1, false).await;
}

#[tokio::test]
async fn forward_jump_after_origin_error_leaves_contiguous_media_unmarked() { exercise_recovery(0, false).await; }

#[tokio::test]
async fn forward_jump_preserves_one_origin_discontinuity_at_the_same_boundary() { exercise_recovery(1, true).await; }

#[tokio::test]
async fn forward_jump_long_first_ready_window_marks_its_first_visible_segment() {
    super::super::super::origin_restart::trace_fixture();
    let playback = Playback::with_media(MediaShape::Timestamped).await;
    advance(&playback, FIRST + 3, 3, None, "warm").await;
    playback.prepare_lease(VIEWER).await;
    let entry = Window::request(&playback, VIEWER).await;
    assert_media(&playback, entry.uri(entry.head), FIRST + entry.head).await;
    let old = Window::request(&playback, VIEWER).await;
    old.assert_range(3, 8);
    let old_clock = assert_media(&playback, old.uri(8), FIRST + 8).await;
    let old_revision = bound_revision(&playback, 8).await;
    playback.retry_gap_with_status(StatusCode::PROXY_AUTHENTICATION_REQUIRED).await;

    playback.script.lock().unwrap().segment_count = 8;
    advance(&playback, FIRST + 10, 9, None, VIEWER).await;
    let recovered = Window::request(&playback, VIEWER).await;
    recovered.assert_range(11, 16);
    assert_eq!(recovered.discontinuity_sequence, 0);
    assert_eq!(recovered.boundaries, [11]);
    assert_eq!(assert_media(&playback, recovered.uri(11), FIRST + 12).await - old_clock, 4 * TICKS);
    assert_eq!(bound_revision(&playback, 8).await, old_revision);
    recovered.assert_overlap(&Window::request(&playback, VIEWER).await);

    advance(&playback, FIRST + 11, 10, None, VIEWER).await;
    let slid = Window::request(&playback, VIEWER).await;
    slid.assert_range(12, 17);
    assert_eq!(slid.discontinuity_sequence, 1);
    assert_eq!(slid.boundaries, [0_u64; 0]);
    recovered.assert_overlap(&slid);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MappedTail {
    PermanentFailure,
    LatePermanentFailure,
    PendingFetch,
    ReadyOverlap,
}

impl MappedTail {
    fn marks_recovery(self) -> bool { matches!(self, Self::PermanentFailure | Self::LatePermanentFailure) }
}

async fn exercise_mapped_tail(tail: MappedTail) {
    super::super::super::origin_restart::trace_fixture();
    let playback = Playback::with_media(MediaShape::Timestamped).await;
    advance(&playback, FIRST + 3, 3, None, "warm").await;
    playback.prepare_lease(VIEWER).await;
    let entry = Window::request(&playback, VIEWER).await;
    assert_media(&playback, entry.uri(entry.head), FIRST + entry.head).await;
    let old = Window::request(&playback, VIEWER).await;
    old.assert_range(3, 8);
    let old_clock = assert_media(&playback, old.uri(8), FIRST + 8).await;
    let old_revision = bound_revision(&playback, 8).await;

    let (release, gate) = watch::channel(false);
    let (release_successors, successor_gate) = watch::channel(false);
    let tail_msn = FIRST + 9;
    {
        let mut script = playback.script.lock().unwrap();
        match tail {
            MappedTail::PermanentFailure => script.missing_media = Some(tail_msn),
            MappedTail::LatePermanentFailure => {
                script.missing_media = Some(tail_msn);
                script.missing_media_gate = Some(gate);
            }
            MappedTail::PendingFetch => script.successor_gate = Some(gate),
            MappedTail::ReadyOverlap => {}
        }
    }
    accept_window(&playback, FIRST + 4, 4, None, VIEWER).await;
    wait_for_segment(&playback, |state| {
        let status = &state.segments[&9].status;
        match tail {
            MappedTail::PermanentFailure => {
                matches!(status, SegmentCacheStatus::FailedPermanent { status: Some(StatusCode::NOT_FOUND), .. })
            }
            MappedTail::PendingFetch | MappedTail::LatePermanentFailure => {
                matches!(status, SegmentCacheStatus::Fetching { .. })
            }
            MappedTail::ReadyOverlap => matches!(status, SegmentCacheStatus::Ready { .. }),
        }
    })
    .await;
    let old_overlap = if tail == MappedTail::ReadyOverlap {
        let window = Window::request(&playback, VIEWER).await;
        window.assert_range(4, 9);
        assert_eq!(assert_media(&playback, window.uri(9), tail_msn).await, old_clock + TICKS);
        Some((window, bound_revision(&playback, 9).await))
    } else {
        let state = playback.session.read().await;
        assert_eq!(state.last_rendered_manifest.as_ref().unwrap().last_proxy_seq, 8);
        None
    };

    playback.retry_gap_with_status(StatusCode::PROXY_AUTHENTICATION_REQUIRED).await;
    let first_proxy = if tail == MappedTail::ReadyOverlap { 9 } else { 10 };
    let first_msn = FIRST + first_proxy;
    playback.script.lock().unwrap().successor_gate =
        (tail == MappedTail::LatePermanentFailure).then_some(successor_gate);
    accept_window(&playback, first_msn, first_proxy, None, VIEWER).await;
    if matches!(tail, MappedTail::PendingFetch | MappedTail::LatePermanentFailure) {
        assert!(matches!(playback.session.read().await.segments[&9].status, SegmentCacheStatus::Fetching { .. }));
        release.send(true).unwrap();
    }
    if tail == MappedTail::LatePermanentFailure {
        wait_for_segment(&playback, |state| {
            matches!(
                state.segments[&9].status,
                SegmentCacheStatus::FailedPermanent { status: Some(StatusCode::NOT_FOUND), .. }
            )
        })
        .await;
        assert!(playback.session.read().await.segments[&first_proxy].last_rendered_at_ms.is_none());
        release_successors.send(true).unwrap();
    }
    wait_for_segment(&playback, |state| {
        (first_proxy..first_proxy + 6)
            .all(|seq| matches!(state.segments[&seq].status, SegmentCacheStatus::Ready { .. }))
    })
    .await;
    let recovered = Window::request(&playback, VIEWER).await;
    recovered.assert_range(first_proxy, first_proxy + 5);
    assert_eq!(recovered.discontinuity_sequence, 0);
    assert_eq!(
        assert_media(&playback, recovered.uri(first_proxy), first_msn).await - old_clock,
        (first_proxy - 8) * TICKS,
    );
    assert_eq!(bound_revision(&playback, 8).await, old_revision);
    if let Some((previous, revision)) = old_overlap {
        previous.assert_overlap(&recovered);
        assert_eq!(bound_revision(&playback, 9).await, revision);
    }
    eprintln!(
        "mapped_tail={tail:?} mapped_msn={tail_msn} mapped_proxy=9 recovered_msn={first_msn} recovered_proxy={first_proxy} tags={:?}",
        recovered.boundaries,
    );
    let expected = if tail.marks_recovery() { &[10][..] } else { &[][..] };
    assert_eq!(recovered.boundaries, expected);
    recovered.assert_overlap(&Window::request(&playback, VIEWER).await);
}

#[tokio::test]
async fn mapped_permanent_404_marks_the_recovered_window() { exercise_mapped_tail(MappedTail::PermanentFailure).await; }

#[tokio::test]
async fn mapped_404_confirmed_after_origin_commit_marks_the_recovered_window() {
    exercise_mapped_tail(MappedTail::LatePermanentFailure).await;
}

#[tokio::test]
async fn mapped_pending_fetch_leaves_the_recovered_window_unmarked() {
    exercise_mapped_tail(MappedTail::PendingFetch).await;
}

#[tokio::test]
async fn mapped_ready_overlap_keeps_its_publication_without_a_marker() {
    exercise_mapped_tail(MappedTail::ReadyOverlap).await;
}
