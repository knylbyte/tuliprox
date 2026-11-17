use super::{
    access_lease, hls_key_readiness_evidence_is_current, live_media_fixture, manifest_snapshot, terminal_plan,
    HlsAccessLeaseDenialMode, HlsAccessLeaseDenialOutcome, HlsLeaseManifestPublicationOutcome,
    HlsLeaseManifestPublicationRejectReason, HlsMediaActivityCommitOutcome, HlsProxyManager, HlsTerminalTailProtection,
};
use crate::{
    terminal_tail::HlsLeasePlaybackMode, HlsAccessLeaseId, HlsAccessLeasePendingDeadline,
    HlsPublishedTransientResourceIds, HlsSessionKey, ProxySessionId,
};
use std::sync::Arc;

mod recovery_publication;

mod recovery_entry_continuation {
    use super::super::access_lease;
    use crate::{
        lease::{
            HlsLeaseManifestPublicationGuard, HlsLeaseManifestPublicationOutcome as Outcome,
            HlsLeaseManifestPublicationRejectReason as Reject,
        },
        live_window::HlsLiveWindowView,
        manifest_snapshot::{
            derive_hls_lease_manifest_snapshot, derive_hls_recovery_entry_snapshot, HlsLeaseManifestSnapshotInput,
        },
        HlsAccessLeaseId, HlsAccessLeaseState, HlsLeaseManifestSnapshot, HlsManifestRenderer, HlsProxyManager,
        HlsSession, HlsSessionKey, ProxySessionId, SegmentCacheStatus, SegmentRevisionKind, SegmentRevisionState,
    };
    use shared::model::{HlsCacheConfigDto, HlsStartupMode};
    use std::sync::Arc;
    use tuliprox_core::model::HlsCacheConfig;
    use tuliprox_parser::hls::origin_manifest::{parse_origin_media_manifest, OriginManifestParseOutcome};

    const NOW: u64 = 2_000;
    const MEDIA: &[u8] = b"fixture media";

    struct Publication {
        _cache: tempfile::TempDir,
        manager: HlsProxyManager,
        lease: HlsAccessLeaseId,
        proxy: ProxySessionId,
        previous: HlsLeaseManifestSnapshot,
        candidate: HlsLeaseManifestSnapshot,
        older_origin: HlsLeaseManifestSnapshot,
    }

    fn render_candidate(state: &HlsSession) -> HlsLeaseManifestSnapshot {
        let plan = state.plan_live_window(HlsLiveWindowView::Active { first_published: 3 }).unwrap();
        let rendered = HlsManifestRenderer::render_live_window(state, &plan, NOW).unwrap();
        derive_hls_lease_manifest_snapshot(
            &HlsLeaseManifestSnapshotInput::FastStartWindow {
                session: state,
                plan: &plan,
                rendered: &rendered,
                materialized_body: &rendered.body,
            },
            NOW,
        )
        .unwrap()
        .unwrap()
    }

    fn store_session_window(state: &mut HlsSession) {
        let plan = state.plan_live_window(HlsLiveWindowView::Active { first_published: 2 }).unwrap();
        let source = HlsManifestRenderer::render_live_window(state, &plan, NOW).unwrap();
        assert!(matches!(state.store_rendered_manifest(source), crate::RenderedManifestStoreOutcome::Stored));
    }

    fn fallback_snapshot(state: &HlsSession, manager: &HlsProxyManager) -> HlsLeaseManifestSnapshot {
        let (rendered, plan) = HlsManifestRenderer::render_recovery_entry_window(state, &manager.strip(), NOW).unwrap();
        derive_hls_recovery_entry_snapshot(state, plan.as_ref(), &rendered, &rendered.body, 0, NOW).unwrap().unwrap()
    }

