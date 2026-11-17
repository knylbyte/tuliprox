use super::*;
use crate::{
    media_reserve::{evaluate_lease_reserve, HlsLeaseReserveInput, HlsLeaseReserveSnapshot},
    recovery_timing::HlsRecoveryTriggerBudgetMs,
    HlsAccessLease, HlsAccessLeaseId, HlsAccessLeaseState, HlsPlaybackFamilyKey,
};

async fn install_clients(request: &OriginRefreshRequest) -> [HlsAccessLeaseId; 2] {
    let state = request.session.read().await;
    let ids = [HlsAccessLeaseId("continuity-early".into()), HlsAccessLeaseId("continuity-late".into())];
    for (seq, id) in (0..2).zip(&ids) {
        let mut lease = HlsAccessLease::pending(
            id.clone(),
            HlsPlaybackFamilyKey::new("fixture", &id.0),
            state.proxy_session_id.clone(),
            "fixture".into(),
            "fixture-token".into(),
            1,
            "12345".into(),
            1,
            request.now_ms,
            60_000,
        );
        lease.state = HlsAccessLeaseState::Activated;
        lease.pending_deadline = None;
        lease.active_until_ms = Some(request.now_ms + 60_000);
        lease.last_manifest_snapshot = Some(post_refresh_live_manifest_snapshot());
        let token = lease.playback_cursor.record_request_started(seq, request.now_ms);
        let token = lease.playback_cursor.bind_request_timing(token, &state.segments[&seq]);
        assert!(lease.playback_cursor.record_transfer_started(token, request.now_ms));
        request.hls_proxy.prepare_access_lease(lease).await;
    }
    ids
}

async fn reserves(request: &OriginRefreshRequest) -> Vec<(String, HlsLeaseReserveSnapshot)> {
    let state = request.session.read().await;
    let timeline = state.ready_timeline_snapshot(0, request.now_ms);
    let clients = request
        .hls_proxy
        .access_leases()
        .write()
        .await
        .active_live_playback_snapshots_for_session(&state.proxy_session_id, request.now_ms);
    let mut result = clients
        .into_iter()
        .map(|client| {
            let reserve = evaluate_lease_reserve(HlsLeaseReserveInput {
                manifest: client.last_manifest_snapshot.as_ref().unwrap(),
                cursor: &client.playback_cursor,
                ready_timeline: &timeline,
                now_ms: request.now_ms,
                recovery_trigger_budget: HlsRecoveryTriggerBudgetMs::from_millis(4_000),
                origin_path_degraded: true,
                recovery_committed: false,
            });
            (client.lease_id.0, reserve)
        })
        .collect::<Vec<_>>();
    result.sort_by(|left, right| left.0.cmp(&right.0));
    result
}

#[tokio::test]
async fn two_active_leases_share_discovered_anchor_but_keep_reserve_and_single_recovery_owner() {
    let gate = Arc::new(super::super::support::TestOriginResponseGate {
        request_number: 2,
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
    });
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let body = manifest("http://fixture.example", 101, 103).body;
    let origin = super::super::support::spawn_test_origin_with_response_gate(
        Arc::new(move |_| {
            if counter.fetch_add(1, Ordering::AcqRel) == 0 {
                (200, Vec::new(), body.clone())
            } else {
                (503, Vec::new(), String::new())
            }
        }),
        Some(Arc::clone(&gate)),
    )
    .await;
    let mut first = established(&origin.base_url).await;
    let ids = install_clients(&first).await;
    let mut second = first.clone();
    first.access_lease_id = Some(ids[0].clone());
    second.access_lease_id = Some(ids[1].clone());
    let shared = Arc::clone(&first.session);
    let anchor = HlsCacheOriginAnchor::capture(&*shared.read().await).unwrap();
    assert_eq!((anchor.proxy_seq, anchor.identity.origin_msn()), (3, 104));
    let before = reserves(&first).await;
    assert_eq!(before.len(), 2);
    assert_eq!((before[0].1.guaranteed_reserve_ms, before[1].1.guaranteed_reserve_ms), (8_000, 4_000));
    assert_ne!(before[0].1.cutover_required, before[1].1.cutover_required);
    let now_ms = first.now_ms;
    assert!(mark_origin_refresh_started(&mut first, now_ms).await);
    let task = tokio::spawn(refresh_and_commit(first, now_ms));
    tokio::time::timeout(std::time::Duration::from_secs(10), gate.entered.notified()).await.unwrap();
    let generation = shared.read().await.origin_control.acceptance_generation;
    assert!(shared.read().await.origin_control.acceptance_episode.is_some());
    assert_eq!(HlsCacheOriginAnchor::capture(&*second.session.read().await), Some(anchor));
    assert_eq!(
        mark_origin_refresh_started_with_outcome(&mut second, now_ms).await,
        HlsOriginRefreshTriggerOutcome::InFlight
    );
    assert_eq!(shared.read().await.origin_control.acceptance_generation, generation);
    assert_eq!(reserves(&second).await, before, "shared continuity is not a READY-reserve proof");
    {
        let state = shared.read().await;
        let mut leases = second.hls_proxy.access_leases().write().await;
        let identity =
            leases.response_snapshot(&ids[1], &state.proxy_session_id, now_ms).unwrap().media_identity().unwrap();
        assert!(leases
            .record_segment_request_started_if_identity_matches(&ids[1], &state.proxy_session_id, identity, 3, now_ms)
            .is_some());
    }
    assert_eq!(HlsCacheOriginAnchor::capture(&*shared.read().await), Some(anchor));
    assert_eq!(second.segment_worker_pool.metrics().snapshot().refresh_started, 1);
    gate.release.notify_one();
    task.await.unwrap();
    let state = shared.read().await;
    assert!(!state.origin_refresh.in_flight);
    assert_eq!(state.origin_control.acceptance_generation, generation);
    assert_eq!(state.origin_control.acceptance_episode.as_ref().unwrap().full_bursts_completed, 1);
}
