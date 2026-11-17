use super::*;

#[tokio::test]
async fn stale_restart_probe_cannot_write_or_delete_the_successor_cache_files() {
    for mutation in ["work", "account", "acceptance"] {
        let origin = Origin::start().await;
        let mut request = ready_window(&origin).await;
        let old_entry = request.session.read().await.segments[&3].clone();
        let old_revision = request.session.read().await.startup.as_ref().unwrap().revisions[&3].clone();
        authorize_reentry(&mut request).await;
        *origin.state.new_namespace_bytes.write().await = Some(media(7));
        *origin.state.pause_query.write().await = Some("token=paused".into());
        let captured = request.clone();
        let paused = playlist(&origin.base, "fedcba9876543210", 4, "?token=paused");
        let task = tokio::spawn(async move {
            crate::refresh::origin_probe::prepare_origin_update(
                &captured,
                &paused,
                captured.manifest_commit_requirement.acceptance_mode(),
            )
            .await
        });
        tokio::time::timeout(Duration::from_secs(5), origin.state.entered.notified()).await.unwrap();
        {
            let mut state = request.session.write().await;
            match mutation {
                "work" => state.activity.origin_work_generation += 1,
                "account" => state.transient.invalidate_origin_resources(),
                _ => state.origin_control.acceptance_generation.0 += 1,
            }
        }
        let successor = playlist(&origin.base, "fedcba9876543210", 4, "?token=successor");
        let result = candidate(&request, successor).await;
        assert!(result.is_ok(), "{mutation}: {:?}", result.err());
        let (entries, key, revision) = {
            let state = request.session.read().await;
            assert_eq!(state.origin_epoch, 1);
            assert_eq!(state.proxy_next_seq, Some(8));
            (
                state.segments.clone(),
                state.segments[&4].cache_key.clone(),
                state.startup.as_ref().unwrap().revisions[&4].clone(),
            )
        };
        *origin.state.pause_query.write().await = None;
        origin.state.release.notify_one();
        assert!(task.await.unwrap().is_err(), "{mutation}: stale restart proof cannot enter cache publication");
        request.hls_proxy.garbage_collector().run_once(request.now_ms).await.unwrap();
        assert_eq!(request.session.read().await.segments, entries);
        assert_eq!(old_entry.access.active_readers(), 0);
        assert_eq!(tokio::fs::read(request.segment_cache.object_path(&key)).await.unwrap(), media(7));
        assert_eq!(
            tokio::fs::read(revision.revision().wait_complete(tokio::time::Instant::now()).await.unwrap().path)
                .await
                .unwrap(),
            media(7)
        );
        assert_eq!(
            tokio::fs::read(old_revision.revision().wait_complete(tokio::time::Instant::now()).await.unwrap().path)
                .await
                .unwrap(),
            media(1)
        );
    }
}

#[tokio::test]
async fn stale_hash_probe_cannot_replace_or_clean_a_successful_successor() {
    for mutation in ["work", "account", "acceptance"] {
        let origin = Origin::start().await;
        let request = ready_window(&origin).await;
        let original = request.session.read().await.segments[&3].clone();
        let revision = request.session.read().await.startup.as_ref().unwrap().revisions[&3].clone();
        *origin.state.pause_query.write().await = Some("token=paused".into());
        let old_request = request.clone();
        let old_fetched = rotate_tail(window(&origin, 101, 4, "old"), "paused");
        let old_body = old_fetched.clone();
        let pending = tokio::spawn(async move {
            crate::refresh::origin_probe::prepare_origin_update(
                &old_request,
                &old_fetched,
                HlsManifestCommitAcceptanceMode::StrictPinnedHost,
            )
            .await
        });
        tokio::time::timeout(Duration::from_secs(5), origin.state.entered.notified()).await.unwrap();
        assert!(original.access.active_readers() > 0);
        {
            let mut state = request.session.write().await;
            match mutation {
                "work" => state.activity.origin_work_generation += 1,
                "account" => state.transient.invalidate_origin_resources(),
                _ => state.origin_control.acceptance_generation.0 += 1,
            }
        }
        let successor = rotate_tail(window(&origin, 101, 4, "old"), "successor");
        assert!(candidate(&request, successor.clone()).await.is_ok());
        let before = request.session.read().await.segments.clone();
        origin.state.release.notify_one();
        if let Ok(Some(update)) = pending.await.unwrap() {
            let mut state = request.session.write().await;
            assert!(crate::refresh::commit::commit_fetched_manifest_with_origin_update(
                &mut state,
                &old_body,
                &request,
                request.now_ms,
                HlsManifestCommitAcceptanceMode::StrictPinnedHost,
                None,
                Some(&update)
            )
            .is_err());
        }
        request.hls_proxy.garbage_collector().run_once(request.now_ms).await.unwrap();
        let state = request.session.read().await;
        assert_eq!(state.segments, before);
        assert_eq!(HlsOriginContentEvidence::verified_bindings(&state).len(), 1);
        assert_eq!(state.segments[&3].origin_fetch_ref, original.origin_fetch_ref);
        assert_eq!(original.access.active_readers(), 0);
        drop(state);
        assert_eq!(tokio::fs::read(request.segment_cache.object_path(&original.cache_key)).await.unwrap(), media(1));
        assert_eq!(
            tokio::fs::read(revision.revision().wait_complete(tokio::time::Instant::now()).await.unwrap().path)
                .await
                .unwrap(),
            media(1)
        );
        assert!(crate::refresh::origin_probe::commit_pinned_manifest(
            &request,
            &successor,
            HlsManifestCommitAcceptanceMode::StrictPinnedHost
        )
        .await
        .is_ok());
    }
}
