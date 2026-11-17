use super::super::access_lease;
use crate::{
    api::{
        derive_hls_lease_manifest_snapshot, CachedSegmentMetadata, HlsAccessLeaseId, HlsAccessLeaseState,
        HlsLeaseManifestPublicationOutcome as Outcome, HlsLeaseManifestPublicationRejectReason as Reject,
        HlsLeaseManifestSnapshot, HlsLeaseManifestSnapshotInput, HlsLiveWindowView, HlsManifestRenderer,
        HlsPlaybackFamilyKey, HlsProxyManager, HlsSession, HlsSessionKey, RenderError, SegmentCacheStatus,
    },
    SegmentRevisionKind,
};
use shared::model::{HlsCacheConfigDto, HlsStartupMode, HlsStripMode, Secs};
use std::{fmt::Write as _, ops::RangeInclusive};
use tuliprox_core::model::HlsCacheConfig;
use tuliprox_parser::hls::origin_manifest::{
    parse_origin_media_manifest, OriginManifestParseOutcome, ParsedOriginManifest,
};

const NOW: u64 = 2_000;

fn manifest(first: u64, count: u64) -> ParsedOriginManifest {
    let mut body = format!("#EXTM3U\n#EXT-X-TARGETDURATION:10\n#EXT-X-MEDIA-SEQUENCE:{first}\n");
    for msn in first..first + count {
        writeln!(body, "#EXTINF:10,\n{msn}.ts").unwrap();
    }
    let OriginManifestParseOutcome::Normal(parsed) =
        parse_origin_media_manifest(&body, "https://origin.example/live.m3u8")
    else {
        panic!("expected media playlist");
    };
    parsed
}

fn mark_ready(state: &mut HlsSession, range: RangeInclusive<u64>) {
    let startup = state.startup.as_mut().unwrap();
    for (&seq, entry) in state.segments.range_mut(range) {
        entry.status = SegmentCacheStatus::Ready { content_length: 1, ready_at_ms: NOW };
        let revision =
            startup.store.create(state.proxy_session_id.clone(), seq, SegmentRevisionKind::Processed).unwrap();
        assert!(revision.revision().complete(CachedSegmentMetadata { path: "fixture.ts".into(), size: 1 }));
        startup.revisions.insert(seq, revision);
    }
}

fn snapshot(state: &HlsSession, view: HlsLiveWindowView) -> Result<HlsLeaseManifestSnapshot, RenderError> {
    let plan = state.plan_live_window(view).unwrap();
    let rendered = HlsManifestRenderer::render_live_window(state, &plan, NOW)?;
    Ok(derive_hls_lease_manifest_snapshot(
        &HlsLeaseManifestSnapshotInput::FastStartWindow {
            session: state,
            plan: &plan,
            rendered: &rendered,
            materialized_body: &rendered.body,
        },
        NOW,
    )
    .unwrap()
    .unwrap())
}

fn numbering(snapshot: &HlsLeaseManifestSnapshot) -> Vec<(u64, u64)> {
    let mut number = snapshot.discontinuity_sequence;
    snapshot
        .visible_segments
        .iter()
        .map(|unit| {
            number += u64::from(unit.discontinuity_before);
            (unit.proxy_seq, number)
        })
        .collect()
}

#[derive(Clone, Copy, Debug)]
enum Order {
    ActiveFirst,
    EntryFirst,
    EntryBeforeActiveReady,
}

#[derive(Clone, Copy, Debug)]
enum OriginTag {
    None,
    AtGap,
    AfterGap,
}