    impl Publication {
        async fn new() -> Self {
            let cache = tempfile::tempdir().unwrap();
            let mut config = HlsCacheConfigDto {
                cache_path: Some(cache.path().to_string_lossy().into_owned()),
                ..Default::default()
            };
            config.startup.mode = HlsStartupMode::FirstReady;
            config.strip.value = 2;
            let manager = HlsProxyManager::with_hls_cache_config(&HlsCacheConfig::from(&config));
            manager.freeze_clock_for_test(NOW);
            let session =
                manager.get_or_create_session(HlsSessionKey::new(1, "recovery-follow-up"), b"fixture", NOW).await;
            let lease = HlsAccessLeaseId("viewer-b".into());
            let (proxy, fallback, candidate, older_origin) = {
                let mut state = session.write().await;
                let proxy = state.proxy_session_id.clone();
                let body = "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:102\n#EXTINF:4,\n2.ts\n#EXTINF:4,\n3.ts\n#EXTINF:4,\n4.ts\n#EXTINF:4,\n5.ts\n#EXTINF:4,\n6.ts\n#EXTINF:4,\n7.ts\n";
                let OriginManifestParseOutcome::Normal(manifest) =
                    parse_origin_media_manifest(body, "http://origin.example/index.m3u8")
                else {
                    panic!("fixture manifest")
                };
                state.proxy_next_seq = Some(2);
                state.apply_origin_manifest(&manifest).unwrap();
                state.record_origin_manifest_commit();
                state.record_live_window_origin_commit(NOW);
                for seq in 2..=7 {
                    let revision = state
                        .startup
                        .as_ref()
                        .unwrap()
                        .store
                        .create(proxy.clone(), seq, SegmentRevisionKind::Raw)
                        .unwrap();
                    let metadata =
                        manager.segment_cache().write_bytes_and_commit(&revision.revision().key, MEDIA).await.unwrap();
                    let size = metadata.size;
                    revision.revision().bind_origin(&state);
                    assert!(revision.revision().complete(metadata));
                    state.startup.as_mut().unwrap().revisions.insert(seq, revision);
                    state.segments.get_mut(&seq).unwrap().status =
                        SegmentCacheStatus::Ready { content_length: size, ready_at_ms: NOW };
                }
                let older_origin = render_candidate(&state);
                store_session_window(&mut state);
                state.apply_origin_manifest(&manifest).unwrap();
                state.record_origin_manifest_commit();
                state.record_live_window_origin_commit(NOW);
                // A later origin commit can exist before the session renders it.
                // Recovery provenance must still come from the committed render.
                let previous_render = fallback_snapshot(&state, &manager);
                assert_eq!(previous_render.cached_source.as_ref().unwrap().origin_commits, 1);
                store_session_window(&mut state);
                let candidate = render_candidate(&state);
                let fallback = fallback_snapshot(&state, &manager);
                assert_eq!(fallback.cached_source.as_ref().unwrap().origin_commits, 2);
                (proxy, fallback, candidate, older_origin)
            };
            {
                let mut leases = manager.access_leases().write().await;
                let mut active = access_lease(&lease.0, &proxy);
                active.state = HlsAccessLeaseState::Activated;
                active.active_until_ms = Some(NOW + 30_000);
                leases.prepare_access_lease(active);
                let guard = leases.prepare_manifest_publication(&lease, &proxy, NOW).unwrap();
                assert!(leases.commit_manifest_publication(&lease, &proxy, guard, fallback, NOW).is_committed());
            }
            let previous = manager
                .access_lease_response_snapshot(&lease, &proxy, NOW)
                .await
                .unwrap()
                .last_manifest_snapshot
                .unwrap();
            assert_eq!(previous.visible_proxy_seqs().collect::<Vec<_>>(), [3, 4, 5]);
            assert_eq!(candidate.visible_proxy_seqs().collect::<Vec<_>>(), [3, 4, 5, 6, 7]);
            assert!(previous.source_commit_identity > candidate.source_commit_identity);
            Self { _cache: cache, manager, lease, proxy, previous, candidate, older_origin }
        }

        async fn guard(&self) -> HlsLeaseManifestPublicationGuard {
            self.manager.prepare_access_lease_manifest_publication(&self.lease, &self.proxy, NOW).await.unwrap()
        }

        async fn snapshot(&self) -> HlsLeaseManifestSnapshot {
            self.manager
                .access_lease_response_snapshot(&self.lease, &self.proxy, NOW)
                .await
                .unwrap()
                .last_manifest_snapshot
                .unwrap()
        }

        async fn commit(&self, guard: HlsLeaseManifestPublicationGuard, snapshot: HlsLeaseManifestSnapshot) -> Outcome {
            self.manager.commit_access_lease_manifest_publication(&self.lease, &self.proxy, guard, snapshot, NOW).await
        }

