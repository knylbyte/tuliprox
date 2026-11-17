use super::*;

fn publication(session: &HlsSession, seqs: &[u64], at: u64) -> HlsLeaseManifestSnapshot {
    let mut snapshot = activated_startup_lease(&session.proxy_session_id).last_manifest_snapshot.unwrap();
    snapshot.delivered_at_ms = at;
    snapshot.first_proxy_seq = *seqs.first().unwrap();
    snapshot.last_proxy_seq = *seqs.last().unwrap();
    snapshot.visible_segments = seqs
        .iter()
        .map(|seq| HlsLeaseManifestSegment {
            proxy_seq: *seq,
            duration_ms: 4_000,
            uri: format!("/hls/shared/live/{}/lease/{seq:06}.ts", session.proxy_session_id.0).into(),
            discontinuity_before: false,
            map_ref_ready: true,
            encryption: None,
        })
        .collect::<Vec<_>>()
        .into();
    snapshot.playlist_duration_ms = u64::try_from(seqs.len()).unwrap() * 4_000;
    snapshot.last_visible_media_end_ms = snapshot.playlist_duration_ms;
    snapshot
}

async fn retained_fixture(base: u64) -> (tempfile::TempDir, Arc<HlsGarbageCollector>, crate::HlsSessionHandle) {
    let directory = tempfile::tempdir().unwrap();
    let (gc, session) = gc_with_session(&directory).await;
    {
        let mut state = session.write().await;
        let mut body = "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:1\n".to_string();
        for seq in 1..=7 {
            writeln!(body, "#EXTINF:4,\n{seq}.ts").unwrap();
        }
        state.proxy_next_seq = Some(1);
        state.apply_origin_manifest(&normal_manifest(&body)).unwrap();
    }
    cache_selected_ready_segments(&gc, &session, &[1, 2, 3, 4, 5, 6], b"segment-body").await;
    {
        let mut state = session.write().await;
        let lease = HlsAccessLeaseId("retention".into());
        for at in [base, base + 5_000] {
            let snapshot = publication(&state, &[1, 2, 3, 4, 5, 6], at);
            state.record_live_window_publication(&lease, base, &snapshot);
        }
        let snapshot = publication(&state, &[2, 3, 4, 5, 6], base + 10_000);
        state.record_live_window_publication(&lease, base, &snapshot);
        state.publishable_origin_head_proxy_seq = Some(2);
    }
    (directory, gc, session)
}

#[tokio::test]
async fn removed_publication_deadline_starts_at_removal_and_preserves_the_fifo_successor() {
    let (_directory, gc, session) = retained_fixture(0).await;
    let key = session.read().await.segments[&1].cache_key.clone();
    for now in [33_000, 37_999] {
        let report = gc.run_once(now).await.unwrap();
        assert_eq!(report.segments_deleted(), 0);
        assert!(gc.cache.metadata(&key).await.unwrap().is_some());
        assert!(session.read().await.segments.contains_key(&1));
    }
    let report = gc.run_once(38_000).await.unwrap();
    assert_eq!(report.segments_deleted_duration, 1);
    assert!(gc.cache.metadata(&key).await.unwrap().is_none());
    assert!(session.read().await.segments.contains_key(&2));
}

#[tokio::test]
async fn multiple_leases_use_the_longest_containing_playlist_then_honor_an_active_reader() {
    let (_directory, gc, session) = retained_fixture(0).await;
    {
        let mut state = session.write().await;
        state.release_publication_retention(&HlsAccessLeaseId("retention".into()), 0);
        for (lease, seqs, at) in [
            ("short", vec![1, 2, 3], 0),
            ("long", vec![1, 2, 3, 4, 5, 6], 5_000),
            ("short", vec![2, 3], 10_000),
            ("long", vec![2, 3, 4, 5, 6], 15_000),
        ] {
            let snapshot = publication(&state, &seqs, at);
            state.record_live_window_publication(&HlsAccessLeaseId(lease.into()), 0, &snapshot);
        }
        state.segments[&1].access.reader_started(40_000);
    }
    assert_eq!(gc.run_once(42_999).await.unwrap().segments_deleted(), 0);
    assert_eq!(gc.run_once(43_000).await.unwrap().segments_deleted(), 0);
    session.read().await.segments[&1].access.reader_finished();
    assert_eq!(gc.run_once(43_000).await.unwrap().segments_deleted_duration, 1);
    assert!(session.read().await.segments.contains_key(&2));
}