#[allow(clippy::too_many_lines, reason = "Keep preparation and the two publication commits in execution order.")]
async fn exercise(order: Order, origin_tag: OriginTag, failed_prefix: Option<u64>) {
    let expected_number = match origin_tag {
        OriginTag::None | OriginTag::AtGap => 1,
        OriginTag::AfterGap => 2,
    };
    let cache = tempfile::tempdir().unwrap();
    let mut config =
        HlsCacheConfigDto { cache_path: Some(cache.path().to_string_lossy().into_owned()), ..Default::default() };
    config.startup.mode = HlsStartupMode::FirstReady;
    config.strip.value = 2;
    let manager = HlsProxyManager::with_hls_cache_config(&HlsCacheConfig::from(&config));
    manager.freeze_clock_for_test(NOW);
    let session = manager.get_or_create_session(HlsSessionKey::new(1, "recovery-publication"), b"fixture", NOW).await;
    let proxy = session.read().await.proxy_session_id.clone();
    let active = HlsAccessLeaseId("established".into());
    let joining = HlsAccessLeaseId("joining".into());
    {
        let mut leases = manager.access_leases().write().await;
        for id in [&active, &joining] {
            let mut lease = access_lease(&id.0, &proxy);
            lease.family_key = HlsPlaybackFamilyKey::new("alice", &id.0);
            if *id == active {
                lease.state = HlsAccessLeaseState::Activated;
                lease.active_until_ms = Some(NOW + 30_000);
            }
            leases.prepare_access_lease(lease);
        }
    }
    let active_view = HlsLiveWindowView::Active { first_published: 0 };
    let (old, old_entry) = {
        let mut state = session.write().await;
        state.live_window_strip = manager.strip();
        state.apply_origin_manifest(&manifest(100, 6)).unwrap();
        mark_ready(&mut state, 0..=5);
        state.render_and_store_manifest(NOW).unwrap();
        (snapshot(&state, active_view).unwrap(), state.segments[&5].clone())
    };
    assert_eq!(numbering(&old), (0..=5).map(|seq| (seq, 0)).collect::<Vec<_>>());
    {
        let mut leases = manager.access_leases().write().await;
        let guard = leases.prepare_manifest_publication(&active, &proxy, NOW).unwrap();
        assert!(leases.commit_manifest_publication(&active, &proxy, guard, old, NOW).is_committed());
    }

    let window_leases = if failed_prefix.is_some() {
        let mut leases = Vec::new();
        for id in [&active, &joining] {
            leases.push(manager.access_lease_response_snapshot(id, &proxy, NOW).await.unwrap());
        }
        leases
    } else {
        Vec::new()
    };
    let (mut active_candidate, entry_candidate) = {
        let mut state = session.write().await;
        let mut recovered = manifest(if failed_prefix.is_some() { 106 } else { 107 }, 8);
        match origin_tag {
            OriginTag::None => {}
            OriginTag::AtGap => recovered.segments[0].discontinuity_before = true,
            OriginTag::AfterGap => recovered.segments[2].discontinuity_before = true,
        }
        state.apply_origin_manifest(&recovered).unwrap();
        if let Some(seq) = failed_prefix {
            state.segments.get_mut(&seq).unwrap().status = SegmentCacheStatus::FailedPermanent {
                failed_at_ms: NOW,
                status: Some(axum::http::StatusCode::NOT_FOUND),
            };
        }
        if matches!(order, Order::EntryBeforeActiveReady) {
            mark_ready(&mut state, 9..=11);
            assert_eq!(snapshot(&state, active_view), Err(RenderError::NoRenderableWindow));
        } else {
            mark_ready(&mut state, if failed_prefix.is_some() { 8..=13 } else { 6..=13 });
        }
        for lease in &window_leases {
            state.plan_live_window_for_lease(lease).unwrap();
        }
        (snapshot(&state, active_view).ok(), snapshot(&state, HlsLiveWindowView::Entry).unwrap())
    };
    let active_guard = manager.prepare_access_lease_manifest_publication(&active, &proxy, NOW).await.unwrap();
    let entry_guard = manager.prepare_access_lease_manifest_publication(&joining, &proxy, NOW).await.unwrap();
    assert_eq!(entry_candidate.first_proxy_seq, 9);
    let (first_id, first_guard, first, second_id, second_guard, prepared_second, second_view) =
        if matches!(order, Order::ActiveFirst) {
            (
                &active,
                active_guard,
                active_candidate.take().unwrap(),
                &joining,
                entry_guard,
                Some(entry_candidate),
                HlsLiveWindowView::Entry,
            )
        } else {
            (&joining, entry_guard, entry_candidate, &active, active_guard, active_candidate, active_view)
        };
    assert!(manager
        .commit_access_lease_manifest_publication(first_id, &proxy, first_guard, first.clone(), NOW)
        .await
        .is_committed());
    let already_committed = if let Some(prepared) = prepared_second {
        let result = manager
            .commit_access_lease_manifest_publication(second_id, &proxy, second_guard, prepared.clone(), NOW)
            .await;
        assert!(result.is_committed() || result == Outcome::Rejected(Reject::SnapshotChanged), "{result:?}");
        if result.is_committed() {
            assert!(numbering(&prepared).iter().all(|&(_, number)| number == expected_number));
        }
        result.is_committed()
    } else {
        let mut state = session.write().await;
        mark_ready(&mut state, if failed_prefix.is_some() { 8..=8 } else { 6..=8 });
        mark_ready(&mut state, 12..=13);
        assert_eq!(state.render_and_store_manifest(NOW).unwrap().first_proxy_seq, 8);
        false
    };

    let second = snapshot(&*session.read().await, second_view).unwrap();
    eprintln!(
        "recovery order={order:?} origin_tag={origin_tag:?} failed_prefix={failed_prefix:?} first={:?} second={:?}",
        numbering(&first),
        numbering(&second)
    );
    assert!(numbering(&first).iter().all(|&(_, number)| number == expected_number));
    assert!(numbering(&second).iter().all(|&(_, number)| number == expected_number));
    if !already_committed {
        let guard = manager.prepare_access_lease_manifest_publication(second_id, &proxy, NOW).await.unwrap();
        let result =
            manager.commit_access_lease_manifest_publication(second_id, &proxy, guard, second.clone(), NOW).await;
        assert!(result.is_committed(), "{result:?}");
    }
    {
        let state = session.read().await;
        let first_view = if matches!(order, Order::ActiveFirst) { active_view } else { HlsLiveWindowView::Entry };
        for (view, published) in [(first_view, first), (second_view, second)] {
            let repeated = snapshot(&state, view).unwrap();
            assert_eq!(numbering(&repeated), numbering(&published));
            assert_eq!(
                repeated.cached_source.as_ref().unwrap().bindings,
                published.cached_source.as_ref().unwrap().bindings
            );
            for (unit, original) in repeated.visible_segments.iter().zip(published.visible_segments.iter()) {
                assert_eq!(unit.uri, original.uri);
            }
        }
        assert_eq!(state.origin_discontinuity_before(&state.segments[&8]), matches!(origin_tag, OriginTag::AfterGap));
        assert_eq!(state.origin_discontinuity_before(&state.segments[&6]), matches!(origin_tag, OriginTag::AtGap));
    }
    assert_eq!(session.read().await.segments[&5], old_entry);
    {
        let leases = manager.access_leases().read().await;
        for seq in 9..=11 {
            assert_eq!(
                leases.published_segment_revision(&active, seq),
                leases.published_segment_revision(&joining, seq)
            );
        }
    }
    let state = session.read().await;
    let first_msn = if failed_prefix.is_some() { 108 } else { 109 };
    assert_eq!(state.segments[&8].origin_key.host_local_sequence, first_msn);
    assert_eq!(state.segments[&9].origin_key.host_local_sequence, first_msn + 1);
}