        async fn assert_rejected(&self, label: &str, snapshot: HlsLeaseManifestSnapshot, reason: Reject) {
            let outcome = self.commit(self.guard().await, snapshot).await;
            assert_eq!(outcome, Outcome::Rejected(reason), "{label}");
            assert_eq!(self.snapshot().await, self.previous, "{label}");
            let leases = self.manager.access_leases().read().await;
            let previous = self.previous.startup_revisions.as_ref().unwrap();
            for (seq, revision) in &previous.revisions {
                assert_eq!(leases.published_segment_revision(&self.lease, *seq).as_ref(), Some(revision), "{label}");
            }
            assert!(leases.published_segment_revision(&self.lease, 6).is_none(), "{label}");
            eprintln!(
                "continuation_rejection case={label} outcome={outcome:?} snapshot_generation={}",
                self.previous.snapshot_generation
            );
        }
    }

    #[tokio::test]
    async fn manifest_publication_rejects_regressive_sources_and_window_bounds() {
        let publication = Publication::new().await;
        publication.assert_rejected("older_origin", publication.older_origin.clone(), Reject::SourceRegressive).await;
        let mut head = publication.candidate.clone();
        head.first_proxy_seq = 2;
        publication.assert_rejected("regressive_head", head, Reject::SourceRegressive).await;
        let mut tail = publication.candidate.clone();
        tail.last_proxy_seq = 4;
        publication.assert_rejected("regressive_tail", tail, Reject::SourceRegressive).await;
    }

    #[tokio::test]
    async fn manifest_publication_rejects_changed_bindings_gaps_and_missing_files() {
        let publication = Publication::new().await;
        let mut uri = publication.candidate.clone();
        Arc::make_mut(&mut uri.visible_segments)[0].uri = "changed.ts".into();
        publication.assert_rejected("changed_uri", uri, Reject::RevisionConflict).await;
        let mut revision = publication.candidate.clone();
        let revisions = &mut Arc::make_mut(revision.startup_revisions.as_mut().unwrap()).revisions;
        revisions.insert(3, revisions[&4].clone());
        publication.assert_rejected("changed_revision", revision, Reject::RevisionConflict).await;
        let mut discontinuity = publication.candidate.clone();
        discontinuity.discontinuity_sequence += 1;
        publication.assert_rejected("changed_discontinuity", discontinuity, Reject::RevisionConflict).await;
        let mut gap = publication.candidate.clone();
        let mut segments = gap.visible_segments.to_vec();
        segments.remove(3);
        gap.visible_segments = segments.into();
        let source = Arc::make_mut(gap.cached_source.as_mut().unwrap());
        let mut bindings = source.bindings.to_vec();
        bindings.remove(3);
        source.bindings = bindings.into_boxed_slice();
        publication.assert_rejected("gap_at_6", gap, Reject::RevisionConflict).await;
        let new_segment = &publication.candidate.startup_revisions.as_ref().unwrap().revisions[&6];
        let SegmentRevisionState::Complete(metadata) = new_segment.revision().state() else {
            panic!("new segment must be indexed as complete");
        };
        std::fs::remove_file(metadata.path).unwrap();
        publication.assert_rejected("missing_file_at_6", publication.candidate.clone(), Reject::RevisionConflict).await;
    }

    #[tokio::test]
    async fn manifest_publication_fences_delayed_candidate_to_its_fallback_snapshot() {
        let publication = Publication::new().await;
        let delayed_guard = publication.guard().await;
        let delayed_candidate = publication.candidate.clone();
        assert_eq!(
            publication.commit(publication.guard().await, publication.candidate.clone()).await,
            Outcome::Committed { snapshot_generation: publication.previous.snapshot_generation + 1 },
        );
        let winner = publication.snapshot().await;
        assert_eq!(winner.visible_proxy_seqs().collect::<Vec<_>>(), [3, 4, 5, 6, 7]);
        assert!(!winner.is_recovery_entry());
        let outcome = publication.commit(delayed_guard, delayed_candidate).await;
        assert_eq!(outcome, Outcome::Rejected(Reject::SnapshotChanged));
        assert_eq!(publication.snapshot().await, winner);
        let leases = publication.manager.access_leases().read().await;
        for (seq, revision) in &winner.startup_revisions.as_ref().unwrap().revisions {
            assert_eq!(leases.published_segment_revision(&publication.lease, *seq).as_ref(), Some(revision));
        }
        eprintln!("continuation_race winner_generation={} delayed_outcome={outcome:?}", winner.snapshot_generation);
    }
}