async fn pressure(kind: &str) {
    let base = tuliprox_core::utils::current_time_millis() - 10_000;
    let (_directory, gc, session) = retained_fixture(base).await;
    let (id, target, keys) = {
        let state = session.read().await;
        (
            state.proxy_session_id.clone(),
            state.segments[&7].cache_key.clone(),
            (1..=6).map(|seq| state.segments[&seq].cache_key.clone()).collect::<Vec<_>>(),
        )
    };
    gc.cache.update_cache_limits(72, 72);
    if kind == "session" {
        update_gc_policy(&gc, |policy| policy.cache_bytes_per_session = 60);
    }
    if kind == "global" {
        update_gc_policy(&gc, |policy| policy.cache_bytes_global = 60);
    }
    let before = gc.cache.capacity_usage(&id).await.unwrap();
    assert_eq!((before.global_bytes, before.session_bytes), (72, 72));
    let report = gc.run_once(base + 37_999).await.unwrap();
    assert_eq!(report.segments_deleted(), 0);
    assert_eq!(report.maps_deleted, 0);
    let error = gc.cache.write_bytes_and_commit(&target, b"incoming").await.unwrap_err();
    assert!(crate::cache::hls_cache_capacity_from_io(&error).is_some(), "existing capacity errors defer new admission");
    assert!(gc.cache.metadata(&target).await.unwrap().is_none());
    for key in keys {
        assert!(gc.cache.metadata(&key).await.unwrap().is_some());
    }
    let after = gc.cache.capacity_usage(&id).await.unwrap();
    assert_eq!((after.global_bytes, after.session_bytes), (72, 72));
    assert!(!gc.cache.has_active_temp_files());
}

#[tokio::test]
async fn session_size_reclamation_keeps_removed_publication_bytes_and_defers_new_admission() {
    pressure("session").await;
}

#[tokio::test]
async fn global_size_reclamation_keeps_removed_publication_bytes_and_defers_new_admission() {
    pressure("global").await;
}

#[tokio::test]
async fn projected_capacity_reclamation_keeps_removed_publication_bytes_and_defers_new_admission() {
    pressure("projected").await;
}

#[tokio::test]
async fn cached_transient_mapping_and_bytes_outlive_their_cache_ttl_until_the_removal_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let (gc, session) = gc_with_session(&directory).await;
    let (old, old_key, ids) = {
        let mut state = session.write().await;
        let proxy = state.proxy_session_id.clone();
        state.transient.set_resource_ttl_ms(100);
        state.transient.commit_origin_manifest_for_test(
            "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:10\n#EXTINF:4,\nshared.ts\n#EXTINF:4,\n11.ts\n#EXTINF:4,\n12.ts\n#EXTINF:4,\n13.ts\n#EXTINF:4,\n14.ts\n#EXTINF:4,\n15.ts\n", BASE_URL, &proxy, b"secret", 0,
        ).unwrap();
        let body = state.transient.last_manifest_body.clone().unwrap();
        let ids = crate::HlsPublishedTransientResourceIds::from_manifest_body(&body);
        let snapshot = crate::manifest_snapshot::derive_hls_lease_manifest_snapshot(
            &crate::manifest_snapshot::HlsLeaseManifestSnapshotInput::TransientPassthrough {
                materialized_body: &body,
                source_commit_identity: HlsManifestCommitIdentity::new(1),
                finalized_manifest_generation: None,
            },
            0,
        )
        .unwrap()
        .unwrap();
        state.record_live_window_publication_with_resources(
            &HlsAccessLeaseId("transient".into()),
            0,
            &snapshot,
            ids.clone(),
        );
        let resource = state
            .transient
            .resources
            .values()
            .find(|resource| resource.resolved_origin_uri.ends_with("shared.ts"))
            .unwrap()
            .clone();
        let TransientObjectFetchDecision::Fetch(token) =
            state.transient.begin_object_fetch(&proxy, &resource, "ts", 0, 100)
        else {
            panic!("initial full object");
        };
        let key = token.cache_key().clone();
        gc.cache.write_bytes_and_commit(&key, b"original").await.unwrap();
        assert!(state.transient.mark_object_ready_if_current(&token, "video/mp2t".into(), 8, 0, 100));
        let mut next = snapshot;
        next.delivered_at_ms = 10_000;
        next.visible_segments = next.visible_segments[1..].to_vec().into();
        next.first_proxy_seq = 11;
        next.playlist_duration_ms = 20_000;
        let next_ids = crate::HlsPublishedTransientResourceIds::from_manifest_body(
            &next.visible_segments.iter().map(|unit| unit.uri.as_ref()).collect::<Vec<_>>().join("\n"),
        );
        state.record_live_window_publication_with_resources(&HlsAccessLeaseId("transient".into()), 0, &next, next_ids);
        (resource, key, ids)
    };
    assert_eq!(gc.run_once(37_999).await.unwrap().transient_objects_deleted, 0);
    {
        let mut state = session.write().await;
        assert!(state
            .transient
            .resolve_resource_for_lease(&old.id, &HlsAccessLeaseId("transient".into()), 0, &ids, 37_999)
            .is_some());
        assert!(
            !state.transient.origin_resource_is_fetchable(&old, 37_999),
            "retention cannot authorize another origin fetch"
        );
        let lookup = TransientPassthroughState::transient_object_key(&state.proxy_session_id, &old.id, "ts");
        assert!(state.transient.ready_object(&lookup, TransientResourceKind::Segment, 37_999).is_some());
    }
    assert_eq!(tokio::fs::read(gc.cache.object_path(&old_key)).await.unwrap(), b"original");
    // A new committed manifest no longer owns the old semantic mapping.
    session.write().await.transient.replace_manifest_with_semantics(
        "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\nother.ts\n".into(),
        10_000,
        Some(4_000),
    );
    let report = gc.run_once(38_000).await.unwrap();
    assert_eq!(report.transient_objects_deleted, 1);
    assert!(!session.read().await.transient.resources.contains_key(&old.id));
    assert!(gc.cache.metadata(&old_key).await.unwrap().is_none());
}

