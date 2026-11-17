use super::*;

#[tokio::test]
async fn published_resource_history_survives_production_segment_removal_and_file_deletion() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let (gc, session) = gc_with_session(&temp_dir).await;
    let baseline = normal_manifest(
        "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:480\n\
             #EXTINF:4,\n480.ts\n#EXTINF:4,\n481.ts\n#EXTINF:4,\n482.ts\n",
    );
    let replay_then_new = normal_manifest(
        "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:480\n\
             #EXTINF:4,\n480.ts\n#EXTINF:4,\n481.ts\n#EXTINF:4,\n482.ts\n#EXTINF:4,\n480.ts\n",
    );
    let removed_key = {
        let mut session = session.write().await;
        session.apply_origin_manifest(&baseline).expect("baseline maps");
        for segment in session.segments.values_mut() {
            segment.status = SegmentCacheStatus::Ready { content_length: 1, ready_at_ms: 1 };
        }
        session.render_and_store_manifest(1).expect("baseline publishes");
        session.segments.get(&0).expect("published head").cache_key.clone()
    };
    gc.cache.write_bytes_and_commit(&removed_key, b"x").await.expect("cache object writes");
    {
        let mut session = session.write().await;
        assert_eq!(remove_segment_entry(&mut session, 0), Some(removed_key.clone()));
    }
    gc.cache.delete(&removed_key).await.expect("production cache file deletion succeeds");
    {
        let mut session = session.write().await;
        session.apply_origin_manifest(&replay_then_new).expect("history trims removed resource replay");
        assert!(!session.segments.contains_key(&0));
        assert_eq!(session.proxy_next_seq, Some(4));
        assert_eq!(session.segments[&3].origin_key.host_local_sequence, 483);
        assert_ne!(
            session.segments[&3].occurrence_identity,
            baseline.segments.first().map(|segment| {
                crate::resource_identity::HlsSegmentOccurrenceIdentity::from_parsed(
                    crate::resource_identity::HlsMediaPlaylistScope::from_session(&session.key).unwrap(),
                    segment,
                )
            })
        );
    }
    assert!(gc.cache.metadata(&removed_key).await.expect("metadata lookup succeeds").is_none());
}

#[tokio::test]
async fn forward_jump_discontinuity_sequence_survives_cache_collection() {
    let directory = tempfile::tempdir().unwrap();
    let (gc, session) = gc_with_session(&directory).await;
    update_gc_policy(&gc, |policy| {
        policy.cache_bytes_global = u64::MAX;
        policy.cache_bytes_per_session = u64::MAX;
        policy.session_idle_timeout_ms = u64::MAX;
    });
    populate_ready_segments(&gc, &session, 1).await;
    {
        let mut state = session.write().await;
        let first_missing_msn = state.origin_seq_highwater.unwrap() + 1;
        state.apply_origin_manifest(&rolling_metadata_manifest(first_missing_msn + 1)).unwrap();
    }
    cache_selected_ready_segments(&gc, &session, &[7, 8, 9, 10, 11, 12], b"segment-body").await;
    let (next_msn, boundary_key) = {
        let mut state = session.write().await;
        let rendered = state.render_and_store_manifest(20).unwrap();
        assert_eq!(rendered.first_proxy_seq, 7);
        assert_eq!(rendered.discontinuity_sequence, 0);
        assert_eq!(rendered.body.lines().filter(|line| *line == "#EXT-X-DISCONTINUITY").count(), 1);
        let msn = state.segments[&8].origin_key.host_local_sequence;
        let key = state.segments[&7].cache_key.clone();
        state.apply_origin_manifest(&rolling_metadata_manifest(msn)).unwrap();
        (msn, key)
    };
    cache_selected_ready_segments(&gc, &session, &[13], b"segment-body").await;
    let slid = session.write().await.render_and_store_manifest(30).unwrap();
    assert_eq!(slid.first_proxy_seq, 8);
    assert_eq!(slid.discontinuity_sequence, 1);
    assert!(!slid.body.lines().any(|line| line == "#EXT-X-DISCONTINUITY"));
    assert!(gc.cache.metadata(&boundary_key).await.unwrap().is_some());
    gc.run_once(100_000).await.unwrap();
    assert!(gc.cache.metadata(&boundary_key).await.unwrap().is_none());
    {
        let state = session.read().await;
        assert!(!state.segments.contains_key(&7));
        let rendered = crate::HlsManifestRenderer::render(&state, 100_000).unwrap();
        assert_eq!(rendered.body, slid.body);
    }
    gc.run_once(110_000).await.unwrap();
    let mut state = session.write().await;
    state.apply_origin_manifest(&rolling_metadata_manifest(next_msn)).unwrap();
    assert_eq!(state.render_and_store_manifest(110_000).unwrap().body, slid.body);
    eprintln!("gc_forward_jump first_proxy=8 boundary_proxy=7 removed=true header_before_gc=1 header_after_gc=1");
}

#[test]
fn forward_jump_evicted_boundary_still_counts_at_its_original_position() {
    let mut state = HlsSession::new(HlsSessionKey::new(1, "retained-discontinuity"), b"secret", 0);
    state.apply_origin_manifest(&rolling_metadata_manifest(100)).unwrap();
    for entry in state.segments.values_mut() {
        entry.status = SegmentCacheStatus::Ready { content_length: 1, ready_at_ms: 1 };
    }
    let old_window = state.render_and_store_manifest(1).unwrap();
    state.apply_origin_manifest(&rolling_metadata_manifest(107)).unwrap();
    for (_, entry) in state.segments.range_mut(6..) {
        entry.status = SegmentCacheStatus::Ready { content_length: 1, ready_at_ms: 1 };
    }
    let first_recovered = state.render_and_store_manifest(2).unwrap();
    assert_eq!(first_recovered.first_proxy_seq, 6);
    assert_eq!(first_recovered.discontinuity_sequence, 0);
    assert_eq!(first_recovered.body.lines().filter(|line| *line == "#EXT-X-DISCONTINUITY").count(), 1);
    state.apply_origin_manifest(&rolling_metadata_manifest(108)).unwrap();
    for (_, entry) in state.segments.range_mut(7..) {
        entry.status = SegmentCacheStatus::Ready { content_length: 1, ready_at_ms: 1 };
    }
    let current = state.render_and_store_manifest(2).unwrap();
    assert_eq!(current.first_proxy_seq, 7);
    assert_eq!(current.discontinuity_sequence, 1);

    // An obsolete entry with no cached media can be retired before an older
    // cached window loses its retention.
    assert!(remove_segment_entry(&mut state, 6).is_some());
    assert_eq!(crate::HlsManifestRenderer::render(&state, 2).unwrap().body, current.body);
    state.publishable_origin_head_proxy_seq = Some(0);
    state.publishable_origin_tail_proxy_seq = Some(5);
    assert_eq!(crate::HlsManifestRenderer::render(&state, 2).unwrap().body, old_window.body);

    state.publishable_origin_head_proxy_seq = Some(7);
    state.publishable_origin_tail_proxy_seq = Some(12);
    for seq in 0..6 {
        assert!(remove_segment_entry(&mut state, seq).is_some());
    }
    assert_eq!(state.discontinuity_sequence, 1);
    assert_eq!(crate::HlsManifestRenderer::render(&state, 3).unwrap().body, current.body);

    for seq in 7..=12 {
        assert!(remove_segment_entry(&mut state, seq).is_some());
    }
    state.apply_origin_manifest(&rolling_metadata_manifest(114)).unwrap();
    for entry in state.segments.values_mut() {
        entry.status = SegmentCacheStatus::Ready { content_length: 1, ready_at_ms: 1 };
    }
    let after_empty_cache = state.render_and_store_manifest(4).unwrap();
    assert_eq!(after_empty_cache.first_proxy_seq, 13);
    assert_eq!(after_empty_cache.discontinuity_sequence, 1);
}