#[test]
fn key_readiness_evidence_is_valid_at_expiry_and_stale_one_millisecond_later() {
    assert!(hls_key_readiness_evidence_is_current(Some(50_000), 50_000));
    assert!(!hls_key_readiness_evidence_is_current(Some(50_000), 50_001));
    assert!(hls_key_readiness_evidence_is_current(None, u64::MAX));
}

#[tokio::test]
async fn expired_and_denied_lease_identities_cannot_extend_shared_session_activity() {
    let (expired_manager, expired_session, expired_proxy, expired_lease_id, expired_identity) =
        live_media_fixture("expired-media").await;
    {
        let mut leases = expired_manager.access_leases.write().await;
        let mut lease = leases.remove_access_lease(&expired_lease_id).expect("expiring lease");
        lease.pending_deadline = Some(HlsAccessLeasePendingDeadline::Bootstrap { deadline_ms: 2_000 });
        lease.valid_until_ms = 2_000;
        leases.prepare_access_lease(lease);
    }
    assert_eq!(
        expired_manager
            .mark_authorized_media_access_for_lease_if_identity_matches(
                &expired_session,
                &expired_lease_id,
                &expired_proxy,
                expired_identity,
                2_000,
            )
            .await,
        HlsMediaActivityCommitOutcome::StaleLeaseIdentity
    );
    assert_eq!(expired_session.read().await.activity.last_authorized_media_at_ms, None);

    let (denied_manager, denied_session, denied_proxy, denied_lease_id, denied_identity) =
        live_media_fixture("denied-media").await;
    assert!(matches!(
        denied_manager
            .access_leases
            .write()
            .await
            .deny_access_lease(&denied_lease_id, HlsAccessLeaseDenialMode::ImmediateEnd),
        HlsAccessLeaseDenialOutcome::Ended { terminal_release: None }
    ));
    assert_eq!(
        denied_manager
            .mark_authorized_media_access_for_lease_if_identity_matches(
                &denied_session,
                &denied_lease_id,
                &denied_proxy,
                denied_identity,
                2_000,
            )
            .await,
        HlsMediaActivityCommitOutcome::StaleLeaseIdentity
    );
    assert_eq!(denied_session.read().await.activity.last_authorized_media_at_ms, None);
}

#[tokio::test]
async fn manifest_publication_manager_rejects_delayed_source_and_expiry_races() {
    let manager = HlsProxyManager::new();
    let proxy_session_id = ProxySessionId("proxy".to_string());
    let lease_id = HlsAccessLeaseId("lease-a".to_string());
    manager.access_leases.write().await.prepare_access_lease(access_lease(&lease_id.0, &proxy_session_id));
    let older_request = manager
        .prepare_access_lease_manifest_publication_for_test(&lease_id, &proxy_session_id, 2_000)
        .await
        .expect("older request guard");
    let newer_request = manager
        .prepare_access_lease_manifest_publication_for_test(&lease_id, &proxy_session_id, 2_000)
        .await
        .expect("newer request guard");

    assert_eq!(
        manager
            .commit_access_lease_manifest_publication(
                &lease_id,
                &proxy_session_id,
                newer_request,
                manifest_snapshot(20),
                2_100,
            )
            .await,
        HlsLeaseManifestPublicationOutcome::Committed { snapshot_generation: 1 }
    );
    assert_eq!(
        manager
            .commit_access_lease_manifest_publication(
                &lease_id,
                &proxy_session_id,
                older_request,
                manifest_snapshot(10),
                2_200,
            )
            .await,
        HlsLeaseManifestPublicationOutcome::Rejected(HlsLeaseManifestPublicationRejectReason::SourceRegressive)
    );

    let expiry_lease_id = HlsAccessLeaseId("expiry".to_string());
    manager.access_leases.write().await.prepare_access_lease(access_lease(&expiry_lease_id.0, &proxy_session_id));
    let expiry_request = manager
        .prepare_access_lease_manifest_publication_for_test(&expiry_lease_id, &proxy_session_id, 2_000)
        .await
        .expect("pre-expiry request guard");
    assert_eq!(
        manager
            .commit_access_lease_manifest_publication(
                &expiry_lease_id,
                &proxy_session_id,
                expiry_request,
                manifest_snapshot(30),
                61_000,
            )
            .await,
        HlsLeaseManifestPublicationOutcome::Rejected(HlsLeaseManifestPublicationRejectReason::LeaseExpired)
    );
}