#[tokio::test]
async fn removed_encrypted_publication_keeps_its_map_key_and_original_bindings() {
    let (_directory, gc, session) = retained_fixture(0).await;
    let (key_resource, key_cache, map_cache, published_ids) = {
        let mut state = session.write().await;
        let resource = TransientResourceRef::new(
            TransientResourceKind::Key,
            "http://origin.example.com/old.key",
            b"secret",
            0,
            100,
            Some("bin".into()),
        );
        state.transient.upsert_resources([resource.clone()]);
        let resource = state.transient.resources[&resource.id].clone();
        let map = crate::MapEntry::new(
            &state.proxy_session_id,
            ProxyMapId(9),
            OriginMapKey {
                origin_epoch: 0,
                resolved_origin_uri: "http://origin.example.com/old.mp4".into(),
                byte_range: None,
            },
            "mp4".into(),
        );
        let map_cache = map.cache_key.clone();
        state.maps.insert(ProxyMapId(9), map);
        state.maps.get_mut(&ProxyMapId(9)).unwrap().status =
            MapCacheStatus::Ready { content_length: 4, ready_at_ms: 0 };
        let entry = state.segments.get_mut(&1).unwrap();
        entry.map_ref = Some(ProxyMapId(9));
        entry.encryption = Some(HlsSegmentEncryption {
            resource_id: resource.id.clone(),
            resource_extension: "bin".into(),
            iv: Some("0x01".into()),
            key_format: None,
            key_format_versions: None,
        });
        let ids = crate::HlsPublishedTransientResourceIds::from_manifest_body(&format!(
            "/hls/shared/live/{}/lease/r/{}.bin",
            state.proxy_session_id.0, resource.id.0
        ));
        assert!(ids.contains(&resource.id));
        let snapshot = publication(&state, &[1, 2, 3, 4, 5, 6], 5_000);
        state.record_live_window_publication_with_resources(
            &HlsAccessLeaseId("retention".into()),
            0,
            &snapshot,
            ids.clone(),
        );
        let snapshot = publication(&state, &[2, 3, 4, 5, 6], 10_000);
        state.record_live_window_publication(&HlsAccessLeaseId("retention".into()), 0, &snapshot);
        let proxy = state.proxy_session_id.clone();
        let TransientObjectFetchDecision::Fetch(token) =
            state.transient.begin_object_fetch(&proxy, &resource, "bin", 0, 100)
        else {
            panic!("key cache fixture");
        };
        assert!(state.transient.mark_object_ready_if_current(&token, "application/octet-stream".into(), 16, 0, 100));
        (resource, token.cache_key().clone(), map_cache, ids)
    };
    gc.cache.write_bytes_and_commit(&key_cache, b"0123456789abcdef").await.unwrap();
    gc.cache.write_bytes_and_commit(&map_cache, b"init").await.unwrap();
    let report = gc.run_once(37_999).await.unwrap();
    assert_eq!((report.segments_deleted(), report.maps_deleted, report.transient_objects_deleted), (0, 0, 0));
    let state = session.read().await;
    assert_eq!(state.segments[&1].map_ref, Some(ProxyMapId(9)));
    assert_eq!(state.segments[&1].encryption.as_ref().unwrap().resource_id, key_resource.id);
    assert!(state
        .transient
        .resolve_resource_for_lease(&key_resource.id, &HlsAccessLeaseId("retention".into()), 0, &published_ids, 37_999)
        .is_some());
    assert_eq!(tokio::fs::read(gc.cache.object_path(&key_cache)).await.unwrap(), b"0123456789abcdef");
    assert_eq!(tokio::fs::read(gc.cache.object_path(&map_cache)).await.unwrap(), b"init");
    drop(state);
    let report = gc.run_once(38_000).await.unwrap();
    assert_eq!((report.segments_deleted_duration, report.maps_deleted), (1, 1));
    gc.run_once(38_001).await.unwrap();
    assert!(gc.cache.metadata(&key_cache).await.unwrap().is_none());
}