#[test]
fn forward_jump_evicted_unpublished_boundary_stays_visible_on_first_publication() {
    let mut state = HlsSession::new(HlsSessionKey::new(1, "unpublished-discontinuity"), b"secret", 0);
    state.apply_origin_manifest(&rolling_metadata_manifest(100)).unwrap();
    for entry in state.segments.values_mut() {
        entry.status = SegmentCacheStatus::Ready { content_length: 1, ready_at_ms: 1 };
    }
    let previous = state.render_and_store_manifest(1).unwrap();
    state.apply_origin_manifest(&rolling_metadata_manifest(107)).unwrap();
    assert!(state.segments[&6].discontinuity_before);
    state.apply_origin_manifest(&rolling_metadata_manifest(108)).unwrap();
    assert!(remove_segment_entry(&mut state, 6).is_some());
    for (_, entry) in state.segments.range_mut(7..) {
        entry.status = SegmentCacheStatus::Ready { content_length: 1, ready_at_ms: 2 };
    }

    let first = state.render_and_store_manifest(2).unwrap();
    assert_eq!(first.first_proxy_seq, 7);
    assert_eq!(first.discontinuity_sequence, previous.discontinuity_sequence);
    assert_eq!(first.body.lines().filter(|line| *line == "#EXT-X-DISCONTINUITY").count(), 1);
    assert_eq!(state.render_and_store_manifest(3).unwrap().body, first.body);

    state.apply_origin_manifest(&rolling_metadata_manifest(109)).unwrap();
    state.segments.get_mut(&13).unwrap().status = SegmentCacheStatus::Ready { content_length: 1, ready_at_ms: 4 };
    let slid = state.render_and_store_manifest(4).unwrap();
    assert_eq!(slid.first_proxy_seq, 8);
    assert_eq!(slid.discontinuity_sequence, 1);
    assert!(!slid.body.lines().any(|line| line == "#EXT-X-DISCONTINUITY"));
    assert!(remove_segment_entry(&mut state, 7).is_some());
    assert_eq!(state.render_and_store_manifest(5).unwrap().body, slid.body);
    eprintln!("gc_pending_boundary removed_proxy=6 first_proxy=7 first_header=0 slid_proxy=8 slid_header=1");
}

fn published_media(manifest: &crate::RenderedManifest) -> Vec<(&str, u64)> {
    let mut number = manifest.discontinuity_sequence;
    let mut media = Vec::new();
    for line in manifest.body.lines() {
        if line == "#EXT-X-DISCONTINUITY" {
            number += 1;
        } else if line.starts_with("/hls/") {
            media.push((line, number));
        }
    }
    media
}

fn ready_first_ready_segments(state: &mut HlsSession, range: std::ops::RangeInclusive<u64>, now_ms: u64) {
    let startup = state.startup.as_mut().unwrap();
    for (&seq, entry) in state.segments.range_mut(range) {
        entry.status = SegmentCacheStatus::Ready { content_length: 1, ready_at_ms: now_ms };
        let revision =
            startup.store.create(state.proxy_session_id.clone(), seq, crate::SegmentRevisionKind::Processed).unwrap();
        assert!(revision.revision().complete(crate::CachedSegmentMetadata { path: "fixture.ts".into(), size: 1 }));
        startup.revisions.insert(seq, revision);
    }
}