#[tokio::test]
async fn active_then_entry_keeps_recovery_numbers() {
    for tag in [OriginTag::None, OriginTag::AtGap, OriginTag::AfterGap] {
        exercise(Order::ActiveFirst, tag, None).await;
    }
}

#[tokio::test]
async fn entry_then_active_keeps_recovery_numbers() {
    for tag in [OriginTag::None, OriginTag::AtGap, OriginTag::AfterGap] {
        exercise(Order::EntryFirst, tag, None).await;
    }
}

#[tokio::test]
async fn entry_before_active_head_is_ready_keeps_recovery_numbers() {
    for tag in [OriginTag::None, OriginTag::AtGap, OriginTag::AfterGap] {
        exercise(Order::EntryBeforeActiveReady, tag, None).await;
    }
}

#[tokio::test]
async fn failed_first_ready_prefix_keeps_numbers_across_lease_views() {
    for order in [Order::ActiveFirst, Order::EntryFirst, Order::EntryBeforeActiveReady] {
        for failed in [6, 7] {
            exercise(order, OriginTag::None, Some(failed)).await;
        }
    }
}

async fn commit_window(
    manager: &HlsProxyManager,
    state: &crate::HlsSessionHandle,
    id: &HlsAccessLeaseId,
) -> HlsLeaseManifestSnapshot {
    let proxy = state.read().await.proxy_session_id.clone();
    let guard = manager.prepare_access_lease_manifest_publication(id, &proxy, NOW).await.unwrap();
    let lease = manager.access_lease_response_snapshot(id, &proxy, NOW).await.unwrap();
    let candidate = {
        let mut state = state.write().await;
        let plan = state.plan_live_window_for_lease(&lease).unwrap();
        let rendered = HlsManifestRenderer::render_live_window(&state, &plan, NOW).unwrap();
        derive_hls_lease_manifest_snapshot(
            &HlsLeaseManifestSnapshotInput::FastStartWindow {
                session: &state,
                plan: &plan,
                rendered: &rendered,
                materialized_body: &rendered.body,
            },
            NOW,
        )
        .unwrap()
        .unwrap()
    };
    let outcome = manager.commit_access_lease_manifest_publication(id, &proxy, guard, candidate.clone(), NOW).await;
    assert!(outcome.is_committed(), "{outcome:?}");
    candidate
}

