use super::{
    build_hls_origin_resource_headers, commit_startup_response, ensure_segment_key_dependency_ready,
    hls_object_body_deadline, reliable_decoded_content_length, CachedSegmentMetadata, HlsOriginByteRangeExpectation,
    HlsOriginResourceBodyDeadline, HlsOriginResourceClients, HlsOriginResourceFetchTarget, HlsRepairRenderedObjectId,
    HlsResourceFetchKind, HlsResourceFetchSource, HlsSegmentRepairObjectContext, HlsSegmentRepairSource,
    SegmentFetchCommit, SegmentFetchContext, SegmentFetchError, SegmentFetchFailure, SegmentFetchPolicy,
    SegmentFetchSnapshot, SegmentOriginWorkFinish,
};
use crate::{
    origin_attempt::{prepare_media_origin_attempt, HlsMediaOriginAttemptGuard as SegmentOriginAttemptGuard},
    resource_fetch::{run_hls_origin_resource_retry_loop_observing_headers, HlsOriginResourceRetryRun},
};
use axum::http::HeaderMap;
use futures::FutureExt;
use tuliprox_core::utils::content_coding::DecodedHttpResponse;
use tuliprox_parser::hls::origin_manifest::ParsedByteRange;

pub(super) async fn fetch_segment_into_cache(
    context: &SegmentFetchContext,
    snapshot: &SegmentFetchSnapshot,
    policy: &SegmentFetchPolicy,
) -> Result<SegmentFetchCommit, SegmentFetchFailure> {
    #[cfg(any(test, feature = "test-support"))]
    {
        let barrier = context
            .session
            .write()
            .await
            .startup_request_barrier
            .take_if(|(proxy_seq, _)| *proxy_seq == snapshot.proxy_seq);
        if let Some((_, barrier)) = barrier {
            barrier.prepare_ready.notify_one();
            barrier.resume_prepare.notified().await;
        }
    }
    if let Some(predecessor) = snapshot.startup_request.as_ref().and_then(|request| request.predecessor.as_ref()) {
        let cancellation = {
            let session = context.session.read().await;
            if session.activity.origin_work_generation != snapshot.origin_work_generation {
                return Err(SegmentFetchError::Superseded.into());
            }
            session.origin_work_cancellation.clone()
        };
        tokio::select! {
            biased;
            () = cancellation.cancelled() => return Err(SegmentFetchError::Superseded.into()),
            _ = predecessor.acquire() => {}
        }
    }
    // The first cache write scans the whole cache directory; do it before any per-attempt deadline starts.
    context.segment_cache.prepare_capacity_accounting().await.map_err(|error| SegmentFetchError::cache_body(&error))?;
    let target = context.session.read().await.target_duration;
    let budget = policy.demand_wait_timeout_for_session(
        if snapshot.key_dependency.is_some() {
            super::HlsSegmentFetchWorkload::EncryptedWithKey
        } else {
            super::HlsSegmentFetchWorkload::Clear
        },
        target,
    );
    let now = tokio::time::Instant::now();
    let work = crate::HlsOriginRetryWork::begin(
        &context.session,
        context.origin_io.as_ref().map(|io| &io.ctx.active_provider),
        now.checked_add(budget).unwrap_or(now),
    )
    .await;
    let mut awaiting_key = false;
    let run = async {
        if let (Some(work), Some(io)) = (&work, &context.origin_io) {
            work.protect(&io.ctx.active_provider, &context.session, snapshot.origin_work_generation, false).await?;
        }
        awaiting_key = snapshot.key_dependency.is_some();
        ensure_segment_key_dependency_ready(context, snapshot, policy, work.clone()).await?;
        awaiting_key = false;
        fetch_segment_with_retries_into_cache(context, snapshot, policy, work.clone())
            .await
            .map_err(SegmentFetchFailure::Media)
    };
    if let Some(work) = &work {
        match tokio::time::timeout_at(work.deadline(), run).await {
            Ok(result) => result,
            Err(_) if awaiting_key => Err(SegmentFetchFailure::KeyUnavailable(SegmentFetchError::Timeout)),
            Err(_) => Err(SegmentFetchError::Timeout.into()),
        }
    } else {
        run.await
    }
}

pub(super) async fn prepare_segment_origin_attempt(
    context: SegmentFetchContext,
    attempt: crate::HlsResourceFetchAttempt,
    expected_generation: u64,
) -> Result<SegmentOriginAttemptGuard, SegmentFetchError> {
    if let Some(io) = &context.origin_io {
        attempt.protect_origin_work(&io.ctx.active_provider, &context.session, expected_generation, false).await?;
    }
    let guard = prepare_media_origin_attempt(
        context.origin_io.as_ref(),
        &context.session,
        expected_generation,
        hls_object_body_deadline(attempt.timeout.effective_ms),
    )
    .await
    .map_err(SegmentFetchError::ProviderUnavailable)?;
    if let Some(io) = &context.origin_io {
        attempt.protect_origin_work(&io.ctx.active_provider, &context.session, expected_generation, true).await?;
    }
    Ok(guard)
}

pub(super) async fn finish_segment_origin_attempt(
    context: SegmentFetchContext,
    guard: SegmentOriginAttemptGuard,
) -> SegmentOriginWorkFinish {
    guard.finish(&context.session).await
}