#[tokio::test]
async fn rolling_manifest_publication_uses_lease_store_when_session_is_gc_marked() {
    let manager = HlsProxyManager::new();
    let session = manager.get_or_create_session(HlsSessionKey::new(1, "rolling"), b"secret", 1_000).await;
    let proxy_session_id = session.read().await.proxy_session_id.clone();
    let lease_id = HlsAccessLeaseId("lease-a".to_string());
    manager.access_leases.write().await.prepare_access_lease(access_lease(&lease_id.0, &proxy_session_id));
    let publication = manager
        .prepare_access_lease_manifest_publication_for_test(&lease_id, &proxy_session_id, 2_000)
        .await
        .expect("rolling publication guard");
    let published_resource_ids = HlsPublishedTransientResourceIds::from_manifest_body(
        "#EXTM3U\n#EXTINF:6,\n/hls/shared/live/proxy/lease-a/r/resource.ts\n",
    );
    session.write().await.mark_for_gc_removal();

    assert_eq!(
        manager
            .commit_access_lease_manifest_publication_with_resources(
                &lease_id,
                &proxy_session_id,
                publication,
                manifest_snapshot(20),
                published_resource_ids.clone(),
                2_100,
            )
            .await,
        HlsLeaseManifestPublicationOutcome::Committed { snapshot_generation: 1 }
    );
    let lease = manager
        .access_lease_response_snapshot(&lease_id, &proxy_session_id, 2_100)
        .await
        .expect("published rolling lease");
    assert_eq!(lease.published_transient_resource_ids(), &published_resource_ids);
}

#[tokio::test]
async fn stale_lease_removal_preparation_cannot_release_replacement_protection() {
    let manager = HlsProxyManager::new();
    let (session, _) =
        manager.get_or_create_session_with_outcome(HlsSessionKey::new(1, "stream-a"), b"secret", 1_000).await;
    let proxy_session_id = session.read().await.proxy_session_id.clone();
    let lease_id = HlsAccessLeaseId("replacement-protection".to_string());
    let old_plan = terminal_plan(7, &proxy_session_id, &lease_id);
    let mut old_lease = access_lease(&lease_id.0, &proxy_session_id);
    old_lease.playback_mode = HlsLeasePlaybackMode::TerminalTail(Arc::clone(&old_plan));
    manager.access_leases.write().await.prepare_access_lease(old_lease);
    assert_eq!(
        session.write().await.install_terminal_tail_protection(
            lease_id.clone(),
            HlsTerminalTailProtection {
                generation: old_plan.generation,
                base_proxy_seqs: Arc::clone(&old_plan.protected_base_proxy_seqs),
                key_bindings: old_plan.key_bindings(),
            },
        ),
        super::super::super::session::HlsTerminalTailProtectionInstall::Installed
    );
    let preparation = manager
        .access_leases
        .read()
        .await
        .prepare_access_lease_removal(&lease_id)
        .expect("old lease removal preparation");

    let replacement_plan = terminal_plan(8, &proxy_session_id, &lease_id);
    let mut replacement = access_lease(&lease_id.0, &proxy_session_id);
    replacement.issued_at_ms = 2_000;
    replacement.playback_mode = HlsLeasePlaybackMode::TerminalTail(Arc::clone(&replacement_plan));
    assert!(manager.access_leases.write().await.prepare_access_lease(replacement));
    assert_eq!(
        session.write().await.install_terminal_tail_protection(
            lease_id.clone(),
            HlsTerminalTailProtection {
                generation: replacement_plan.generation,
                base_proxy_seqs: Arc::clone(&replacement_plan.protected_base_proxy_seqs),
                key_bindings: replacement_plan.key_bindings(),
            },
        ),
        super::super::super::session::HlsTerminalTailProtectionInstall::Installed
    );

    assert!(!manager.remove_prepared_access_lease(&lease_id, &preparation).await);
    assert_eq!(
        manager
            .access_leases
            .write()
            .await
            .response_snapshot(&lease_id, &proxy_session_id, 2_000)
            .map(|lease| lease.issued_at_ms),
        Some(2_000)
    );
    assert_eq!(
        session.read().await.terminal_tail_protection(&lease_id).map(|protection| protection.generation),
        Some(replacement_plan.generation)
    );
}