#[tokio::test]
async fn forward_jump_late_404_survives_first_ready_metadata_collection() {
    use shared::model::{HlsCacheConfigDto, HlsStartupMode, HlsStripMode};
    use tuliprox_core::model::HlsCacheConfig;

    const NOW: u64 = 2_000;
    for (collect, late_404) in [(false, false), (true, false), (false, true), (true, true)] {
        let directory = tempfile::tempdir().unwrap();
        let mut dto = HlsCacheConfigDto {
            cache_path: Some(directory.path().to_string_lossy().into_owned()),
            ..Default::default()
        };
        dto.startup.mode = HlsStartupMode::FirstReady;
        dto.strip.mode = HlsStripMode::Segments;
        dto.strip.value = 2;
        let manager = crate::HlsProxyManager::with_hls_cache_config(&HlsCacheConfig::from(&dto));
        manager.freeze_clock_for_test(NOW);
        let handle = manager.get_or_create_session(HlsSessionKey::new(1, "recovery-gc-late"), b"fixture", NOW).await;
        let (old, old_bindings) = {
            let mut state = handle.write().await;
            state.live_window_strip = manager.strip();
            let baseline = rolling_metadata_manifest_with_len(100, 7);
            state.apply_origin_manifest_for_host(&baseline, 1).unwrap();
            ready_first_ready_segments(&mut state, 0..=4, NOW);
            let old = state.render_and_store_manifest(NOW).unwrap();
            assert_eq!(old.segment_proxy_seqs, [2, 3, 4]);
            assert!(published_media(&old).iter().all(|(_, number)| *number == 0));
            let bindings = (2..=4).map(|seq| state.segments[&seq].clone()).collect::<Vec<_>>();
            state.segments.get_mut(&5).unwrap().status =
                SegmentCacheStatus::Fetching { priority: SegmentFetchPriority::Prefetch, started_at_ms: NOW };
            state.apply_origin_manifest_for_host(&rolling_metadata_manifest(107), 1).unwrap();
            assert_eq!(state.publishable_origin_head_proxy_seq, Some(7));
            assert!(!state.segments[&7].discontinuity_before);
            (old, bindings)
        };

        if collect {
            manager.garbage_collector().run_once(NOW + 1).await.unwrap();
        }
        let mut state = handle.write().await;
        assert_eq!(state.segments.contains_key(&6), !collect);
        assert!(matches!(state.segments[&5].status, SegmentCacheStatus::Fetching { .. }));
        assert_eq!(state.last_rendered_manifest.as_ref(), Some(&old));
        if late_404 {
            state.segments.get_mut(&5).unwrap().status = SegmentCacheStatus::FailedPermanent {
                status: Some(axum::http::StatusCode::NOT_FOUND),
                failed_at_ms: NOW + 2,
            };
        }
        ready_first_ready_segments(&mut state, 7..=12, NOW);
        let recovered = state.render_and_store_manifest(NOW + 3).unwrap();
        assert_eq!(recovered.segment_proxy_seqs, [7, 8, 9, 10, 11, 12]);
        assert_eq!(recovered.discontinuity_sequence, 0);
        let tag_count = recovered.body.lines().filter(|line| *line == "#EXT-X-DISCONTINUITY").count();
        assert_eq!(tag_count, usize::from(late_404));
        let recovered_media = published_media(&recovered);
        assert!(recovered_media.iter().all(|(_, number)| *number == u64::from(late_404)));
        assert_eq!(state.render_and_store_manifest(NOW + 4).unwrap().body, recovered.body);
        let recovery_bindings =
            state.segments.range(8..=12).map(|(_, entry)| entry.cache_key.clone()).collect::<Vec<_>>();
        let revisions = state
            .startup
            .as_ref()
            .unwrap()
            .revisions
            .range(8..=12)
            .map(|(_, revision)| revision.revision().id())
            .collect::<Vec<_>>();

        state.apply_origin_manifest_for_host(&rolling_metadata_manifest(108), 1).unwrap();
        ready_first_ready_segments(&mut state, 13..=13, NOW);
        let slid = state.render_and_store_manifest(NOW + 5).unwrap();
        assert_eq!(slid.segment_proxy_seqs, [8, 9, 10, 11, 12, 13]);
        assert_eq!(slid.discontinuity_sequence, u64::from(late_404));
        assert!(!slid.body.lines().any(|line| line == "#EXT-X-DISCONTINUITY"));
        assert_eq!(published_media(&slid)[..5], recovered_media[1..]);
        assert!(state.segments.range(2..=4).map(|(_, entry)| entry).eq(&old_bindings));
        assert!(state.segments.range(8..=12).map(|(_, entry)| &entry.cache_key).eq(&recovery_bindings));
        assert!(state
            .startup
            .as_ref()
            .unwrap()
            .revisions
            .range(8..=12)
            .map(|(_, revision)| revision.revision().id())
            .eq(revisions));
        assert_eq!(state.render_and_store_manifest(NOW + 6).unwrap().body, slid.body);
        eprintln!(
            "gc={collect} late_404={late_404} first_proxy=7 header={} tags={tag_count} slid_proxy=8 slid_header={}",
            recovered.discontinuity_sequence, slid.discontinuity_sequence
        );
    }
}

#[tokio::test]
async fn forward_jump_confirms_only_collected_unloaded_mappings() {
    for (collect, old_ready) in [(false, false), (true, true), (true, false)] {
        let (manager, handle, old) = first_ready_gc_baseline().await;
        {
            let mut state = handle.write().await;
            if old_ready {
                ready_first_ready_segments(&mut state, 5..=6, 2_000);
            }
            state.apply_origin_manifest_for_host(&rolling_metadata_manifest(107), 1).unwrap();
        }
        if collect {
            manager.garbage_collector().run_once(2_001).await.unwrap();
        }
        let mut state = handle.write().await;
        let retired = collect && !old_ready;
        assert_eq!(state.segments.range(5..=6).count(), if retired { 0 } else { 2 });
        assert_eq!(state.last_rendered_manifest.as_ref(), Some(&old));
        assert_first_ready_recovery_slide(&mut state, 7, 107, 1, 0, u64::from(retired));
        eprintln!(
            "gc_mappings collect={collect} old_ready={old_ready} removed={retired} header=0 slid_header={}",
            u64::from(retired)
        );
    }
}

#[tokio::test]
async fn forward_jump_merges_collected_window_only_without_playable_media() {
    for (playable, origin_tag) in [(false, false), (false, true), (true, false), (true, true)] {
        let (manager, handle, old) = first_ready_gc_baseline().await;
        {
            let mut state = handle.write().await;
            let mut middle = rolling_metadata_manifest(108);
            middle.segments[0].discontinuity_before = origin_tag;
            state.apply_origin_manifest_for_host(&middle, 1).unwrap();
            if playable {
                ready_first_ready_segments(&mut state, 8..=8, 2_000);
            }
            assert!(state.render_and_store_manifest(2_000).is_err());
            state.apply_origin_manifest_for_host(&rolling_metadata_manifest(115), 1).unwrap();
        }
        manager.garbage_collector().run_once(2_001).await.unwrap();
        let mut state = handle.write().await;
        let remaining = state.segments.range(7..=12).map(|(&seq, _)| seq).collect::<Vec<_>>();
        assert_eq!(remaining, if playable { vec![8] } else { vec![] });
        assert_eq!(state.last_rendered_manifest.as_ref(), Some(&old));
        assert_first_ready_recovery_slide(&mut state, 13, 115, 1, u64::from(playable), 1);
        let boundaries = state.live_recovery_discontinuities.keys().copied().collect::<Vec<_>>();
        assert_eq!(boundaries, if playable { vec![8, 13] } else { vec![13] });
        eprintln!("gc_jumps playable={playable} origin_tag={origin_tag} boundaries={boundaries:?} first_header={} slid_header={}", u64::from(playable), 1 + u64::from(playable));
    }
}

