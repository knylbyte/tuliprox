use super::*;

#[tokio::test]
async fn removed_startup_revision_route_keeps_its_full_and_range_bytes_after_gc() {
    let fixture = CachedSegmentFixture::new(b"0123456789".to_vec()).await;
    let old = publish_revision_fixture(&fixture, false).await.unwrap();
    let base = current_time_millis();
    let lease = fixture
        .manager
        .access_lease_response_snapshot(&fixture.lease_id, &fixture.proxy_session_id, base)
        .await
        .unwrap();
    let mut next = lease.last_manifest_snapshot.unwrap();
    {
        let mut state = fixture.session.write().await;
        state.record_live_window_publication(&fixture.lease_id, lease.issued_at_ms, &next);
        let mut successor = state.segments[&12].clone();
        successor.proxy_seq = 13;
        successor.cache_key = SegmentCacheKey::new(fixture.proxy_session_id.clone(), 13, "ts");
        successor.access = Arc::new(CacheAccessState::new());
        successor.media_start_ms += 4_000;
        state.segments.insert(13, successor);
        state.publishable_origin_head_proxy_seq = Some(13);
    }
    let successor = {
        let state = fixture.session.read().await;
        state
            .startup
            .as_ref()
            .unwrap()
            .store
            .create(fixture.proxy_session_id.clone(), 13, crate::SegmentRevisionKind::Processed)
            .unwrap()
    };
    successor.revision().bind_origin(&*fixture.session.read().await);
    let metadata =
        fixture.segment_cache.write_bytes_and_commit(&successor.revision().key, b"ABCDEFGHIJ").await.unwrap();
    successor.revision().complete(metadata);
    fixture.session.write().await.startup.as_mut().unwrap().revisions.insert(13, successor.clone());
    next.delivered_at_ms = base + 10_000;
    next.first_proxy_seq = 13;
    next.last_proxy_seq = 13;
    next.visible_segments = Arc::from([crate::HlsLeaseManifestSegment {
        proxy_seq: 13,
        uri: "13.ts".into(),
        ..next.visible_segments[0].clone()
    }]);
    let publication = Arc::make_mut(next.startup_revisions.as_mut().unwrap());
    publication.retained_start_seq = 13;
    publication.revisions = [(13, successor)].into();
    {
        let mut leases = fixture.manager.access_leases().write().await;
        let guard =
            leases.prepare_manifest_publication(&fixture.lease_id, &fixture.proxy_session_id, base + 10_000).unwrap();
        assert!(leases
            .commit_manifest_publication(
                &fixture.lease_id,
                &fixture.proxy_session_id,
                guard,
                next.clone(),
                base + 10_000
            )
            .is_committed());
        assert_eq!(leases.published_segment_revision(&fixture.lease_id, 12).unwrap(), old);
    }
    fixture.session.write().await.record_live_window_publication(&fixture.lease_id, lease.issued_at_ms, &next);
    let report = fixture.manager.run_garbage_collection_once(base + 17_999).await.unwrap();
    assert_eq!(report.revision_deletions_succeeded, 0);
    for range in [None, Some("bytes=2-5")] {
        let response = ready_response(fixture.serve(range).await);
        if range.is_some() {
            assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
            assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes 2-5/10");
            assert_eq!(response.headers()[header::CONTENT_LENGTH], "4");
            assert_eq!(response.into_body().collect().await.unwrap().to_bytes(), Bytes::from_static(b"2345"));
        } else {
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.into_body().collect().await.unwrap().to_bytes(), Bytes::from_static(b"0123456789"));
        }
    }
    assert_eq!(
        fixture.manager.access_leases().read().await.published_segment_revision(&fixture.lease_id, 12).unwrap(),
        old
    );
}