#[tokio::test]
async fn removed_publication_revision_retires_with_its_segment_after_the_deadline() {
    let (_directory, gc, session) = retained_fixture(0).await;
    let store = Arc::new(crate::SegmentRevisionStore::default());
    gc.install_revision_store(&store);
    let id = session.read().await.proxy_session_id.clone();
    let revision = store.create(id, 1, crate::SegmentRevisionKind::Processed).unwrap();
    revision.revision().bind_origin(&*session.read().await);
    let key = revision.revision().key.clone();
    let metadata = gc.cache.write_bytes_and_commit(&key, b"revision").await.unwrap();
    revision.revision().complete(metadata);
    *revision.revision().file_pin.lock().unwrap() =
        Some(gc.cache.pin_revision_file(&gc.cache.object_path(&key)).unwrap());
    let config = tuliprox_core::model::HlsStartupConfig::default();
    session.write().await.startup = Some(crate::HlsSessionStartup {
        config: config.clone(),
        worker: std::sync::Weak::new(),
        access_leases: Arc::new(RwLock::new(HlsAccessLeaseStore::default())),
        revisions: [(1, revision)].into(),
        policy_fixed: true,
        store,
        budget: crate::ProgressiveBudgetManager::new(config),
    });
    assert_eq!(gc.run_once(37_999).await.unwrap().revision_deletions_succeeded, 0);
    assert_eq!(tokio::fs::read(gc.cache.object_path(&key)).await.unwrap(), b"revision");
    assert!(session.read().await.startup.as_ref().unwrap().revisions.contains_key(&1));
    let report = gc.run_once(38_000).await.unwrap();
    assert_eq!(report.segments_deleted_duration, 1);
    assert_eq!(report.revision_deletions_succeeded, 1);
    assert!(session.read().await.startup.as_ref().unwrap().revisions.is_empty());
    assert!(gc.cache.metadata(&key).await.unwrap().is_none());
}

#[tokio::test]
async fn expired_lease_releases_repeated_publication_evidence_and_allows_idle_cleanup() {
    let (_directory, gc, session) = retained_fixture(0).await;
    let id = session.read().await.proxy_session_id.clone();
    let leases = Arc::new(RwLock::new(HlsAccessLeaseStore::default()));
    gc.install_access_leases(&leases);
    let lease_id = HlsAccessLeaseId("retention".into());
    let lease = HlsAccessLease::pending(
        lease_id.clone(),
        HlsPlaybackFamilyKey::new("user", "fingerprint"),
        id.clone(),
        "user".into(),
        "token".into(),
        1,
        "stream".into(),
        1,
        0,
        11_000,
    );
    let identity = lease.media_identity().unwrap();
    assert!(leases.write().await.prepare_access_lease(lease));
    {
        let mut state = session.write().await;
        for at in 10_000..10_100 {
            let snapshot = publication(&state, &[2, 3, 4, 5, 6], at);
            state.record_live_window_publication(&lease_id, 0, &snapshot);
        }
        assert_eq!(state.live_window_demands.len(), 1);
    }
    assert_eq!(gc.run_once(10_999).await.unwrap().sessions_deleted, 0);
    assert_eq!(gc.run_once(11_000).await.unwrap().sessions_deleted, 0);
    assert_eq!(
        leases.write().await.response_snapshot(&lease_id, &id, 11_000).unwrap().state,
        crate::HlsAccessLeaseState::Expired
    );
    assert!(
        !leases.write().await.media_identity_is_current(&lease_id, &id, identity, 11_000),
        "cache retention cannot extend lease authorization"
    );
    assert!(session.read().await.live_window_demands.is_empty());
    assert_eq!(gc.run_once(34_999).await.unwrap().sessions_deleted, 0);
    // Remaining units had a longest containing playlist of 24 seconds plus four seconds of media.
    assert_eq!(gc.run_once(39_000).await.unwrap().sessions_deleted, 1);
    assert_eq!(gc.sessions.len().await, 0);
    assert_eq!(gc.cache.capacity_usage(&id).await.unwrap().session_bytes, 0);
}