#[tokio::test]
async fn forward_jump_gc_merges_unplayable_window_across_host_handoff() {
    for (collect_before_handoff, playable) in [(false, false), (false, true), (true, false), (true, true)] {
        let (manager, handle, old) = first_ready_gc_baseline().await;
        {
            let mut state = handle.write().await;
            state.apply_origin_manifest_for_host(&rolling_metadata_manifest(108), 1).unwrap();
            if playable {
                ready_first_ready_segments(&mut state, 8..=8, 2_000);
            }
            assert!(state.render_and_store_manifest(2_000).is_err());
            if collect_before_handoff {
                state.apply_origin_manifest_for_host(&rolling_metadata_manifest(109), 1).unwrap();
            }
        }
        if collect_before_handoff {
            manager.garbage_collector().run_once(2_001).await.unwrap();
            assert!(!handle.read().await.segments.contains_key(&7));
        }
        let handoff_proxy = 13 + u64::from(collect_before_handoff);
        {
            let mut state = handle.write().await;
            let replacement = rolling_metadata_manifest(200);
            let preview = state.preview_origin_handoff_manifest(&replacement, 2, 0).unwrap();
            state.apply_origin_handoff_manifest_if_preview_matches(&replacement, 2, 0, &preview).unwrap();
            assert_eq!(state.origin_epoch, 1);
            ready_first_ready_segments(&mut state, handoff_proxy..=handoff_proxy, 2_001);
        }
        manager.garbage_collector().run_once(2_001).await.unwrap();
        let mut state = handle.write().await;
        let retained = state.segments.range(7..handoff_proxy).map(|(&seq, _)| seq).collect::<Vec<_>>();
        assert_eq!(retained, if playable { vec![8] } else { vec![] });
        assert_eq!(state.last_rendered_manifest.as_ref(), Some(&old));
        assert_first_ready_recovery_slide(&mut state, handoff_proxy, 200, 2, u64::from(playable), 1);
        let boundaries = state.live_recovery_discontinuities.keys().copied().collect::<Vec<_>>();
        assert_eq!(boundaries, if playable { vec![8, handoff_proxy] } else { vec![handoff_proxy] });
        eprintln!("gc_handoff before_handoff={collect_before_handoff} playable={playable} boundaries={boundaries:?} first_header={} slid_header={}", u64::from(playable), 1 + u64::from(playable));
    }
}

async fn first_ready_gc_baseline() -> (crate::HlsProxyManager, crate::HlsSessionHandle, crate::RenderedManifest) {
    use shared::model::{HlsCacheConfigDto, HlsStartupMode, HlsStripMode};
    use tuliprox_core::model::HlsCacheConfig;

    let mut dto = HlsCacheConfigDto::default();
    dto.startup.mode = HlsStartupMode::FirstReady;
    dto.strip.mode = HlsStripMode::Segments;
    dto.strip.value = 2;
    let manager = crate::HlsProxyManager::with_hls_cache_config(&HlsCacheConfig::from(&dto));
    manager.freeze_clock_for_test(2_000);
    let handle = manager.get_or_create_session(HlsSessionKey::new(1, "recovery-gc-mappings"), b"fixture", 2_000).await;
    let old = {
        let mut state = handle.write().await;
        state.live_window_strip = manager.strip();
        state.apply_origin_manifest_for_host(&rolling_metadata_manifest_with_len(100, 7), 1).unwrap();
        ready_first_ready_segments(&mut state, 0..=4, 2_000);
        let old = state.render_and_store_manifest(2_000).unwrap();
        assert_eq!(old.segment_proxy_seqs, [2, 3, 4]);
        old
    };
    (manager, handle, old)
}

fn assert_first_ready_recovery_slide(state: &mut HlsSession, proxy: u64, msn: u64, host: u64, header: u64, tags: u64) {
    let old_binding = state.segments[&4].clone();
    ready_first_ready_segments(state, proxy..=proxy + 5, 2_002);
    let first = state.render_and_store_manifest(2_002).unwrap();
    assert_eq!(first.first_proxy_seq, proxy);
    assert_eq!(state.segments[&proxy].origin_key.host_local_sequence, msn);
    assert_eq!(first.discontinuity_sequence, header);
    assert_eq!(
        first.body.lines().filter(|line| *line == "#EXT-X-DISCONTINUITY").count(),
        usize::try_from(tags).unwrap()
    );
    assert_eq!(state.render_and_store_manifest(2_003).unwrap().body, first.body);
    let revision = state.startup.as_ref().unwrap().revisions[&(proxy + 1)].revision().id();
    state.apply_origin_manifest_for_host(&rolling_metadata_manifest(msn + 1), host).unwrap();
    ready_first_ready_segments(state, proxy + 6..=proxy + 6, 2_004);
    let slid = state.render_and_store_manifest(2_004).unwrap();
    assert_eq!(slid.first_proxy_seq, proxy + 1);
    assert_eq!(slid.discontinuity_sequence, header + tags);
    assert!(!slid.body.lines().any(|line| line == "#EXT-X-DISCONTINUITY"));
    assert_eq!(published_media(&slid)[..5], published_media(&first)[1..]);
    assert_eq!(state.startup.as_ref().unwrap().revisions[&(proxy + 1)].revision().id(), revision);
    assert_eq!(state.segments[&4], old_binding);
    assert!(state.retired_live_mappings.is_empty());
}

#[tokio::test]
async fn gc_keeps_last_rendered_manifest_segments() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let (gc, session) = gc_with_session(&temp_dir).await;
    populate_ready_segments(&gc, &session, 0).await;

    let report = gc.run_once(10_000).await.expect("gc should run");

    assert_eq!(report.segments_deleted_duration, 0);
    assert_eq!(session.read().await.segments.len(), 6);
}

#[tokio::test]
async fn finalized_transient_resource_mappings_in_current_manifest_are_protected() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let (gc, session) = gc_with_session(&temp_dir).await;
    let resource_id = {
        let mut session = session.write().await;
        let resource = crate::TransientResourceRef::new(
            crate::TransientResourceKind::Segment,
            "http://origin.example.com/live/seg.ts",
            b"secret",
            0,
            10,
            Some("ts".to_string()),
        );
        let resource_id = resource.id.clone();
        session.transient.upsert_resources([resource]);
        session.transient.replace_manifest_with_semantics(
            format!("#EXTM3U\n#EXTINF:1,\n/hls/shared/live/session/lease/r/{}.ts\n#EXT-X-ENDLIST\n", resource_id.0),
            0,
            Some(1_000),
        );
        resource_id
    };

    let report = gc.run_once(20).await.expect("gc should run");

    assert_eq!(report.transient_resources_pruned, 0);
    assert!(session.read().await.transient.resources.contains_key(&resource_id));
}

#[test]
fn finalized_manifest_mapping_count_does_not_become_object_pin_count() {
    let mut session = HlsSession::new(HlsSessionKey::new(1, "large-finalized"), b"secret", 0);
    let resources = install_finalized_transient_manifest(&mut session, 1_643, 10);

    let protected = ProtectedSet::from_session(&session);

    assert_eq!(resources.len(), 1_643);
    assert_eq!(session.transient.current_manifest_resource_ids().len(), 1_643);
    assert!(protected.transient_object_ids.is_empty());
    assert!(resources.iter().all(|resource| session.transient.resolve_current_resource(&resource.id, 20).is_some()));
}