#[tokio::test]
#[allow(clippy::too_many_lines, reason = "Keep the publication, slide and collection checks in fixture order.")]
async fn separated_first_ready_gaps_are_stable_in_both_publication_orders() {
    for entry_first in [false, true] {
        for (first_404, origin_tag) in [(false, false), (true, false), (true, true)] {
            let cache = tempfile::tempdir().unwrap();
            let mut config = HlsCacheConfigDto {
                cache_path: Some(cache.path().to_string_lossy().into_owned()),
                cache_duration: Secs::new(1),
                ..Default::default()
            };
            config.startup.mode = HlsStartupMode::FirstReady;
            config.strip.mode = HlsStripMode::Segments;
            config.strip.value = 8;
            let manager = HlsProxyManager::with_hls_cache_config(&HlsCacheConfig::from(&config));
            manager.freeze_clock_for_test(NOW);
            let session = manager.get_or_create_session(HlsSessionKey::new(1, "separated-gaps"), b"fixture", NOW).await;
            let proxy = session.read().await.proxy_session_id.clone();
            let active = HlsAccessLeaseId("established".into());
            let joining = HlsAccessLeaseId("joining".into());
            {
                let mut leases = manager.access_leases().write().await;
                for id in [&active, &joining] {
                    let mut lease = access_lease(&id.0, &proxy);
                    lease.family_key = HlsPlaybackFamilyKey::new("alice", &id.0);
                    if *id == active {
                        lease.state = HlsAccessLeaseState::Activated;
                        lease.active_until_ms = Some(NOW + 30_000);
                    }
                    leases.prepare_access_lease(lease);
                }
            }
            let (initial, old_binding) = {
                let mut state = session.write().await;
                state.live_window_strip = manager.strip();
                state.apply_origin_manifest(&manifest(100, 6)).unwrap();
                mark_ready(&mut state, 0..=5);
                state.render_and_store_manifest(NOW).unwrap();
                let initial = snapshot(&state, HlsLiveWindowView::Active { first_published: 0 }).unwrap();
                (initial, state.segments[&5].clone())
            };
            {
                let mut leases = manager.access_leases().write().await;
                let guard = leases.prepare_manifest_publication(&active, &proxy, NOW).unwrap();
                assert!(leases.commit_manifest_publication(&active, &proxy, guard, initial, NOW).is_committed());
            }
            {
                let mut state = session.write().await;
                let mut recovery = manifest(106, 12);
                recovery.segments[5].discontinuity_before = origin_tag;
                state.apply_origin_manifest(&recovery).unwrap();
                for seq in [6, 10] {
                    state.segments.get_mut(&seq).unwrap().status = if seq == 10 || first_404 {
                        SegmentCacheStatus::FailedPermanent {
                            status: Some(axum::http::StatusCode::NOT_FOUND),
                            failed_at_ms: NOW,
                        }
                    } else {
                        SegmentCacheStatus::FailedRetryable { failed_at_ms: NOW, retry_after_ms: 1_000 }
                    };
                }
                mark_ready(&mut state, 7..=9);
                mark_ready(&mut state, 11..=17);
            }

            let (early, late) = if entry_first {
                let early = commit_window(&manager, &session, &joining).await;
                (early, commit_window(&manager, &session, &active).await)
            } else {
                let late = commit_window(&manager, &session, &active).await;
                (commit_window(&manager, &session, &joining).await, late)
            };
            let early_number = u64::from(first_404);
            assert_eq!(numbering(&early), (7..=9).map(|seq| (seq, early_number)).collect::<Vec<_>>());
            assert_eq!(numbering(&late), (12..=17).map(|seq| (seq, early_number + 1)).collect::<Vec<_>>());
            assert_eq!(late.discontinuity_sequence, early_number);
            assert!(late.visible_segments[0].discontinuity_before);
            {
                let mut state = session.write().await;
                let positions = state
                    .live_recovery_discontinuities
                    .keys()
                    .map(|seq| state.segments[seq].origin_key.host_local_sequence)
                    .collect::<Vec<_>>();
                assert_eq!(positions, if first_404 { vec![107, 111] } else { vec![111] });
                assert_eq!(state.origin_discontinuity_before(&state.segments[&11]), origin_tag);
                assert_eq!(state.segments[&5], old_binding);
                state.apply_origin_manifest(&manifest(113, 6)).unwrap();
                mark_ready(&mut state, 18..=18);
            }
            let slid = commit_window(&manager, &session, &active).await;
            assert_eq!(slid.first_proxy_seq, 13);
            assert_eq!(slid.discontinuity_sequence, early_number + 1);
            assert!(slid.visible_segments.iter().all(|segment| !segment.discontinuity_before));
            for (current, previous) in slid.visible_segments.iter().zip(&late.visible_segments[1..]) {
                assert_eq!(current.uri, previous.uri);
                let seq = current.proxy_seq;
                assert_eq!(
                    slid.startup_revisions.as_ref().unwrap().revisions[&seq],
                    late.startup_revisions.as_ref().unwrap().revisions[&seq]
                );
            }
            {
                let mut state = session.write().await;
                state.release_publication_retention(&joining, NOW + 1);
                state.render_and_store_manifest(NOW + 1).unwrap();
            }
            manager.garbage_collector().run_once(NOW + 180_000).await.unwrap();
            {
                let state = session.read().await;
                assert!(!state.segments.contains_key(&11));
                assert_eq!(state.rendered_discontinuity_sequence(13, None), early_number + 1);
                for (seq, binding) in
                    slid.visible_proxy_seqs().zip(slid.cached_source.as_ref().unwrap().bindings.iter())
                {
                    assert!(binding.is_current(&state, &state.segments[&seq]));
                }
            }
            eprintln!("separated_gaps entry_first={entry_first} first_404={first_404} origin_tag={origin_tag} early_number={early_number} late_header={} slide_header={} gc_header={}", late.discontinuity_sequence, slid.discontinuity_sequence, early_number + 1);
        }
    }
}