#[allow(clippy::too_many_lines)]
pub(super) async fn fetch_segment_with_retries_into_cache(
    context: &SegmentFetchContext,
    snapshot: &SegmentFetchSnapshot,
    policy: &SegmentFetchPolicy,
    retry_work: Option<std::sync::Arc<crate::HlsOriginRetryWork>>,
) -> Result<SegmentFetchCommit, SegmentFetchError> {
    let (provider_headers, media_duration_ms) = {
        let session = context.session.read().await;
        (
            session.origin_provider_session_headers.clone(),
            session.segments.get(&snapshot.proxy_seq).map(|entry| entry.duration_ms),
        )
    };
    let headers = build_segment_origin_headers(&context.headers, &provider_headers, snapshot.fetch_ref.byte_range)?;
    let target = HlsOriginResourceFetchTarget {
        kind: HlsResourceFetchKind::Segment,
        source: HlsResourceFetchSource::Normal,
        object_id: snapshot.proxy_seq_log.clone(),
        origin_url: snapshot.fetch_ref.resolved_origin_url.clone(),
        headers,
        byte_range_expectation: if snapshot.fetch_ref.byte_range.is_some() {
            HlsOriginByteRangeExpectation::PartialContent
        } else {
            HlsOriginByteRangeExpectation::FullObject
        },
        media_duration_ms,
    };
    let clients = HlsOriginResourceClients {
        retry_work,
        session: Some(context.session.clone()),
        client: context.client.clone(),
        no_redirect_client: context.no_redirect_client.clone(),
        use_manual_redirects: context.use_manual_redirects,
    };
    let log_identity = {
        let session = context.session.read().await;
        super::super::HlsLogIdentity::from_session(&session)
    };
    let expected_generation = snapshot.origin_work_generation;
    let response_headers = snapshot.startup_request.as_ref().map(|request| std::sync::Arc::clone(&request.headers));
    let context = context.clone();
    let snapshot = snapshot.clone();
    let prepare_context = context.clone();
    let cleanup_context = context.clone();
    run_hls_origin_resource_retry_loop_observing_headers(
        HlsOriginResourceRetryRun { target, clients, policy, identity: &log_identity, response_headers },
        move |attempt| {
            let context = prepare_context.clone();
            async move { prepare_segment_origin_attempt(context, attempt, expected_generation).await }.boxed()
        },
        move |guard| {
            let context = cleanup_context.clone();
            async move {
                finish_segment_origin_attempt(context, guard).await;
            }
            .boxed()
        },
        move |mut response, _attempt, body_deadline, guard| {
            let context = context.clone();
            let snapshot = snapshot.clone();
            async move {
                let digest = crate::refresh::origin_probe::OriginContentDigest::default();
                response.body = digest.observe(response.body);
                let commit_result =
                    commit_segment_response_into_cache(&context, &snapshot, response, body_deadline).await;
                let origin_work = finish_segment_origin_attempt(context, guard).await;
                commit_result.map(|metadata| SegmentFetchCommit {
                    content_length: metadata.size,
                    generation_valid: origin_work.generation_valid,
                    origin_digest: digest.completed(),
                })
            }
            .boxed()
        },
    )
    .await
}

async fn commit_segment_response_into_cache(
    context: &SegmentFetchContext,
    snapshot: &SegmentFetchSnapshot,
    response: DecodedHttpResponse,
    body_deadline: HlsOriginResourceBodyDeadline,
) -> Result<CachedSegmentMetadata, SegmentFetchError> {
    let (proxy_session_id, log_identity, accelerated) = {
        let session = context.session.read().await;
        (
            session.proxy_session_id.clone(),
            super::super::HlsLogIdentity::from_session(&session),
            session.startup.is_some(),
        )
    };
    let repair_context = HlsSegmentRepairObjectContext {
        source: HlsSegmentRepairSource::Normal,
        log_identity,
        proxy_session_id,
        hls_access_lease_id: context.repair_access_lease_id.clone(),
        rendered_object_id: HlsRepairRenderedObjectId::Normal { proxy_seq: snapshot.proxy_seq },
        resource_id: format!("{:06}", snapshot.proxy_seq),
        file_ext: snapshot.proxy_file_ext.clone(),
        // Segment repair uses the concrete fetch URL for diagnostics/postprocess metadata only.
        origin_fetch_uri_for_diagnostics: snapshot.fetch_ref.resolved_origin_url.clone(),
        media_sequence: Some(snapshot.origin_seq),
        discontinuity_sequence: None,
        complete_object: snapshot.complete_object,
        encrypted: snapshot.encryption.is_some(),
        custom_response: false,
    };
    if accelerated {
        return commit_startup_response(context, snapshot, response, body_deadline.deadline(), repair_context).await;
    }
    if let Some(content_length) = reliable_decoded_content_length(&response) {
        context
            .segment_cache
            .ensure_projected_write_capacity(&snapshot.cache_key, content_length)
            .await
            .map_err(|error| SegmentFetchError::cache_body(&error))?;
    }
    context
        .segment_repair
        .commit_origin_response(
            &context.segment_cache,
            &snapshot.cache_key,
            response.body,
            body_deadline.deadline(),
            repair_context,
        )
        .await
        .map_err(|err| SegmentFetchError::cache_body(&err))
}

pub(super) fn build_segment_origin_headers(
    source_headers: &HeaderMap,
    provider_session_headers: &HeaderMap,
    byte_range: Option<ParsedByteRange>,
) -> Result<HeaderMap, SegmentFetchError> {
    build_hls_origin_resource_headers(source_headers, provider_session_headers, byte_range)
}