#[tokio::test]
async fn large_finalized_manifest_keeps_mappings_without_pinning_all_objects() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let (gc, session) = gc_with_session(&temp_dir).await;
    let (resource, lookup_key, fetch_token) = {
        let mut session = session.write().await;
        let resources = install_finalized_transient_manifest(&mut session, 1_643, 10);
        let resource = resources.into_iter().next().expect("large manifest resource");
        let lookup_key = TransientPassthroughState::transient_object_key(&session.proxy_session_id, &resource.id, "ts");
        let proxy_session_id = session.proxy_session_id.clone();
        let fetch_token = match session.transient.begin_object_fetch(&proxy_session_id, &resource, "ts", 0, 10) {
            TransientObjectFetchDecision::Fetch(token) => token,
            TransientObjectFetchDecision::Ready | TransientObjectFetchDecision::Wait(_) => {
                panic!("first archive object starts a cache fill")
            }
        };
        (resource, lookup_key, fetch_token)
    };
    gc.cache.write_bytes_and_commit(fetch_token.cache_key(), b"archive-object").await.expect("archive object writes");
    {
        let mut session = session.write().await;
        assert!(session.commit_transient_object_ready_if_current(
            resource.kind,
            &fetch_token,
            "video/mp2t".to_string(),
            14,
            0,
            10,
        ));
        let protected = ProtectedSet::from_session(&session);
        assert_eq!(session.transient.current_manifest_resource_ids().len(), 1_643);
        assert!(!protected.transient_object_ids.contains(&resource.id));
    }

    let report = gc.run_once(20).await.expect("finalized archive GC runs");

    assert_eq!(report.transient_resources_pruned, 0);
    assert_eq!(report.transient_objects_deleted, 1);
    assert!(gc.cache.metadata(fetch_token.cache_key()).await.expect("object metadata reads").is_none());
    let mut session = session.write().await;
    assert_eq!(session.transient.resources.len(), 1_643);
    let current = session.transient.resolve_current_resource(&resource.id, 20).expect("mapping survives byte eviction");
    assert!(!session.transient.object_cache.contains_key(&lookup_key));
    let proxy_session_id = session.proxy_session_id.clone();
    assert!(matches!(
        session.transient.begin_object_fetch(&proxy_session_id, &current, "ts", 20, 10,),
        TransientObjectFetchDecision::Fetch(_)
    ));
}

#[tokio::test]
async fn transient_object_gc_deletes_the_physical_fill_generation_instead_of_the_logical_lookup_path() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let (gc, session) = gc_with_session(&temp_dir).await;
    let (lookup_key, fetch_token, resource_kind) = {
        let mut session = session.write().await;
        let resource = TransientResourceRef::new(
            TransientResourceKind::Segment,
            "http://origin.example.com/live/object1.ts",
            b"secret",
            0,
            10,
            Some("ts".to_string()),
        );
        let resource_id = resource.id.clone();
        session.transient.upsert_resources([resource]);
        let resource = session.transient.resources.get(&resource_id).expect("registered resource").clone();
        let proxy_session_id = session.proxy_session_id.clone();
        let lookup_key = TransientPassthroughState::transient_object_key(&proxy_session_id, &resource_id, "ts");
        let fetch_token = match session.transient.begin_object_fetch(&proxy_session_id, &resource, "ts", 0, 10) {
            TransientObjectFetchDecision::Fetch(token) => token,
            TransientObjectFetchDecision::Ready | TransientObjectFetchDecision::Wait(_) => {
                panic!("new transient object starts one physical cache fill")
            }
        };
        (lookup_key, fetch_token, resource.kind)
    };
    assert_ne!(&lookup_key, fetch_token.cache_key());
    gc.cache
        .write_bytes_and_commit(fetch_token.cache_key(), b"transient-body")
        .await
        .expect("physical generation writes");
    gc.cache.write_bytes_and_commit(&lookup_key, b"logical-decoy").await.expect("logical lookup decoy writes");
    assert!(session.write().await.commit_transient_object_ready_if_current(
        resource_kind,
        &fetch_token,
        "video/mp2t".to_string(),
        14,
        0,
        10,
    ));

    let report = gc.run_once(20).await.expect("gc should run");

    assert_eq!(report.transient_objects_deleted, 1);
    assert_eq!(report.transient_object_bytes_deleted, 14);
    assert!(gc.cache.metadata(fetch_token.cache_key()).await.expect("physical metadata read").is_none());
    assert!(gc.cache.metadata(&lookup_key).await.expect("logical metadata read").is_some());
}

