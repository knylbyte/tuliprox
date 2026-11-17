use super::{
    super::{
        origin_account_runtime::{
            prepare_hls_origin_runtime_for, release_retained_origin_allocation, RetainedOriginPreparation,
        },
        origin_refresh_request::{hls_origin_refresh_request, HlsOriginRefreshOptions},
    },
    access::hls_cache_session_key_for_origin,
    cached::{finish_hls_recovery_entry_response, HlsProvisionalRecoveryEntryResponse},
    clear_hls_provisioning_handoff_consumer, duration_to_millis_saturating,
    effective_hls_url_failover_provider_for_fetch_url, hls_access_lease_timing_for_session, hls_access_lease_ttl_ms,
    hls_availability_reevaluation_registration_failure_response, hls_cache_configured,
    hls_canonical_owner_registration, hls_canonical_owner_request_deadline_ms, hls_canonical_retry_after_response,
    hls_canonical_status_response, hls_effective_origin_acquire_policy, hls_pending_bootstrap_window_ms,
    hls_provider_connections_exhausted_manifest_resolution, hls_runtime_or_standalone_custom_tail_response,
    hls_unpublished_lease_channel_unavailable_response, join_hls_canonical_manifest_owner,
    latest_shared_hls_manifest_rendered_at_ms, mark_hls_authorized_manifest_access,
    maybe_mark_hls_provisioning_handoff_for_canonical_manifest, prepare_hls_origin_runtime,
    trigger_hls_canonical_manifest_refresh, try_hls_cached_manifest_response, HlsCacheManifestOrigin,
    HlsCanonicalOwnerHandoffContext, HlsCanonicalOwnerRegistration, HlsManifestRefreshOrdering,
    HlsOriginRuntimeAcquireError, HlsOriginRuntimeNoAccountReason, HlsOriginWorkKind, HlsProviderExhaustedResolution,
    HlsRuntimeBandwidthLearningContext, PreparedHlsOriginRuntime,
};
use crate::{
    api::model::{get_custom_stream_response_error_status, AppState, CustomVideoStreamType},
    auth::Fingerprint,
    model::ConfigInputFlags,
    utils::request,
};
use axum::{
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use log::{debug, warn};
use shared::utils::sanitize_sensitive_info;
use std::{sync::Arc, time::Duration};
use tuliprox_hls::api::{
    build_proxy_session_id, hls_cached_manifest_options_for_requirement, hls_manifest_acceptance_directive_for_session,
    register_hls_availability_reevaluation, safe_hls_access_lease_id, safe_proxy_session_id, safe_session_key,
    HlsAccessContext, HlsAccessLeaseId, HlsAccessLeasePendingDeadline, HlsAccessLeaseState, HlsAccessLeaseTouch,
    HlsAccountBindingProtection, HlsManifestAcceptanceDirective, HlsManifestAcceptanceEvaluationOutcome,
    HlsManifestCommitRequirement, HlsOriginIoContext, HlsOriginWorkClass, HlsRecoveryPoll, HlsRetainedPollOrigin,
    HlsRuntimeCustomTailReason, HlsSessionHandle, HlsSessionStoreOutcome, LiveHlsOriginEntry, ProxySessionId,
};

#[allow(clippy::too_many_arguments)]
pub(in crate::api::endpoints::hls_api) async fn prepare_hls_canonical_manifest_origin_runtime(
    app_state: &Arc<AppState>,
    session: &HlsSessionHandle,
    context: &HlsAccessContext,
    origin: &HlsCacheManifestOrigin<'_>,
    path_proxy_session_id: &ProxySessionId,
    access_lease_id: &HlsAccessLeaseId,
    access_lease_state: HlsAccessLeaseState,
    fingerprint: &Fingerprint,
    server_path: Option<&str>,
    now_ms: u64,
) -> Result<PreparedHlsOriginRuntime, Box<axum::response::Response>> {
    let expected_account = tuliprox_hls::api::HlsOriginAccountSnapshot::from_session(&*session.read().await);
    let mut allow_grace_hold = true;
    loop {
        match prepare_manifest_origin_work(app_state, session, origin, path_proxy_session_id, fingerprint, now_ms).await
        {
            Ok(prepared) => return Ok(prepared),
            Err(HlsOriginRuntimeAcquireError::NoAccountAvailable {
                reason: HlsOriginRuntimeNoAccountReason::OriginBindingPreempted,
            }) => {
                return Err(Box::new(
                    hls_runtime_or_standalone_custom_tail_response(
                        app_state,
                        session,
                        path_proxy_session_id,
                        access_lease_id,
                        HlsRuntimeCustomTailReason::LowPriorityPreempted,
                        StatusCode::SERVICE_UNAVAILABLE,
                    )
                    .await,
                ));
            }
            Err(HlsOriginRuntimeAcquireError::NoAccountAvailable {
                reason: HlsOriginRuntimeNoAccountReason::ProviderConnectionsExhausted,
            }) => {
                let strip = app_state.hls.proxy.strip();
                match hls_provider_connections_exhausted_manifest_resolution(
                    app_state,
                    Some(fingerprint),
                    session,
                    &context.username,
                    origin.input,
                    context.virtual_id,
                    access_lease_id,
                    access_lease_state,
                    &strip,
                    server_path,
                    allow_grace_hold,
                )
                .await
                {
                    HlsProviderExhaustedResolution::RetryAcquire => {
                        allow_grace_hold = false;
                    }
                    HlsProviderExhaustedResolution::Provisioning(response) => return Err(Box::new(response)),
                    HlsProviderExhaustedResolution::Exhausted(response) => {
                        if matches!(access_lease_state, HlsAccessLeaseState::Pending | HlsAccessLeaseState::Idle)
                            && session.read().await.origin_playback_kind()
                                == tuliprox_core::model::PlaybackKind::LiveHls
                            && app_state
                                .hls
                                .proxy
                                .access_lease_response_snapshot(
                                    access_lease_id,
                                    path_proxy_session_id,
                                    app_state.hls.proxy.now_ms(),
                                )
                                .await
                                .is_some()
                        {
                            app_state
                                .hls
                                .proxy
                                .fail_access_lease_start_admission(
                                    &app_state.hls_ctx(),
                                    session,
                                    access_lease_id,
                                    tuliprox_hls::api::HlsBoundAccountAcquireErrorKind::Exhausted,
                                    now_ms,
                                    &expected_account,
                                )
                                .await;
                            return Err(Box::new(
                                hls_unpublished_lease_channel_unavailable_response(
                                    app_state,
                                    path_proxy_session_id,
                                    access_lease_id,
                                )
                                .await,
                            ));
                        }
                        return Err(Box::new(response));
                    }
                }
            }
            Err(HlsOriginRuntimeAcquireError::Fatal(status)) => {
                return Err(Box::new(hls_canonical_status_response(status)))
            }
        }
    }
}

async fn prepare_manifest_origin_work(
    app_state: &Arc<AppState>,
    session: &HlsSessionHandle,
    origin: &HlsCacheManifestOrigin<'_>,
    proxy_session_id: &ProxySessionId,
    fingerprint: &Fingerprint,
    now_ms: u64,
) -> Result<PreparedHlsOriginRuntime, HlsOriginRuntimeAcquireError> {
    let policy = hls_effective_origin_acquire_policy(session).await;
    prepare_hls_origin_runtime(
        app_state,
        session,
        origin.input,
        origin.raw_request_url,
        origin.session_entry_url.as_str(),
        proxy_session_id,
        fingerprint,
        policy.connection_kind,
        policy.priority,
        HlsOriginWorkKind::Manifest,
        HlsOriginWorkClass::ManifestInteractive,
        now_ms,
    )
    .await
}

pub(in crate::api::endpoints::hls_api) async fn apply_hls_user_agent_stream_index(
    session: &HlsSessionHandle,
    headers: &mut HeaderMap,
    enabled: bool,
    active_users: &crate::api::model::ActiveUserManager,
) {
    if !enabled {
        return;
    }
    let stream_index = {
        let mut session = session.write().await;
        if session.user_agent_stream_index.is_none() {
            session.user_agent_stream_index = Some(active_users.next_user_agent_stream_index());
        }
        session.user_agent_stream_index
    };
    if let Some(stream_index) = stream_index {
        request::append_user_agent_stream_index(headers, stream_index);
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(in crate::api::endpoints::hls_api) async fn try_hls_cache_canonical_manifest_response(
    app_state: &Arc<AppState>,
    fingerprint: &Fingerprint,
    context: &HlsAccessContext,
    path_proxy_session_id: &ProxySessionId,
    access_lease_id: &HlsAccessLeaseId,
    access_lease_state: HlsAccessLeaseState,
    origin: HlsCacheManifestOrigin<'_>,
    mut headers: HeaderMap,
    server_path: Option<&str>,
    _original_hls_entry_path: &str,
    refresh_ordering: HlsManifestRefreshOrdering,
) -> Option<axum::response::Response> {
    if !hls_cache_configured(app_state) {
        return None;
    }
    let session_key = hls_cache_session_key_for_origin(app_state, context, &origin.origin_source).await;
    let expected_proxy_session_id = build_proxy_session_id(&session_key, &app_state.get_encrypt_secret());
    if &expected_proxy_session_id != path_proxy_session_id {
        return Some(StatusCode::NOT_FOUND.into_response());
    }

    // For group sessions the bound member is `HlsSession.origin_source`; the sealed access context
    // member may go stale after a handoff (R2), so validate against the live session state. Before
    // the session exists the sealed member is the only authority and must match the origin source.
    let live_group_session = if session_key.is_group_key() {
        app_state.hls.proxy.sessions().get_by_proxy_session_id(path_proxy_session_id).await
    } else {
        None
    };
    if let Some(session) = live_group_session {
        if session.read().await.origin_source != origin.origin_source {
            return Some(hls_canonical_retry_after_response());
        }
    } else if origin.origin_source.input_id != context.input_id || origin.origin_source.stream_ref != context.stream_ref
    {
        return Some(StatusCode::NOT_FOUND.into_response());
    }

    let now_ms = app_state.hls.proxy.now_ms();
    let rewrite_secret = app_state.get_encrypt_secret();
    let (session, session_outcome) = app_state
        .hls
        .proxy
        .get_or_create_session_with_source_and_outcome(
            session_key,
            origin.origin_source.clone(),
            &rewrite_secret,
            now_ms,
        )
        .await;
    if session.read().await.origin_playback_kind() == tuliprox_core::model::PlaybackKind::LiveHls {
        touch_initial_manifest_access_lease_window(
            app_state,
            access_lease_id,
            path_proxy_session_id,
            access_lease_state,
            hls_initial_manifest_decision_wait_timeout(app_state),
            now_ms,
        )
        .await;
    }
    if matches!(access_lease_state, HlsAccessLeaseState::Pending | HlsAccessLeaseState::Idle)
        && app_state
            .hls
            .proxy
            .access_lease_response_snapshot(access_lease_id, path_proxy_session_id, now_ms)
            .await
            .is_some_and(|lease| lease.manifest_start_attempt.as_ref().is_some_and(|attempt| attempt.failed()))
    {
        return Some(
            hls_unpublished_lease_channel_unavailable_response(app_state, path_proxy_session_id, access_lease_id).await,
        );
    }
    if access_lease_state == HlsAccessLeaseState::Activated {
        let timing = hls_access_lease_timing_for_session(app_state, &session).await;
        match app_state
            .hls
            .proxy
            .touch_manifest_access_lease(
                access_lease_id,
                path_proxy_session_id,
                now_ms,
                Some(timing),
                None,
                hls_access_lease_ttl_ms(app_state),
            )
            .await
        {
            HlsAccessLeaseTouch::Touched { lease } => {
                if session.read().await.has_retained_recovery_poll() {
                    #[cfg(test)]
                    super::cached::hls_manifest_publication_checkpoint(
                        super::cached::HlsManifestPublicationStage::RetainedPoll,
                    )
                    .await;
                    mark_hls_authorized_manifest_access(app_state, &session, now_ms).await;
                    // Select after the last await before returning the response.
                    match app_state.hls.proxy.retained_recovery_poll(&session, context, &lease).await {
                        HlsRecoveryPoll::Regular => {}
                        HlsRecoveryPoll::Unavailable => return Some(hls_canonical_retry_after_response()),
                        HlsRecoveryPoll::Terminal(lease) => {
                            return super::hls_terminal_playback_response(
                                app_state,
                                &lease,
                                path_proxy_session_id,
                                access_lease_id,
                            );
                        }
                        HlsRecoveryPoll::Retained { body, origin: refresh } => {
                            if let Some(refresh) = refresh {
                                spawn_retained_poll_refresh(app_state, &session, refresh, fingerprint, origin, headers);
                            }
                            return Some(super::hls_response(body.to_string()));
                        }
                    }
                }
            }
            HlsAccessLeaseTouch::Denied => {
                return Some(
                    hls_runtime_or_standalone_custom_tail_response(
                        app_state,
                        &session,
                        path_proxy_session_id,
                        access_lease_id,
                        HlsRuntimeCustomTailReason::UserConnectionsExhausted,
                        StatusCode::FORBIDDEN,
                    )
                    .await,
                );
            }
            HlsAccessLeaseTouch::Expired | HlsAccessLeaseTouch::UnknownLease | HlsAccessLeaseTouch::SessionMismatch => {
                return Some(StatusCode::NOT_FOUND.into_response());
            }
        }
    }
    app_state
        .hls
        .proxy
        .sync_session_access_lease_count_and_detach_if_needed(
            &app_state.active_users,
            &app_state.active_provider,
            &session,
            path_proxy_session_id,
            now_ms,
        )
        .await;
    let prepared_origin = match prepare_hls_canonical_manifest_origin_runtime(
        app_state,
        &session,
        context,
        &origin,
        path_proxy_session_id,
        access_lease_id,
        access_lease_state,
        fingerprint,
        server_path,
        now_ms,
    )
    .await
    {
        Ok(prepared) => prepared,
        Err(response) => return Some(*response),
    };
    apply_hls_user_agent_stream_index(
        &session,
        &mut headers,
        origin.input.has_flag(ConfigInputFlags::UserAgentStreamIndex),
        &app_state.active_users,
    )
    .await;
    let origin_entry = canonical_manifest_origin_entry(&prepared_origin, &origin)?;
    {
        let session_guard = session.read().await;
        if session_guard.is_gc_marked_for_removal() {
            return Some(hls_canonical_retry_after_response());
        }
    }
    mark_hls_authorized_manifest_access(app_state, &session, now_ms).await;
    let selected_account = session.read().await.origin_account_binding.as_ref().map_or_else(
        || "<none>".to_string(),
        |binding| sanitize_sensitive_info(binding.account_name.as_ref()).to_string(),
    );
    debug!(
        "HLS origin account selected: proxy_session={} account={}",
        safe_proxy_session_id(path_proxy_session_id),
        selected_account
    );
    let previous_manifest_rendered_at_ms = latest_shared_hls_manifest_rendered_at_ms(&session).await;
    let entry_origin_manifest_commits = session.read().await.origin_manifest_commits();
    let handoff_previous_rendered_at_ms = maybe_mark_hls_provisioning_handoff_for_canonical_manifest(
        app_state,
        &session,
        origin.input,
        context.virtual_id,
        access_lease_id,
        now_ms,
    )
    .await;
    let (manifest_commit_requirement, cached_entry) = app_state
        .hls
        .proxy
        .manifest_commit_requirement_with_cached_entry_for_lease(
            &session,
            session_outcome,
            handoff_previous_rendered_at_ms,
            matches!(access_lease_state, HlsAccessLeaseState::Pending | HlsAccessLeaseState::Idle),
            access_lease_id,
            now_ms,
        )
        .await;
    let manifest_boundary_rendered_at_ms = handoff_previous_rendered_at_ms.unwrap_or(previous_manifest_rendered_at_ms);
    let wait_timeout =
        hls_manifest_wait_timeout_for_requirement(app_state, &session, manifest_commit_requirement).await;
    let cached_manifest_options = hls_cached_manifest_options_for_requirement(
        wait_timeout,
        manifest_commit_requirement,
        manifest_boundary_rendered_at_ms,
        entry_origin_manifest_commits,
    )
    .with_cached_entry(cached_entry);
    let awaiting_media_progress = cached_manifest_options.awaiting_media_progress_entry();
    let bandwidth_learning = match context.known_bitrate_bps {
        Some(_) => HlsRuntimeBandwidthLearningContext::Disabled,
        None => HlsRuntimeBandwidthLearningContext::Eligible(origin.input),
    };
    if cached_manifest_options.cached_entry_requested() {
        // The regular candidate still has priority. A committed cache window
        // can be published before scheduling or joining another origin request.
        let mut immediate = cached_manifest_options;
        immediate.wait_timeout = Duration::ZERO;
        if awaiting_media_progress {
            immediate = immediate
                .requiring_newer_manifest(previous_manifest_rendered_at_ms)
                .requiring_origin_commit_after(entry_origin_manifest_commits);
        }
        if let Some(response) = try_hls_cached_manifest_response(
            app_state,
            Some(fingerprint),
            &session,
            access_lease_id,
            access_lease_state,
            &app_state.hls.proxy.strip(),
            server_path,
            immediate,
            bandwidth_learning,
        )
        .await
        {
            let use_response = if awaiting_media_progress {
                response.status() == StatusCode::OK
            } else {
                response.extensions().get::<HlsProvisionalRecoveryEntryResponse>().is_none()
                    || !session.read().await.origin_refresh.is_due(now_ms)
            };
            if use_response {
                return Some(finish_hls_recovery_entry_response(response));
            }
            // An unplayable fallback must not prevent the normal refresh path from spending a due retry.
        }
    }
    let hls_ctx = app_state.hls_ctx();
    let acceptance_evaluation =
        hls_manifest_acceptance_directive_for_session(&hls_ctx, &session, path_proxy_session_id).await;
    let (acceptance_directive, availability_reevaluation_owner_key) = match acceptance_evaluation {
        HlsManifestAcceptanceEvaluationOutcome::Evaluated(directive) => (directive, None),
        HlsManifestAcceptanceEvaluationOutcome::StateContention { owner_key } => {
            (HlsManifestAcceptanceDirective::none(), Some(owner_key))
        }
        HlsManifestAcceptanceEvaluationOutcome::SessionSuperseded => {
            drop(prepared_origin.preacquired_origin_account_handle);
            return Some(hls_canonical_retry_after_response());
        }
    };

    let mut preacquired_provider_handle = prepared_origin.preacquired_origin_account_handle;
    let mut origin_io = canonical_manifest_origin_io(app_state, &session, fingerprint).await;
    if availability_reevaluation_owner_key.is_none() {
        if let Some(provider_handle) = preacquired_provider_handle.take() {
            origin_io = origin_io.with_managed_provider_handle(provider_handle);
        }
    }

    let refresh_request = hls_origin_refresh_request(
        app_state,
        &session,
        tuliprox_hls::api::HlsPreparedOriginAccount { entry: origin_entry, snapshot: prepared_origin.snapshot },
        HlsOriginRefreshOptions {
            headers,
            origin_io,
            manifest_commit_requirement,
            acceptance_directive,
            access_lease_id: Some(access_lease_id.clone()),
            rewrite_secret: rewrite_secret.to_vec(),
            now_ms,
        },
    );
    let refresh_ordering = if session_outcome == HlsSessionStoreOutcome::Reused {
        refresh_ordering
    } else {
        HlsManifestRefreshOrdering::Background
    };
    if let Some(owner_key) = availability_reevaluation_owner_key {
        drop(preacquired_provider_handle);
        touch_initial_manifest_access_lease_window(
            app_state,
            access_lease_id,
            path_proxy_session_id,
            access_lease_state,
            wait_timeout,
            now_ms,
        )
        .await;
        let owner_wait_lease = app_state
            .hls
            .proxy
            .access_lease_response_snapshot(access_lease_id, path_proxy_session_id, app_state.hls.proxy.now_ms())
            .await;
        let expected_lease_issued_at_ms = owner_wait_lease.as_ref().map(|lease| lease.issued_at_ms);
        let request_deadline_ms = owner_wait_lease
            .as_ref()
            .map_or(now_ms, |lease| hls_canonical_owner_request_deadline_ms(lease, wait_timeout, now_ms));
        let safe_session = {
            let session = session.read().await;
            safe_session_key(&session.key)
        };
        let registration =
            register_hls_availability_reevaluation(hls_ctx, Arc::clone(&session), owner_key, refresh_request);
        let response = match hls_canonical_owner_registration(registration) {
            HlsCanonicalOwnerRegistration::Join(registration) => {
                let strip = app_state.hls.proxy.strip();
                join_hls_canonical_manifest_owner(
                    HlsCanonicalOwnerHandoffContext {
                        app_state,
                        fingerprint: Some(fingerprint),
                        proxy_session_id: path_proxy_session_id,
                        access_lease_id,
                        expected_lease_issued_at_ms,
                        strip: &strip,
                        server_path,
                        manifest_commit_requirement,
                        manifest_boundary_rendered_at_ms,
                        entry_origin_manifest_commits,
                        bandwidth_learning,
                        request_deadline_ms,
                        safe_session,
                    },
                    registration,
                )
                .await
            }
            HlsCanonicalOwnerRegistration::FailClosed(failure) => {
                hls_availability_reevaluation_registration_failure_response(failure)
            }
        };
        return Some(
            finish_shared_entry_response(
                app_state,
                path_proxy_session_id,
                access_lease_id,
                awaiting_media_progress,
                Some(response),
            )
            .await,
        );
    }
    if handoff_previous_rendered_at_ms.is_some() {
        touch_initial_manifest_access_lease_window(
            app_state,
            access_lease_id,
            path_proxy_session_id,
            access_lease_state,
            wait_timeout,
            now_ms,
        )
        .await;
        if let Some(response) = trigger_hls_canonical_manifest_refresh(
            app_state,
            &session,
            path_proxy_session_id,
            access_lease_id,
            access_lease_state,
            refresh_request,
            refresh_ordering,
        )
        .await
        {
            return Some(response);
        }
        let strip = app_state.hls.proxy.strip();
        if let Some(response) = try_hls_cached_manifest_response(
            app_state,
            Some(fingerprint),
            &session,
            access_lease_id,
            access_lease_state,
            &strip,
            server_path,
            cached_manifest_options,
            bandwidth_learning,
        )
        .await
        {
            clear_hls_provisioning_handoff_consumer(
                app_state,
                origin.input,
                context.virtual_id,
                app_state.hls.proxy.now_ms(),
            );
            return Some(finish_hls_recovery_entry_response(response));
        }
        return Some(StatusCode::SERVICE_UNAVAILABLE.into_response());
    }
    match session_outcome {
        HlsSessionStoreOutcome::Created => {
            touch_initial_manifest_access_lease_window(
                app_state,
                access_lease_id,
                path_proxy_session_id,
                access_lease_state,
                wait_timeout,
                now_ms,
            )
            .await;
            if let Some(response) = trigger_hls_canonical_manifest_refresh(
                app_state,
                &session,
                path_proxy_session_id,
                access_lease_id,
                access_lease_state,
                refresh_request,
                refresh_ordering,
            )
            .await
            {
                return Some(response);
            }
            let strip = app_state.hls.proxy.strip();
            if let Some(response) = try_hls_cached_manifest_response(
                app_state,
                Some(fingerprint),
                &session,
                access_lease_id,
                access_lease_state,
                &strip,
                server_path,
                cached_manifest_options,
                bandwidth_learning,
            )
            .await
            {
                return Some(finish_hls_recovery_entry_response(response));
            }
        }
        HlsSessionStoreOutcome::Reused => {
            if let Some(response) = trigger_hls_canonical_manifest_refresh(
                app_state,
                &session,
                path_proxy_session_id,
                access_lease_id,
                access_lease_state,
                refresh_request,
                refresh_ordering,
            )
            .await
            {
                if refresh_ordering == HlsManifestRefreshOrdering::AwaitBeforeTerminalEvaluation
                    && response.status() == StatusCode::SERVICE_UNAVAILABLE
                {
                    let strip = app_state.hls.proxy.strip();
                    if let Some(live_response) = try_hls_cached_manifest_response(
                        app_state,
                        Some(fingerprint),
                        &session,
                        access_lease_id,
                        access_lease_state,
                        &strip,
                        server_path,
                        hls_cached_manifest_options_for_requirement(
                            Duration::ZERO,
                            manifest_commit_requirement,
                            manifest_boundary_rendered_at_ms,
                            entry_origin_manifest_commits,
                        ),
                        bandwidth_learning,
                    )
                    .await
                    .filter(|candidate| candidate.status() == StatusCode::OK)
                    {
                        return Some(live_response);
                    }
                }
                return Some(
                    finish_shared_entry_response(
                        app_state,
                        path_proxy_session_id,
                        access_lease_id,
                        awaiting_media_progress,
                        Some(response),
                    )
                    .await,
                );
            }
            touch_initial_manifest_access_lease_window(
                app_state,
                access_lease_id,
                path_proxy_session_id,
                access_lease_state,
                wait_timeout,
                now_ms,
            )
            .await;
            let strip = app_state.hls.proxy.strip();
            if let Some(response) = try_hls_cached_manifest_response(
                app_state,
                Some(fingerprint),
                &session,
                access_lease_id,
                access_lease_state,
                &strip,
                server_path,
                cached_manifest_options,
                bandwidth_learning,
            )
            .await
            {
                return Some(
                    finish_shared_entry_response(
                        app_state,
                        path_proxy_session_id,
                        access_lease_id,
                        awaiting_media_progress,
                        Some(response),
                    )
                    .await,
                );
            }
        }
    }

    Some(
        finish_shared_entry_response(app_state, path_proxy_session_id, access_lease_id, awaiting_media_progress, None)
            .await,
    )
}

async fn finish_shared_entry_response(
    app_state: &Arc<AppState>,
    proxy_session_id: &ProxySessionId,
    lease_id: &HlsAccessLeaseId,
    awaiting_media_progress: bool,
    response: Option<axum::response::Response>,
) -> axum::response::Response {
    if awaiting_media_progress
        && response
            .as_ref()
            .is_none_or(|response| matches!(response.status(), StatusCode::NOT_FOUND | StatusCode::SERVICE_UNAVAILABLE))
    {
        let now_ms = app_state.hls.proxy.now_ms();
        if let Some(lease) =
            app_state.hls.proxy.access_lease_response_snapshot(lease_id, proxy_session_id, now_ms).await
        {
            if lease.permits_unpublished_standalone_tail(HlsRuntimeCustomTailReason::ChannelUnavailable) {
                let configured = get_custom_stream_response_error_status(&app_state.app_config);
                let status = if configured == StatusCode::NOT_FOUND { StatusCode::BAD_GATEWAY } else { configured };
                let response = super::hls_custom_video_manifest_response_for_lease(
                    app_state,
                    &lease,
                    CustomVideoStreamType::ChannelUnavailable,
                    status,
                )
                .await;
                return if response.status().is_success() { response } else { status.into_response() };
            }
        }
    }
    match response {
        Some(response) => finish_hls_recovery_entry_response(response),
        None => hls_unpublished_lease_channel_unavailable_response(app_state, proxy_session_id, lease_id).await,
    }
}

fn spawn_retained_poll_refresh(
    app_state: &Arc<AppState>,
    session: &HlsSessionHandle,
    refresh_origin: HlsRetainedPollOrigin,
    fingerprint: &Fingerprint,
    origin: HlsCacheManifestOrigin<'_>,
    headers: HeaderMap,
) {
    let app = Arc::clone(app_state);
    let session = Arc::clone(session);
    let fingerprint = fingerprint.clone();
    let input = origin.input.clone();
    let raw_request_url = origin.raw_request_url.to_owned();
    #[cfg(test)]
    let barrier = super::cached::take_retained_origin_barrier();
    tokio::spawn(async move {
        let refresh = refresh_retained_poll(
            &app,
            &session,
            &refresh_origin,
            &fingerprint,
            HlsCacheManifestOrigin {
                raw_request_url: &raw_request_url,
                session_entry_url: origin.session_entry_url,
                input: &input,
                origin_source: origin.origin_source,
            },
            headers,
        );
        #[cfg(test)]
        if let Some(barrier) = barrier {
            let scoped = Arc::clone(&barrier);
            super::cached::HLS_MANIFEST_PUBLICATION_BARRIER.scope(std::cell::RefCell::new(Some(scoped)), refresh).await;
            barrier.finished.notify_one();
            return;
        }
        refresh.await;
    });
}

#[allow(clippy::too_many_lines, reason = "Keep prepared-account ownership through refresh registration.")]
async fn refresh_retained_poll(
    app_state: &Arc<AppState>,
    session: &HlsSessionHandle,
    refresh: &HlsRetainedPollOrigin,
    fingerprint: &Fingerprint,
    origin: HlsCacheManifestOrigin<'_>,
    mut headers: HeaderMap,
) {
    let proxy_id = refresh.proxy_id();
    let lease_id = refresh.lease_id();
    let retained = RetainedOriginPreparation { session, origin: refresh };
    let prepared = match prepare_hls_origin_runtime_for(
        app_state,
        session,
        origin.input,
        origin.raw_request_url,
        origin.session_entry_url.as_str(),
        proxy_id,
        fingerprint,
        crate::api::model::ConnectionKind::Normal,
        0,
        HlsOriginWorkKind::Manifest,
        HlsOriginWorkClass::Background,
        app_state.hls.proxy.now_ms(),
        Some(retained),
    )
    .await
    {
        Ok(prepared) => prepared,
        Err(reason) => {
            debug!(
                "HLS retained poll origin preparation deferred: proxy_session={} reason={reason:?}",
                safe_proxy_session_id(proxy_id)
            );
            #[cfg(test)]
            if matches!(
                reason,
                HlsOriginRuntimeAcquireError::NoAccountAvailable {
                    reason: HlsOriginRuntimeNoAccountReason::ProviderConnectionsExhausted
                }
            ) {
                super::cached::hls_manifest_publication_checkpoint(
                    super::cached::HlsManifestPublicationStage::RetainedOriginExhausted,
                )
                .await;
            }
            return;
        }
    };
    apply_hls_user_agent_stream_index(
        session,
        &mut headers,
        origin.input.has_flag(ConfigInputFlags::UserAgentStreamIndex),
        &app_state.active_users,
    )
    .await;
    #[cfg(test)]
    super::cached::hls_manifest_publication_checkpoint(
        super::cached::HlsManifestPublicationStage::RetainedOriginBeforeRefresh,
    )
    .await;
    let now_ms = app_state.hls.proxy.now_ms();
    let Some(origin_entry) = canonical_manifest_origin_entry(&prepared, &origin) else {
        discard_retained_poll_preparation(app_state, session, refresh, prepared).await;
        return;
    };
    let requirement = app_state
        .hls
        .proxy
        .manifest_commit_requirement_for_lease(session, HlsSessionStoreOutcome::Reused, None, false, lease_id, now_ms)
        .await;
    let hls_ctx = app_state.hls_ctx();
    let (directive, owner_key) = match hls_manifest_acceptance_directive_for_session(&hls_ctx, session, proxy_id).await
    {
        HlsManifestAcceptanceEvaluationOutcome::Evaluated(directive) => (directive, None),
        HlsManifestAcceptanceEvaluationOutcome::StateContention { owner_key } => {
            (HlsManifestAcceptanceDirective::none(), Some(owner_key))
        }
        HlsManifestAcceptanceEvaluationOutcome::SessionSuperseded => {
            discard_retained_poll_preparation(app_state, session, refresh, prepared).await;
            return;
        }
    };
    let io = canonical_manifest_origin_io(app_state, session, fingerprint).await.with_grace(false);
    let preacquired = prepared.preacquired_origin_account_handle;
    let mut request = hls_origin_refresh_request(
        app_state,
        session,
        tuliprox_hls::api::HlsPreparedOriginAccount { entry: origin_entry, snapshot: prepared.snapshot },
        HlsOriginRefreshOptions {
            headers,
            origin_io: io,
            manifest_commit_requirement: requirement,
            acceptance_directive: directive,
            access_lease_id: Some(lease_id.clone()),
            rewrite_secret: app_state.get_encrypt_secret().to_vec(),
            now_ms,
        },
    );
    request.retained_poll_origin = Some(refresh.clone());
    if let Some(owner_key) = owner_key {
        let can_schedule = retained.with_current(app_state, |_, _| Ok(())).await.is_ok();
        if !can_schedule {
            release_retained_origin_allocation(
                app_state,
                &tuliprox_hls::api::build_hls_origin_session_owner(proxy_id),
                preacquired,
            );
            app_state.hls.proxy.release_unused_retained_poll_origin(session, refresh).await;
            return;
        }
        // A reevaluation can sleep or hand off. Later work acquires against
        // the current binding without holding the provisional connection.
        drop(preacquired);
        if let HlsCanonicalOwnerRegistration::FailClosed(reason) = hls_canonical_owner_registration(
            register_hls_availability_reevaluation(hls_ctx, Arc::clone(session), owner_key, request),
        ) {
            warn!("HLS retained poll availability reevaluation not registered: reason={}", reason.as_label());
        }
    } else {
        if let Some(handle) = preacquired {
            request.origin_io = request.origin_io.take().map(|io| io.with_managed_provider_handle(handle));
        }
        super::canonical_owner::trigger_hls_background_manifest_refresh(app_state, session, proxy_id, request).await;
    }
}

async fn discard_retained_poll_preparation(
    app_state: &Arc<AppState>,
    session: &HlsSessionHandle,
    refresh: &HlsRetainedPollOrigin,
    prepared: PreparedHlsOriginRuntime,
) {
    release_retained_origin_allocation(
        app_state,
        &tuliprox_hls::api::build_hls_origin_session_owner(refresh.proxy_id()),
        prepared.preacquired_origin_account_handle,
    );
    app_state.hls.proxy.release_unused_retained_poll_origin(session, refresh).await;
}

fn canonical_manifest_origin_entry(
    prepared: &PreparedHlsOriginRuntime,
    origin: &HlsCacheManifestOrigin<'_>,
) -> Option<LiveHlsOriginEntry> {
    let url_failover_provider = effective_hls_url_failover_provider_for_fetch_url(
        &prepared.fetch_url,
        prepared.url_failover_provider.clone(),
        origin.session_entry_url.url_failover_provider(),
    );
    let entry = LiveHlsOriginEntry::parse_with_provider_configs(
        &prepared.fetch_url,
        url_failover_provider,
        prepared.runtime_provider_config.clone(),
    )?;
    let profile_recovery_failover = origin
        .input
        .playback_provider
        .as_deref()
        .is_some_and(|name| entry.url_failover_provider().is_some_and(|profile| profile.name.as_ref() == name));
    Some(entry.with_profile_recovery_failover(profile_recovery_failover))
}

async fn canonical_manifest_origin_io(
    app_state: &Arc<AppState>,
    session: &HlsSessionHandle,
    fingerprint: &Fingerprint,
) -> HlsOriginIoContext {
    let session = session.read().await;
    let policy = session.effective_origin_acquire_policy_or_default();
    HlsOriginIoContext {
        ctx: app_state.hls_ctx(),
        client_addr: fingerprint.addr,
        allow_grace: HlsOriginWorkClass::ManifestInteractive.allows_grace(),
        priority: policy.priority,
        connection_kind: policy.connection_kind,
        reservation_ttl_secs: session.account_overlap_timing().reservation_ttl_secs(),
        preacquired_provider_handle: None,
        started_generation: None,
        prepared_account: None,
        playback_kind: session.origin_playback_kind(),
        account_runtime: session.origin_account_runtime.clone(),
    }
}

pub(in crate::api::endpoints::hls_api) fn hls_initial_manifest_decision_wait_timeout(
    app_state: &Arc<AppState>,
) -> Duration {
    Duration::from_secs(app_state.hls.proxy.initial_manifest_wait_timeout_secs())
}

pub(in crate::api::endpoints::hls_api) async fn hls_manifest_wait_timeout_for_requirement(
    app_state: &Arc<AppState>,
    session: &HlsSessionHandle,
    requirement: HlsManifestCommitRequirement,
) -> Duration {
    match requirement {
        HlsManifestCommitRequirement::FreshCommitRequired { .. } => {
            hls_initial_manifest_decision_wait_timeout(app_state)
        }
        HlsManifestCommitRequirement::CommittedManifestAllowed => {
            hls_initial_manifest_wait_timeout(app_state, session).await
        }
    }
}

pub(in crate::api::endpoints::hls_api) async fn touch_initial_manifest_access_lease_window(
    app_state: &Arc<AppState>,
    access_lease_id: &HlsAccessLeaseId,
    proxy_session_id: &ProxySessionId,
    access_lease_state: HlsAccessLeaseState,
    wait_timeout: Duration,
    now_ms: u64,
) {
    if wait_timeout.is_zero() || access_lease_state != HlsAccessLeaseState::Pending {
        return;
    }
    let wait_timeout_ms = duration_to_millis_saturating(wait_timeout);
    let deadline_ms = now_ms.saturating_add(wait_timeout_ms.max(hls_pending_bootstrap_window_ms(app_state)));
    let live = if let Some(session) = app_state.hls.proxy.sessions().get_by_proxy_session_id(proxy_session_id).await {
        session.read().await.origin_playback_kind() == tuliprox_core::model::PlaybackKind::LiveHls
    } else {
        false
    };
    let pending_deadline = if live {
        HlsAccessLeasePendingDeadline::Processing { deadline_ms }
    } else {
        HlsAccessLeasePendingDeadline::Bootstrap { deadline_ms }
    };
    let touch = app_state
        .hls
        .proxy
        .touch_manifest_access_lease(
            access_lease_id,
            proxy_session_id,
            now_ms,
            None,
            Some(pending_deadline),
            hls_access_lease_ttl_ms(app_state),
        )
        .await;
    let failure = match touch {
        HlsAccessLeaseTouch::Touched { .. } => return,
        HlsAccessLeaseTouch::Expired => "expired",
        HlsAccessLeaseTouch::Denied => "denied",
        HlsAccessLeaseTouch::UnknownLease => "unknown-lease",
        HlsAccessLeaseTouch::SessionMismatch => "session-mismatch",
    };
    debug!(
        "HLS initial manifest lease window not extended: lease={} proxy_session={} outcome={failure}",
        safe_hls_access_lease_id(access_lease_id),
        safe_proxy_session_id(proxy_session_id)
    );
}

pub(in crate::api::endpoints::hls_api) async fn hls_initial_manifest_wait_timeout(
    app_state: &Arc<AppState>,
    session: &HlsSessionHandle,
) -> Duration {
    let session = session.read().await;
    if matches!(
        session.account_binding_protection(app_state.hls.proxy.now_ms()),
        HlsAccountBindingProtection::NoMediaYet | HlsAccountBindingProtection::Expired
    ) {
        hls_initial_manifest_decision_wait_timeout(app_state)
    } else {
        Duration::ZERO
    }
}