#[test]
fn garbage_collection_preserves_playback_reserve_and_cutover_deadline() {
    use crate::{
        media_reserve::{
            evaluate_lease_reserve, HlsLeaseManifestSegment, HlsLeaseManifestSnapshot, HlsLeasePlaybackCursor,
            HlsLeaseReserveInput, HlsManifestCommitIdentity, HlsManifestDeliveryMode,
        },
        recovery_timing::{HlsLeaseCutoverTiming, HlsRecoveryTriggerBudgetMs},
        terminal_tail::HlsMediaContainer,
    };
    use tuliprox_parser::hls::origin_manifest::{parse_origin_media_manifest, OriginManifestParseOutcome};

    let mut session = HlsSession::new(crate::HlsSessionKey::new(1, "gc-probe"), b"probe", 1_000);
    let mut body = String::from("#EXTM3U\n#EXT-X-TARGETDURATION:12\n#EXT-X-MEDIA-SEQUENCE:0\n");
    for seq in 0..9 {
        write!(body, "#EXTINF:12.0,\n{seq}.ts\n").expect("test manifest writes");
    }
    let OriginManifestParseOutcome::Normal(origin) =
        parse_origin_media_manifest(&body, "http://origin.example/index.m3u8")
    else {
        panic!("normal manifest")
    };
    session.apply_origin_manifest(&origin).unwrap();
    for segment in session.segments.values_mut() {
        segment.status = SegmentCacheStatus::Ready { content_length: 1, ready_at_ms: 1_000 };
    }
    session.publishable_origin_head_proxy_seq = Some(4);
    let mut cursor = HlsLeasePlaybackCursor::default();
    for seq in 0..8 {
        let token = cursor.record_request_started(seq, 1_000 + seq * 10_000);
        let token = cursor.bind_request_timing(token, &session.segments[&seq]);
        cursor.record_transfer_started(token, 1_000 + seq * 10_000);
        let _ = cursor.record_request_completed(token, 1_000 + seq * 10_000);
    }
    let manifest = HlsLeaseManifestSnapshot {
        cached_source: None,
        startup_revisions: None,
        delivery_mode: HlsManifestDeliveryMode::NormalCacheTimeline,
        source_commit_identity: HlsManifestCommitIdentity::new(1),
        uri_materialization: None,
        finalized_transient_manifest_generation: None,
        snapshot_generation: 1,
        delivered_at_ms: 71_000,
        first_proxy_seq: 4,
        last_proxy_seq: 6,
        visible_segments: (4..7)
            .map(|seq| HlsLeaseManifestSegment {
                proxy_seq: seq,
                duration_ms: 12_000,
                uri: format!("{seq}.ts").into(),
                discontinuity_before: false,
                map_ref_ready: true,
                encryption: None,
            })
            .collect::<Vec<_>>()
            .into(),
        discontinuity_sequence: 0,
        target_duration_ms: 12_000,
        playlist_duration_ms: 36_000,
        last_visible_media_end_ms: 36_000,
        active_map: None,
        active_encryption: None,
        container: HlsMediaContainer::MpegTs,
    };
    let evaluate = |session: &HlsSession| {
        let timeline = session.ready_timeline_snapshot(cursor.ready_timeline_start_proxy_seq(4), 71_000);
        let reserve = evaluate_lease_reserve(HlsLeaseReserveInput {
            manifest: &manifest,
            cursor: &cursor,
            ready_timeline: &timeline,
            now_ms: 71_000,
            recovery_trigger_budget: HlsRecoveryTriggerBudgetMs::from_millis(4_000),
            origin_path_degraded: true,
            recovery_committed: false,
        });
        (timeline.units.len(), reserve)
    };
    let (before_units, before) = evaluate(&session);
    assert_eq!(before.guaranteed_reserve_ms, 38_000);
    assert!(!ProtectedSet::from_session(&session).segment_proxy_seqs.contains(&0));
    // Run the actual metadata-removal function used by garbage collection.
    assert!(remove_segment_entry(&mut session, 0).is_some());
    let (after_units, after) = evaluate(&session);
    let still_ready = session.ready_timeline_snapshot(4, 71_000);
    assert_eq!(still_ready.units.len(), 5);
    assert_eq!(cursor.first_requested_proxy_seq, Some(0));
    assert_eq!(before_units, 5);
    assert_eq!(after_units, 5);
    assert_eq!(after, before);
    let timing =
        HlsLeaseCutoverTiming::from_reserve(71_000, after.guaranteed_reserve_ms, after.transition_margin, None);
    assert_eq!(timing.latest_safe_terminal_commit_at.as_millis_since_epoch(), 97_000);
    // Removing the entire historical prefix must keep exactly the same media clock.
    for seq in 1..4 {
        assert!(remove_segment_entry(&mut session, seq).is_some());
    }
    assert_eq!(evaluate(&session).1, before);
    // A real hole in the current READY span must still shorten the reserve.
    assert!(remove_segment_entry(&mut session, 7).is_some());
    assert_eq!(evaluate(&session).1.guaranteed_reserve_ms, 26_000);
}

fn install_fast_start(session: &mut HlsSession) {
    let config = tuliprox_core::model::HlsStartupConfig {
        mode: shared::model::HlsStartupMode::Optimistic,
        ..Default::default()
    };
    session.startup = Some(crate::HlsSessionStartup {
        config: config.clone(),
        worker: std::sync::Weak::new(),
        access_leases: Arc::new(RwLock::new(HlsAccessLeaseStore::default())),
        revisions: std::collections::BTreeMap::new(),
        policy_fixed: true,
        store: Arc::new(crate::SegmentRevisionStore::default()),
        budget: crate::ProgressiveBudgetManager::new(config),
    });
}

#[test]
fn metadata_only_prefix_does_not_block_reclaimable_ready_segments() {
    let mut session = HlsSession::new(HlsSessionKey::new(1, "review-fifo-prefix"), b"secret", 0);
    session.apply_origin_manifest(&six_segment_manifest()).unwrap();
    install_fast_start(&mut session);
    for seq in [3, 4, 5] {
        session.segments.get_mut(&seq).unwrap().status =
            SegmentCacheStatus::Ready { content_length: 1_000, ready_at_ms: 0 };
    }
    session.apply_origin_manifest(&normal_manifest(
        "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:7\n#EXTINF:4,\n7.ts\n#EXTINF:4,\n8.ts\n#EXTINF:4,\n9.ts\n#EXTINF:4,\n10.ts\n#EXTINF:4,\n11.ts\n#EXTINF:4,\n12.ts\n"
    )).unwrap();
    let protected = ProtectedSet::from_session(&session);
    assert!(!protected.segment_proxy_seqs.contains(&3));
    assert!(matches!(session.segments[&0].status, SegmentCacheStatus::Discovered));
    let reclaim = crate::gc::selection::fifo_head_size_candidate(&session, &protected);
    assert_eq!(
        reclaim.map(|entry| entry.proxy_seq),
        Some(3),
        "unfetched metadata 0..2 must not prevent reclamation of expired media 3..5"
    );
    assert_eq!(
        crate::gc::selection::duration_expired_head_segment(&session, &protected, &test_policy(), 100_000),
        Some(3)
    );
    // The first actual media object still blocks FIFO if it is protected or unexpired.
    assert_eq!(crate::gc::selection::duration_expired_head_segment(&session, &protected, &test_policy(), 0), None);
    let mut protected = protected;
    protected.segment_proxy_seqs.insert(3);
    assert!(crate::gc::selection::fifo_head_size_candidate(&session, &protected).is_none());
    assert_eq!(
        crate::gc::selection::duration_expired_head_segment(&session, &protected, &test_policy(), 100_000),
        None
    );
}

#[test]
fn reinserted_metadata_does_not_block_subsequent_cache_deletions() {
    let mut session = HlsSession::new(HlsSessionKey::new(1, "review-fifo-delete"), b"secret", 0);
    session.apply_origin_manifest(&six_segment_manifest()).unwrap();
    install_fast_start(&mut session);
    for entry in session.segments.values_mut() {
        entry.status = SegmentCacheStatus::Ready { content_length: 1_000, ready_at_ms: 0 };
    }
    let protected = ProtectedSet::from_session(&session);
    assert_eq!(crate::gc::selection::fifo_head_size_candidate(&session, &protected).unwrap().proxy_seq, 0);
    remove_segment_entry(&mut session, 0).unwrap();
    let protected = ProtectedSet::from_session(&session);
    assert!(!protected.segment_proxy_seqs.contains(&1));
    let next = crate::gc::selection::fifo_head_size_candidate(&session, &protected);
    assert_eq!(
        next.map(|entry| entry.proxy_seq),
        Some(1),
        "preserving origin mapping must not prevent further capacity reclamation"
    );
    assert_eq!(
        crate::gc::selection::duration_expired_head_segment(&session, &protected, &test_policy(), 100_000),
        Some(1)
    );
    for seq in [1, 2] {
        remove_segment_entry(&mut session, seq).unwrap();
        assert!(matches!(session.segments[&seq].status, SegmentCacheStatus::Discovered));
    }
    let protected = ProtectedSet::from_session(&session);
    assert!(crate::gc::selection::fifo_head_size_candidate(&session, &protected).is_none());
    assert_eq!(
        crate::gc::selection::duration_expired_head_segment(&session, &protected, &test_policy(), 100_000),
        None
    );
}

#[tokio::test]
async fn fast_start_gc_reclaims_multiple_objects_while_retaining_origin_mappings() {
    for size_pressure in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let (gc, session) = gc_with_session(&directory).await;
        update_gc_policy(&gc, |policy| {
            policy.session_idle_timeout_ms = u64::MAX;
            if size_pressure {
                policy.cache_bytes_per_session = 36;
            }
        });
        let keys = {
            let mut state = session.write().await;
            state.apply_origin_manifest(&six_segment_manifest()).unwrap();
            install_fast_start(&mut state);
            for entry in state.segments.values_mut() {
                gc.cache.write_bytes_and_commit(&entry.cache_key, b"segment-body").await.unwrap();
                entry.status = SegmentCacheStatus::Ready { content_length: 12, ready_at_ms: 0 };
            }
            state.segments.values().map(|entry| entry.cache_key.clone()).collect::<Vec<_>>()
        };
        let report = gc.run_once(if size_pressure { 0 } else { 100_000 }).await.unwrap();
        assert_eq!(report.segments_deleted_duration, if size_pressure { 0 } else { 3 });
        assert_eq!(report.segments_deleted_size_session, if size_pressure { 3 } else { 0 });
        let state = session.read().await;
        assert_eq!(state.origin_to_proxy.len(), 6);
        for seq in 0..3 {
            assert!(matches!(state.segments[&seq].status, SegmentCacheStatus::Discovered));
        }
        for (index, key) in keys.iter().enumerate() {
            assert_eq!(gc.cache.metadata(key).await.unwrap().is_some(), index >= 3);
        }
        drop(state);
        session.write().await.apply_origin_manifest(&rolling_metadata_manifest(7)).unwrap();
        gc.run_once(0).await.unwrap();
        let state = session.read().await;
        for seq in 0..3 {
            assert!(!state.segments.contains_key(&seq));
            assert!(!state.origin_to_proxy.values().any(|mapped| *mapped == seq));
        }
        assert!((6..12).all(|seq| state.segments.contains_key(&seq)));
    }
}

fn rolling_metadata_manifest(first: u64) -> tuliprox_parser::hls::origin_manifest::ParsedOriginManifest {
    rolling_metadata_manifest_with_len(first, 6)
}

fn rolling_metadata_manifest_with_len(
    first: u64,
    count: u64,
) -> tuliprox_parser::hls::origin_manifest::ParsedOriginManifest {
    let mut body = format!("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:{first}\n");
    for seq in first..first + count {
        writeln!(body, "#EXTINF:4,\n{seq}.ts").unwrap();
    }
    normal_manifest(&body)
}

#[tokio::test]
async fn obsolete_metadata_stays_bounded_across_one_hundred_live_entries() {
    use shared::model::{HlsCacheConfigDto, HlsStartupMode, Secs};
    use tuliprox_core::model::HlsCacheConfig;

    for mode in [HlsStartupMode::FirstReady, HlsStartupMode::Progressive, HlsStartupMode::Optimistic] {
        let directory = tempfile::tempdir().unwrap();
        let mut dto = HlsCacheConfigDto {
            cache_path: Some(directory.path().to_string_lossy().into_owned()),
            cache_duration: Secs::new(1),
            session_idle_timeout: Secs::new(1_000_000),
            ..Default::default()
        };
        dto.startup.mode = mode;
        let manager = crate::HlsProxyManager::with_hls_cache_config(&HlsCacheConfig::from(&dto));
        let session = manager.get_or_create_session(HlsSessionKey::new(1, "metadata-soak"), b"secret", 0).await;
        for round in 0..100 {
            let first = round * 6;
            let keys = {
                let mut state = session.write().await;
                state.apply_origin_manifest_for_host(&rolling_metadata_manifest(first), 1).unwrap();
                let plan = state.plan_live_window(crate::live_window::HlsLiveWindowView::Entry).unwrap();
                assert_eq!(plan.visible, vec![first + 3, first + 4, first + 5]);
                plan.visible
                    .iter()
                    .map(|seq| {
                        let entry = state.segments.get_mut(seq).unwrap();
                        entry.status = SegmentCacheStatus::Ready { content_length: 12, ready_at_ms: 0 };
                        entry.cache_key.clone()
                    })
                    .collect::<Vec<_>>()
            };
            for key in keys {
                manager.segment_cache().write_bytes_and_commit(&key, b"segment-body").await.unwrap();
            }
            manager.run_garbage_collection_once(100_000 + round).await.unwrap();
            let mut state = session.write().await;
            assert_eq!(state.segments.keys().copied().collect::<Vec<_>>(), (first..first + 6).collect::<Vec<_>>());
            assert_eq!(state.origin_to_proxy.len(), 6);
            assert!(state.origin_to_proxy.values().all(|seq| (first..first + 6).contains(seq)));
            assert_eq!(
                state.segments.values().filter(|entry| matches!(entry.status, SegmentCacheStatus::Discovered)).count(),
                3
            );
            // Refreshing the same origin must reuse even its unfetched mappings.
            state.apply_origin_manifest_for_host(&rolling_metadata_manifest(first), 1).unwrap();
            assert_eq!(state.live_origin_sequences(), (first..first + 6).collect::<Vec<_>>());
            assert_eq!(state.proxy_next_seq, Some(first + 6));
        }
    }
}

#[tokio::test]
async fn obsolete_metadata_is_removed_only_after_publication_and_reader_protection_release() {
    let directory = tempfile::tempdir().unwrap();
    let (gc, session) = gc_with_session(&directory).await;
    let lease_id = HlsAccessLeaseId("metadata-owner".into());
    {
        let mut state = session.write().await;
        state.apply_origin_manifest(&rolling_metadata_manifest(0)).unwrap();
        for entry in state.segments.values_mut() {
            entry.status = SegmentCacheStatus::Ready { content_length: 12, ready_at_ms: 0 };
        }
        state.render_and_store_manifest(0).unwrap();
        install_fast_start(&mut state);
        for entry in state.segments.values_mut() {
            entry.status = SegmentCacheStatus::Discovered;
        }
        state.apply_origin_manifest(&rolling_metadata_manifest(6)).unwrap();
        state.segments.get_mut(&0).unwrap().access.reader_started(0);
        state.live_window_demands.insert(
            lease_id.clone(),
            crate::live_window::HlsLiveWindowDemand {
                issued_at_ms: 0,
                view: crate::live_window::HlsLiveWindowView::Entry,
                published: vec![1],
                retention_recorded: false,
                retention_normal: false,
                retention_resources: crate::HlsPublishedTransientResourceIds::default(),
            },
        );
        assert!(
            ProtectedSet::from_session_for_capacity(&state, Some(u64::MAX), 0).segment_proxy_seqs.contains(&1),
            "capacity recovery must retain the lease's last live anchor"
        );
        state.install_terminal_tail_protection(
            lease_id.clone(),
            HlsTerminalTailProtection {
                generation: HlsTerminalTailGeneration(1),
                base_proxy_seqs: Arc::from([2]),
                key_bindings: Arc::from([]),
            },
        );
    }
    gc.run_once(0).await.unwrap();
    assert_eq!(session.read().await.segments.len(), 12);
    session.write().await.last_rendered_manifest = None;
    gc.run_once(0).await.unwrap();
    {
        let mut state = session.write().await;
        assert_eq!(state.segments.keys().copied().collect::<Vec<_>>(), vec![0, 1, 2, 6, 7, 8, 9, 10, 11]);
        assert_eq!(state.origin_to_proxy.len(), 9);
        state.segments.get_mut(&0).unwrap().access.reader_finished();
        assert!(state.live_window_demands.remove(&lease_id).is_some());
        assert!(state.remove_terminal_tail_protection(&lease_id).is_some());
    }
    gc.run_once(0).await.unwrap();
    let state = session.read().await;
    assert_eq!(state.segments.keys().copied().collect::<Vec<_>>(), (6..12).collect::<Vec<_>>());
    assert_eq!(state.origin_to_proxy.len(), 6);
}

#[test]
fn obsolete_metadata_cleanup_preserves_non_fast_start_timeline_mappings() {
    for archive in [false, true] {
        let mut session = HlsSession::new(HlsSessionKey::new(1, "retained-metadata"), b"secret", 0);
        session.apply_origin_manifest(&rolling_metadata_manifest(0)).unwrap();
        if archive {
            install_fast_start(&mut session);
            session.origin_source.archive_reference = Some(1_784_898_000);
        }
        session.segments.get_mut(&0).unwrap().discontinuity_before = true;
        session.discontinuity_sequence = 7;
        session.apply_origin_manifest(&rolling_metadata_manifest(6)).unwrap();
        assert!(!session.uses_fast_start_live_window());
        let protected = ProtectedSet::from_session(&session);
        assert!(!protected.segment_proxy_seqs.contains(&0));
        let mappings = session.origin_to_proxy.clone();
        let sequences = session.segments.keys().copied().collect::<Vec<_>>();
        let head = session.publishable_origin_head_proxy_seq;
        let next = session.proxy_next_seq;

        crate::gc::selection::remove_obsolete_segment_metadata(&mut session, &protected);

        assert_eq!(session.origin_to_proxy, mappings, "archive={archive}");
        assert_eq!(session.segments.keys().copied().collect::<Vec<_>>(), sequences);
        assert_eq!(session.discontinuity_sequence, 7);
        assert_eq!(session.publishable_origin_head_proxy_seq, head);
        let mut overlapping = rolling_metadata_manifest(0);
        overlapping.segments.extend(rolling_metadata_manifest(6).segments);
        session.apply_origin_manifest(&overlapping).unwrap();
        assert_eq!(session.proxy_next_seq, next, "an overlapping URI must retain its original proxy sequence");
    }
}

#[test]
fn obsolete_metadata_cleanup_requires_origin_evidence_and_preserves_other_cache_states() {
    use crate::gc::{policy::protected_set_for_cache_reclamation, selection::remove_obsolete_segment_metadata};

    let directory = tempfile::tempdir().unwrap();
    let cache = HlsSegmentCache::with_cache_path(directory.path());
    let mut session = HlsSession::new(HlsSessionKey::new(1, "metadata-states"), b"secret", 0);
    session.apply_origin_manifest(&rolling_metadata_manifest(0)).unwrap();
    install_fast_start(&mut session);
    session.origin_window_proxy_seqs = None;
    remove_obsolete_segment_metadata(&mut session, &ProtectedSet::default());
    assert_eq!(session.segments.len(), 6);
    session.origin_window_proxy_seqs = Some(Vec::new());
    for (seq, status) in [
        SegmentCacheStatus::Ready { content_length: 12, ready_at_ms: 0 },
        SegmentCacheStatus::Fetching { priority: SegmentFetchPriority::Demand, started_at_ms: 0 },
        SegmentCacheStatus::Queued { priority: SegmentFetchPriority::Demand, queued_at_ms: 0 },
        SegmentCacheStatus::CapacityDeferred { priority: SegmentFetchPriority::Demand, deferred_at_ms: 0 },
        SegmentCacheStatus::FailedRetryable { failed_at_ms: 0, retry_after_ms: 0 },
    ]
    .into_iter()
    .enumerate()
    {
        session.segments.get_mut(&u64::try_from(seq).unwrap()).unwrap().status = status;
    }
    let excluded = cache.object_path(&session.segments[&5].cache_key);
    let protected = protected_set_for_cache_reclamation(&session, &cache, Some(&excluded), None);
    remove_obsolete_segment_metadata(&mut session, &protected);
    assert_eq!(session.segments.len(), 6);
    let protected = protected_set_for_cache_reclamation(&session, &cache, None, None);
    remove_obsolete_segment_metadata(&mut session, &protected);
    assert!(!session.segments.contains_key(&5));
    assert_eq!(session.segments.len(), 5);
    assert_eq!(session.origin_to_proxy.len(), 5);
}
