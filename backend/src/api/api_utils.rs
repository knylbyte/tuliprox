use crate::api::endpoints::xtream_api::{get_xtream_player_api_stream_url, ApiStreamContext};
use crate::api::model::{tee_stream, UserSession};
use crate::api::model::{
    create_channel_unavailable_stream, create_custom_video_stream_response,
    create_provider_connections_exhausted_stream, create_provider_stream,
    get_stream_response_with_headers, ActiveClientStream, AppState,
    CustomVideoStreamType,
    ProviderStreamFactoryOptions, SharedStreamManager,
    StreamError, ThrottledStream, UserApiRequest,
};
use crate::api::model::{ProviderAllocation, ProviderConfig, ProviderStreamState, StreamDetails, StreamingStrategy};
use crate::api::panel_api::try_provision_account_on_exhausted;
use crate::model::{ConfigInput, ResourceRetryConfig};
use crate::model::{ConfigTarget, ProxyUserCredentials};
use crate::tools::lru_cache::LRUResourceCache;
use crate::utils::{async_file_reader, async_file_writer, create_new_file_for_write};
use crate::utils::request;
use crate::utils::{debug_if_enabled, trace_if_enabled};
use crate::BUILD_TIMESTAMP;
use arc_swap::ArcSwapOption;
use axum::http::{HeaderMap};
use axum::response::IntoResponse;
use chrono::{DateTime, Utc};
use futures::{StreamExt, TryStreamExt};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use log::{debug, error,  log_enabled, trace, warn};
use reqwest::header::RETRY_AFTER;
use serde::Serialize;
use shared::model::{Claims, InputFetchMethod, PlaylistEntry, PlaylistItemType, StreamChannel, TargetType, UserConnectionPermission, XtreamCluster};
use shared::utils::{bin_serialize, default_grace_period_millis, trim_slash};
use shared::utils::{
    extract_extension_from_url, replace_url_extension, sanitize_sensitive_info, DASH_EXT, HLS_EXT,
};
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use url::Url;

const CONTENT_TYPE_BIN: &str = "application/cbor";

#[macro_export]
macro_rules! try_option_bad_request {
    ($option:expr, $msg_is_error:expr, $msg:expr) => {
        match $option {
            Some(value) => value,
            None => {
                if $msg_is_error {
                    error!("{}", $msg);
                } else {
                    debug!("{}", $msg);
                }
                return axum::http::StatusCode::BAD_REQUEST.into_response();
            }
        }
    };
    ($option:expr) => {
        match $option {
            Some(value) => value,
            None => return axum::http::StatusCode::BAD_REQUEST.into_response(),
        }
    };
}

#[macro_export]
macro_rules! try_unwrap_body {
    ($body:expr) => {
        $body.map_or_else(
            |_| axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            |resp| resp.into_response(),
        )
    };
}

#[macro_export]
macro_rules! try_result_or_status {
    ($option:expr, $status:expr, $msg_is_error:expr, $msg:expr) => {
        match $option {
            Ok(value) => value,
            Err(_) => {
                if $msg_is_error {
                    error!("{}", $msg);
                } else {
                    debug!("{}", $msg);
                }
                return $status.into_response();
            }
        }
    };
    ($option:expr, $status:expr) => {
        match $option {
            Ok(value) => value,
            Err(_) => return $status.into_response(),
        }
    };
}

#[macro_export]
macro_rules! try_result_bad_request {
    ($option:expr, $msg_is_error:expr, $msg:expr) => {
       $crate::api::api_utils::try_result_or_status!($option, axum::http::StatusCode::BAD_REQUEST, $msg_is_error, $msg)
    };
    ($option:expr) => {
       $crate::api::api_utils::try_result_or_status!($option, axum::http::StatusCode::BAD_REQUEST)
    };
}

#[macro_export]
macro_rules! try_result_not_found {
    ($option:expr, $msg_is_error:expr, $msg:expr) => {
       $crate::api::api_utils::try_result_or_status!($option, axum::http::StatusCode::NOT_FOUND, $msg_is_error, $msg)
    };
    ($option:expr) => {
       $crate::api::api_utils::try_result_or_status!($option, axum::http::StatusCode::NOT_FOUND)
    };
}

pub use try_result_or_status;
pub use try_option_bad_request;
pub use try_result_bad_request;
pub use try_result_not_found;
pub use try_unwrap_body;
use crate::auth::Fingerprint;

pub fn get_server_time() -> String {
    chrono::offset::Local::now()
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H:%M:%S %Z")
        .to_string()
}

pub fn get_build_time() -> Option<String> {
    BUILD_TIMESTAMP
        .to_string()
        .parse::<DateTime<Utc>>()
        .ok()
        .map(|datetime| datetime.format("%Y-%m-%d %H:%M:%S %Z").to_string())
}

#[allow(clippy::missing_panics_doc)]
pub async fn serve_file(file_path: &Path, mime_type: mime::Mime) -> impl IntoResponse + Send {
    match tokio::fs::try_exists(file_path).await {
        Ok(exists) => {
            if ! exists {
                return axum::http::StatusCode::NOT_FOUND.into_response();
            }
        }
        Err(err) => {
            error!("Failed to open egp file {}, {err:?}", file_path.display());
            return axum::http::StatusCode::NOT_FOUND.into_response();
        }
    }

    match tokio::fs::File::open(file_path).await {
        Ok(file) => {
            let reader = async_file_reader(file);
            let stream = tokio_util::io::ReaderStream::new(reader);
            let body = axum::body::Body::from_stream(stream);

            try_unwrap_body!(axum::response::Response::builder()
                .status(axum::http::StatusCode::OK)
                .header(axum::http::header::CONTENT_TYPE, mime_type.to_string())
                .header(
                    axum::http::header::CACHE_CONTROL,
                    axum::http::header::HeaderValue::from_static("no-cache")
                )
                .body(body))
        }
        Err(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub fn get_user_target_by_username(
    username: &str,
    app_state: &AppState,
) -> Option<(ProxyUserCredentials, Arc<ConfigTarget>)> {
    if !username.is_empty() {
        return app_state.app_config.get_target_for_username(username);
    }
    None
}

pub fn get_user_target_by_credentials<'a>(
    username: &str,
    password: &str,
    api_req: &'a UserApiRequest,
    app_state: &'a AppState,
) -> Option<(ProxyUserCredentials, Arc<ConfigTarget>)> {
    if !username.is_empty() && !password.is_empty() {
        app_state.app_config.get_target_for_user(username, password)
    } else {
        let token = api_req.token.as_str().trim();
        if token.is_empty() {
            None
        } else {
            app_state.app_config.get_target_for_user_by_token(token)
        }
    }
}

pub fn get_user_target<'a>(
    api_req: &'a UserApiRequest,
    app_state: &'a AppState,
) -> Option<(ProxyUserCredentials, Arc<ConfigTarget>)> {
    let username = api_req.username.as_str().trim();
    let password = api_req.password.as_str().trim();
    get_user_target_by_credentials(username, password, api_req, app_state)
}

pub struct StreamOptions {
    pub stream_retry: bool,
    pub buffer_enabled: bool,
    pub buffer_size: usize,
    pub pipe_provider_stream: bool,
}

/// Constructs a `StreamOptions` object based on the application's reverse proxy configuration.
///
/// This function retrieves streaming-related settings from the `AppState`:
/// - `stream_retry`: whether retrying the stream is enabled,
/// - `buffer_enabled`: whether stream buffering is enabled,
/// - `buffer_size`: the size of the stream buffer.
///
/// If the reverse proxy or stream settings are not defined, default values are used:
/// - retry: `false`
/// - buffering: `false`
/// - buffer size: `0`
///
/// Additionally, it computes `pipe_provider_stream`, which is `true` only if
/// both retry and buffering are disabled—indicating that the stream can be piped directly
/// from the provider without additional handling.
///
/// Returns a `StreamOptions` instance with the resolved configuration.
fn get_stream_options(app_state: &AppState) -> StreamOptions {
    let (stream_retry, buffer_enabled, buffer_size) = app_state
        .app_config
        .config
        .load()
        .reverse_proxy
        .as_ref()
        .and_then(|reverse_proxy| reverse_proxy.stream.as_ref())
        .map_or((false, false, 0), |stream| {
            let (buffer_enabled, buffer_size) = stream
                .buffer
                .as_ref()
                .map_or((false, 0), |buffer| (buffer.enabled, buffer.size));
            (
                stream.retry,
                buffer_enabled,
                buffer_size,
            )
        });
    let pipe_provider_stream = !stream_retry && !buffer_enabled;
    StreamOptions {
        stream_retry,
        buffer_enabled,
        buffer_size,
        pipe_provider_stream,
    }
}

// fn get_stream_content_length(provider_response: Option<&(Vec<(String, String)>, StatusCode)>) -> u64 {
//     let content_length = provider_response
//         .as_ref()
//         .and_then(|(headers, _)| headers.iter().find(|(h, _)| h.eq(axum::http::header::CONTENT_LENGTH.as_str())))
//         .and_then(|(_, val)| val.parse::<u64>().ok())
//         .unwrap_or(0);
//     content_length
// }

pub fn get_stream_alternative_url(
    stream_url: &str,
    input: &ConfigInput,
    alias_input: &Arc<ProviderConfig>,
) -> String {
    let Some(input_user_info) = input.get_user_info() else {
        return stream_url.to_owned();
    };
    let Some(alt_input_user_info) = alias_input.get_user_info() else {
        return stream_url.to_owned();
    };

    let modified = stream_url.replacen(&input_user_info.base_url, &alt_input_user_info.base_url, 1);
    let modified = modified.replacen(&input_user_info.username, &alt_input_user_info.username, 1);
    modified.replacen(&input_user_info.password, &alt_input_user_info.password, 1)
}

async fn get_redirect_alternative_url<'a>(
    app_state: &AppState,
    redirect_url: &'a str,
    input: &ConfigInput,
) -> Cow<'a, str> {
    if let Some((base_url, username, password)) = input.get_matched_config_by_url(redirect_url) {
        if let Some(provider_cfg) = app_state
            .active_provider
            .get_next_provider(&input.name)
            .await
        {
            let mut new_url = redirect_url.replacen(base_url, provider_cfg.url.as_str(), 1);
            if let (Some(old_username), Some(old_password)) = (username, password) {
                if let (Some(new_username), Some(new_password)) = (
                    provider_cfg.username.as_ref(),
                    provider_cfg.password.as_ref(),
                ) {
                    new_url = new_url.replacen(old_username, new_username, 1);
                    new_url = new_url.replacen(old_password, new_password, 1);
                    return Cow::Owned(new_url);
                }
                // one has credentials the other not, something not right
                return Cow::Borrowed(redirect_url);
            }
            return Cow::Owned(new_url);
        }
    }
    Cow::Borrowed(redirect_url)
}


/// Determines the appropriate streaming strategy for the given input and stream URL.
///
/// This function attempts to acquire a connection to a streaming provider, either using a forced provider
/// (if specified), or based on the input name. It then selects a corresponding `StreamingOption`:
///
/// - If no connections are available (`Exhausted`), it returns a custom stream indicating exhaustion.
/// - If a connection is available or in a grace period, it constructs a streaming URL accordingly:
///   - If the provider was forced or matches the input, the original URL is reused.
///   - Otherwise, an alternative URL is generated based on the provider and input.
///
/// The function returns:
/// - an optional `ProviderConnectionGuard` to manage the connection's lifecycle,
/// - a `ProviderStreamState` describing how the stream state is,
/// - and optional HTTP headers to include in the request.
///
/// This logic helps abstract the decision-making behind provider selection and stream URL resolution.
async fn resolve_streaming_strategy(
    app_state: &AppState,
    stream_url: &str,
    fingerprint: &Fingerprint,
    input: &ConfigInput,
    force_provider: Option<&str>,
) -> StreamingStrategy {
    // allocate a provider connection
    let mut provider_connection_handle = if let Some(provider) = force_provider {
        app_state
            .active_provider
            .force_exact_acquire_connection(provider, &fingerprint.addr)
            .await
    } else {
        // For panel-managed pools we must not allocate providers in grace period, otherwise
        // the pool may never reach `Exhausted` and panel provisioning won't be triggered.
        let allow_provider_grace = input.panel_api.is_none();
        if !allow_provider_grace {
            debug_if_enabled!(
                "panel_api: disabling provider grace allocations for input {}",
                sanitize_sensitive_info(&input.name)
            );
        }
        app_state
            .active_provider
            .acquire_connection_with_grace_override(&input.name, &fingerprint.addr, allow_provider_grace)
            .await
    };

    if provider_connection_handle.is_none()
        && force_provider.is_none()
        && try_provision_account_on_exhausted(app_state, input).await
    {
        debug_if_enabled!(
            "panel_api: provider pool exhausted for input {}, provision succeeded; re-acquiring connection",
            sanitize_sensitive_info(&input.name)
        );
        provider_connection_handle = app_state
            .active_provider
            .acquire_connection_with_grace_override(&input.name, &fingerprint.addr, input.panel_api.is_none())
            .await;
    } else if provider_connection_handle.is_none() && force_provider.is_none() && input.panel_api.is_some() {
        debug_if_enabled!(
            "panel_api: provider pool exhausted for input {}, provision skipped/failed",
            sanitize_sensitive_info(&input.name)
        );
    }

    let stream_response_params =
        if let Some(allocation) = provider_connection_handle.as_ref().map(|ph| &ph.allocation) {
            match allocation {
                ProviderAllocation::Exhausted => {
                    debug!("Provider {} is exhausted. No connections allowed.", input.name);
                    let stream = create_provider_connections_exhausted_stream(&app_state.app_config, &[]);
                    ProviderStreamState::Custom(stream)
                }
                ProviderAllocation::Available(ref provider_cfg)
                | ProviderAllocation::GracePeriod(ref provider_cfg) => {
                    let allocation_kind = match allocation {
                        ProviderAllocation::Available(_) => "available",
                        ProviderAllocation::GracePeriod(_) => "grace_period",
                        ProviderAllocation::Exhausted => "exhausted",
                    };

                    // force_stream_provider means we keep the url and the provider.
                    // If force_stream_provider or the input is the same as the config we don't need to get new url
                    let (selected_provider_name, url) = if force_provider.is_some() || provider_cfg.id == input.id {
                        (input.name.clone(), stream_url.to_string())
                    } else {
                        (
                            provider_cfg.name.clone(),
                            get_stream_alternative_url(stream_url, input, provider_cfg),
                        )
                    };

                    if let Some(user_info) = provider_cfg.get_user_info() {
                        debug_if_enabled!(
                            "provider session: input={} provider_cfg={} user={} allocation={} stream_url={}",
                            sanitize_sensitive_info(&input.name),
                            sanitize_sensitive_info(&provider_cfg.name),
                            sanitize_sensitive_info(&user_info.username),
                            allocation_kind,
                            sanitize_sensitive_info(&url)
                        );
                    } else {
                        debug_if_enabled!(
                            "provider session: input={} provider_cfg={} user=? allocation={} stream_url={}",
                            sanitize_sensitive_info(&input.name),
                            sanitize_sensitive_info(&provider_cfg.name),
                            allocation_kind,
                            sanitize_sensitive_info(&url)
                        );
                    }

                    match allocation {
                        ProviderAllocation::Exhausted => {
                            let stream = create_provider_connections_exhausted_stream(&app_state.app_config, &[]);
                            ProviderStreamState::Custom(stream)
                        },
                        ProviderAllocation::Available(_) => ProviderStreamState::Available(Some(selected_provider_name), url),
                        ProviderAllocation::GracePeriod(_) => ProviderStreamState::GracePeriod(Some(selected_provider_name), url),
                    }
                }
            }
        } else {
            debug!("Provider {} is exhausted. No connections allowed.", input.name);
            let stream = create_provider_connections_exhausted_stream(&app_state.app_config, &[]);
            ProviderStreamState::Custom(stream)
        };


    StreamingStrategy {
        provider_handle: provider_connection_handle,
        provider_stream_state: stream_response_params,
        input_headers: Some(input.headers.clone()),
    }
}

fn get_grace_period_millis(
    connection_permission: UserConnectionPermission,
    stream_response_params: &ProviderStreamState,
    config_grace_period_millis: u64,
) -> u64 {
    if config_grace_period_millis > 0
        && (
        matches!(stream_response_params, ProviderStreamState::GracePeriod(_, _)) // provider grace period
            || connection_permission == UserConnectionPermission::GracePeriod
        // user grace period
    )
    {
        config_grace_period_millis
    } else {
        0
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn create_stream_response_details(
    app_state: &Arc<AppState>,
    stream_options: &StreamOptions,
    stream_url: &str,
    fingerprint: &Fingerprint,
    req_headers: &HeaderMap,
    input: &ConfigInput,
    item_type: PlaylistItemType,
    share_stream: bool,
    connection_permission: UserConnectionPermission,
    force_provider: Option<&str>,
) -> StreamDetails {
    let mut streaming_strategy = resolve_streaming_strategy(app_state, stream_url, fingerprint, input, force_provider).await;
    let config_grace_period_millis = app_state
        .app_config
        .config
        .load()
        .reverse_proxy
        .as_ref()
        .and_then(|r| r.stream.as_ref())
        .map_or_else(default_grace_period_millis, |s| s.grace_period_millis);
    let mut grace_period_millis = get_grace_period_millis(
        connection_permission,
        &streaming_strategy.provider_stream_state,
        config_grace_period_millis,
    );

    if let ProviderStreamState::GracePeriod(ref provider_grace_check, ref request_url) =
        streaming_strategy.provider_stream_state
    {
        tokio::time::sleep(tokio::time::Duration::from_millis(grace_period_millis)).await;
        if let Some(provider_name) = provider_grace_check.as_ref() {
            if app_state.active_provider.is_over_limit(provider_name).await {
                debug!("Provider connections exhausted after grace period for provider: {provider_name}");

                let provider_handle = streaming_strategy.provider_handle.take();
                app_state.connection_manager.release_provider_handle(provider_handle).await;

                // If panel_api is configured, try to recover by provisioning a new account (if needed)
                // and then re-acquire without provider grace allocations.
                let mut recovered = false;
                if force_provider.is_none() && input.panel_api.is_some() {
                    debug_if_enabled!(
                        "panel_api: provider {} over limit after grace; attempting re-acquire/provision for input {}",
                        sanitize_sensitive_info(provider_name),
                        sanitize_sensitive_info(&input.name)
                    );

                    let mut new_handle = app_state
                        .active_provider
                        .acquire_connection_with_grace_override(&input.name, &fingerprint.addr, false)
                        .await;

                    if new_handle.is_none()
                        && try_provision_account_on_exhausted(app_state, input).await
                    {
                        debug_if_enabled!(
                            "panel_api: provision succeeded after grace exhaustion for input {}; re-acquiring (no grace)",
                            sanitize_sensitive_info(&input.name)
                        );
                        new_handle = app_state
                            .active_provider
                            .acquire_connection_with_grace_override(&input.name, &fingerprint.addr, false)
                            .await;
                    }

                    if let Some(handle) = new_handle {
                        if let Some(provider_cfg) = handle.allocation.get_provider_config() {
                            let (selected_provider_name, url) = if provider_cfg.id == input.id {
                                (input.name.clone(), stream_url.to_string())
                            } else {
                                (
                                    provider_cfg.name.clone(),
                                    get_stream_alternative_url(stream_url, input, &provider_cfg),
                                )
                            };
                            streaming_strategy.provider_stream_state =
                                ProviderStreamState::Available(Some(selected_provider_name), url);
                            streaming_strategy.provider_handle = Some(handle);
                            recovered = true;
                        } else {
                            app_state
                                .connection_manager
                                .release_provider_handle(Some(handle))
                                .await;
                        }
                    }
                }

                if !recovered {
                    app_state
                        .connection_manager
                        .update_stream_detail(
                            &fingerprint.addr,
                            CustomVideoStreamType::ProviderConnectionsExhausted,
                        )
                        .await;
                    let stream = create_provider_connections_exhausted_stream(&app_state.app_config, &[]);
                    streaming_strategy.provider_stream_state = ProviderStreamState::Custom(stream);
                }
            }
        } else {
            streaming_strategy.provider_stream_state =
                ProviderStreamState::Available(provider_grace_check.clone(), request_url.clone());
        }
    }

    // Recompute grace period after potential recovery/strategy changes.
    grace_period_millis = get_grace_period_millis(
        connection_permission,
        &streaming_strategy.provider_stream_state,
        config_grace_period_millis,
    );

    let guard_provider_name = streaming_strategy
        .provider_handle
        .as_ref()
        .and_then(|guard| guard.allocation.get_provider_name());

    match streaming_strategy.provider_stream_state {
        // custom stream means we display our own stream like connection exhausted, channel-unavailable...
        ProviderStreamState::Custom(provider_stream) => {
            let (stream, stream_info) = provider_stream;
            StreamDetails {
                stream,
                stream_info,
                provider_name: guard_provider_name.clone(),
                grace_period_millis,
                reconnect_flag: None,
                provider_handle: streaming_strategy.provider_handle.clone(),
            }
        }
        ProviderStreamState::Available(_provider_name, request_url)
        | ProviderStreamState::GracePeriod(_provider_name, request_url) => {
            let parsed_url = Url::parse(&request_url);
            let ((stream, stream_info), reconnect_flag) = if let Ok(url) = parsed_url {
                let disabled_headers = app_state.get_disabled_headers();
                let provider_stream_factory_options = ProviderStreamFactoryOptions::new(
                    fingerprint.addr,
                    item_type,
                    share_stream,
                    stream_options,
                    &url,
                    req_headers,
                    streaming_strategy.input_headers.as_ref(),
                    disabled_headers.as_ref(),
                );
                let reconnect_flag = provider_stream_factory_options.get_reconnect_flag_clone();
                let provider_stream = match create_provider_stream(
                    app_state,
                    &app_state.http_client.load(),
                    provider_stream_factory_options,
                )
                    .await
                {
                    None => (None, None),
                    Some((stream, info)) => (Some(stream), info),
                };
                (provider_stream, Some(reconnect_flag))
            } else {
                ((None, None), None)
            };

            if log_enabled!(log::Level::Debug) {
                if let Some((headers, status_code, response_url, _custom_video_type)) = stream_info.as_ref() {
                    debug!(
                        "Responding stream request {} with status {}, headers {:?}",
                        sanitize_sensitive_info(
                            response_url.as_ref().map_or(stream_url, |s| s.as_str())
                        ),
                        status_code,
                        headers
                    );
                }
            }

            // if we have no stream, we should release the provider
            let provider_handle = if stream.is_none() {
                let provider_handle = streaming_strategy.provider_handle.take();
                app_state.connection_manager.release_provider_handle(provider_handle).await;
                error!("Cant open stream {}", sanitize_sensitive_info(&request_url));
                None
            } else {
                streaming_strategy.provider_handle.take()
            };

            StreamDetails {
                stream,
                stream_info,
                provider_name: guard_provider_name.clone(),
                grace_period_millis,
                reconnect_flag,
                provider_handle,
            }
        }
    }
}

pub struct RedirectParams<'a, P>
where
    P: PlaylistEntry,
{
    pub item: &'a P,
    pub provider_id: Option<u32>,
    pub cluster: XtreamCluster,
    pub target_type: TargetType,
    pub target: &'a ConfigTarget,
    pub input: &'a ConfigInput,
    pub user: &'a ProxyUserCredentials,
    pub stream_ext: Option<&'a str>,
    pub req_context: ApiStreamContext,
    pub action_path: &'a str,
}

impl<P> RedirectParams<'_, P>
where
    P: PlaylistEntry,
{
    pub fn get_query_path(&self, provider_id: u32, url: &str) -> String {
        let extension = self.stream_ext.map_or_else(
            || {
                extract_extension_from_url(url)
                    .map_or_else(String::new, std::string::ToString::to_string)
            },
            std::string::ToString::to_string,
        );

        // if there is an action_path (like for timeshift duration/start), it will be added in front of the stream_id
        if self.action_path.is_empty() {
            format!("{provider_id}{extension}")
        } else {
            format!("{}/{provider_id}{extension}", trim_slash(self.action_path))
        }
    }
}

pub async fn redirect_response<'a, P>(
    app_state: &AppState,
    params: &'a RedirectParams<'a, P>,
) -> Option<impl IntoResponse + Send>
where
    P: PlaylistEntry,
{
    let item_type = params.item.get_item_type();
    let provider_url = &params.item.get_provider_url();

    let redirect_request =
        params.user.proxy.is_redirect(item_type) || params.target.is_force_redirect(item_type);
    let is_hls_request =
        item_type == PlaylistItemType::LiveHls || params.stream_ext == Some(HLS_EXT);
    let is_dash_request = (!is_hls_request && item_type == PlaylistItemType::LiveDash)
        || params.stream_ext == Some(DASH_EXT);

    if params.target_type == TargetType::M3u {
        if redirect_request || is_dash_request {
            let redirect_url = if is_hls_request {
                &replace_url_extension(provider_url, HLS_EXT)
            } else {
                provider_url
            };
            let redirect_url = if is_dash_request {
                &replace_url_extension(redirect_url, DASH_EXT)
            } else {
                redirect_url
            };
            let redirect_url =
                get_redirect_alternative_url(app_state, redirect_url, params.input).await;
            debug_if_enabled!(
                "Redirecting stream request to {}",
                sanitize_sensitive_info(&redirect_url)
            );
            return Some(redirect(&redirect_url).into_response());
        }
    } else if params.target_type == TargetType::Xtream {
        let Some(provider_id) = params.provider_id else {
            return Some(axum::http::StatusCode::BAD_REQUEST.into_response());
        };

        if redirect_request {
            // handle redirect for series but why?
            if params.cluster == XtreamCluster::Series {
                let ext = params.stream_ext.unwrap_or_default();
                let url = params.input.url.as_str();
                let username = params.input.username.as_ref().map_or("", |v| v);
                let password = params.input.password.as_ref().map_or("", |v| v);
                // TODO do i need action_path like for timeshift ?
                let stream_url = format!("{url}/series/{username}/{password}/{provider_id}{ext}");
                debug_if_enabled!(
                    "Redirecting stream request to {}",
                    sanitize_sensitive_info(&stream_url)
                );
                return Some(redirect(&stream_url).into_response());
            }

            let target_name = params.target.name.as_str();
            let virtual_id = params.item.get_virtual_id();
            let stream_url = match get_xtream_player_api_stream_url(
                params.input,
                params.req_context,
                &params.get_query_path(provider_id, provider_url),
                provider_url,
            ) {
                None => {
                    error!("Cant find stream url for target {target_name}, context {}, stream_id {virtual_id}", params.req_context);
                    return Some(axum::http::StatusCode::BAD_REQUEST.into_response());
                }
                Some(url) => {
                    match app_state
                        .active_provider
                        .get_next_provider(&params.input.name)
                        .await
                    {
                        Some(provider_cfg) => {
                            get_stream_alternative_url(&url, params.input, &provider_cfg)
                        }
                        None => url,
                    }
                }
            };

            // hls or dash redirect
            if is_dash_request {
                let redirect_url = if is_hls_request {
                    &replace_url_extension(&stream_url, HLS_EXT)
                } else {
                    &replace_url_extension(&stream_url, DASH_EXT)
                };
                debug_if_enabled!(
                    "Redirecting stream request to {}",
                    sanitize_sensitive_info(redirect_url)
                );
                return Some(redirect(redirect_url).into_response());
            }

            debug_if_enabled!(
                "Redirecting stream request to {}",
                sanitize_sensitive_info(&stream_url)
            );
            return Some(redirect(&stream_url).into_response());
        }
    }

    None
}

fn is_throttled_stream(item_type: PlaylistItemType, throttle_kbps: usize) -> bool {
    throttle_kbps > 0
        && matches!(
            item_type,
            PlaylistItemType::Video
                | PlaylistItemType::Series
                | PlaylistItemType::SeriesInfo
                | PlaylistItemType::Catchup
        )
}

fn prepare_body_stream(
    app_state: &AppState,
    item_type: PlaylistItemType,
    stream: ActiveClientStream,
) -> axum::body::Body {
    let throttle_kbps = usize::try_from(get_stream_throttle(app_state)).unwrap_or_default();
    let body_stream = if is_throttled_stream(item_type, throttle_kbps) {
        axum::body::Body::from_stream(ThrottledStream::new(stream.boxed(), throttle_kbps))
    } else {
        axum::body::Body::from_stream(stream)
    };
    body_stream
}

/// # Panics
pub async fn force_provider_stream_response(
    fingerprint: &Fingerprint,
    app_state: &Arc<AppState>,
    user_session: &UserSession,
    mut stream_channel: StreamChannel,
    req_headers: &HeaderMap,
    input: &ConfigInput,
    user: &ProxyUserCredentials,
) -> impl IntoResponse + Send {
    let stream_options = get_stream_options(app_state);
    let share_stream = false;
    let connection_permission = UserConnectionPermission::Allowed;
    let item_type = stream_channel.item_type;

    let stream_details = create_stream_response_details(
        app_state,
        &stream_options,
        &user_session.stream_url,
        fingerprint,
        req_headers,
        input,
        item_type,
        share_stream,
        connection_permission,
        Some(&user_session.provider),
    )
        .await;

    if stream_details.has_stream() {
        let provider_response = stream_details
            .stream_info
            .as_ref()
            .map(|(h, sc, url, cvt)| (h.clone(), *sc, url.clone(), *cvt));
        app_state
            .active_users
            .update_session_addr(&user.username, &user_session.token, &fingerprint.addr)
            .await;
        stream_channel.shared = share_stream;
        let stream =
            ActiveClientStream::new(stream_details, app_state, user, connection_permission, fingerprint, stream_channel, Some(&user_session.token), req_headers)
                .await;

        let (status_code, header_map) =
            get_stream_response_with_headers(provider_response.map(|(h, s, _, _)| (h, s)));
        let mut response = axum::response::Response::builder().status(status_code);
        for (key, value) in &header_map {
            response = response.header(key, value);
        }

        let body_stream = prepare_body_stream(app_state, item_type, stream);
        debug_if_enabled!(
            "Streaming provider forced stream request from {}",
            sanitize_sensitive_info(&user_session.stream_url)
        );
        return try_unwrap_body!(response.body(body_stream));
    }

    app_state.connection_manager.release_provider_handle(stream_details.provider_handle).await;
    if let (Some(stream), _stream_info) = create_channel_unavailable_stream(
        &app_state.app_config,
        &[],
        axum::http::StatusCode::SERVICE_UNAVAILABLE,
    ) {
        app_state.connection_manager.update_stream_detail(&fingerprint.addr, CustomVideoStreamType::ChannelUnavailable).await;
        debug!("Streaming custom stream");
        try_unwrap_body!(axum::response::Response::builder()
            .status(axum::http::StatusCode::OK)
            .body(axum::body::Body::from_stream(stream)))
    } else {
        axum::http::StatusCode::BAD_REQUEST.into_response()
    }
}

/// # Panics
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub async fn stream_response(
    fingerprint: &Fingerprint,
    app_state: &Arc<AppState>,
    session_token: &str,
    mut stream_channel: StreamChannel,
    stream_url: &str,
    req_headers: &HeaderMap,
    input: &ConfigInput,
    target: &ConfigTarget,
    user: &ProxyUserCredentials,
    connection_permission: UserConnectionPermission,
) -> impl IntoResponse + Send {
    if log_enabled!(log::Level::Trace) {
        trace!("Try to open stream {}", sanitize_sensitive_info(stream_url));
    }

    if connection_permission == UserConnectionPermission::Exhausted {
        return create_custom_video_stream_response(
            app_state,
            &fingerprint.addr,
            CustomVideoStreamType::UserConnectionsExhausted,
        ).await
            .into_response();
    }

    let virtual_id = stream_channel.virtual_id;
    let item_type = stream_channel.item_type;

    let share_stream = is_stream_share_enabled(item_type, target);
    let _shared_lock = if share_stream {
        let write_lock = app_state.app_config.file_locks.write_lock_str(stream_url).await;

        if let Some(value) =
            try_shared_stream_response_if_any(app_state, stream_url, fingerprint, user, connection_permission, stream_channel.clone(), session_token, req_headers).await
        {
            return value.into_response();
        }
        Some(write_lock)
    } else {
        None
    };

    let stream_options = get_stream_options(app_state);
    let mut stream_details = create_stream_response_details(
        app_state,
        &stream_options,
        stream_url,
        fingerprint,
        req_headers,
        input,
        item_type,
        share_stream,
        connection_permission,
        None,
    ).await;

    if stream_details.has_stream() {
        // let content_length = get_stream_content_length(provider_response.as_ref());
        let provider_response = stream_details
            .stream_info
            .as_ref()
            .map(|(h, sc, response_url, cvt)| (h.clone(), *sc, response_url.clone(), *cvt));
        let provider_name = stream_details.provider_name.clone();

        let provider_handle = if share_stream {
            stream_details.provider_handle.take()
        } else {
            None
        };

        let mut is_stream_shared = share_stream;
        if let Some((_header, _status_code, _url, Some(_custom_video))) = stream_details.stream_info.as_ref() {
            if stream_details.stream.is_some() {
                is_stream_shared = false;
            }
        }

        stream_channel.shared = is_stream_shared;
        let stream =
            ActiveClientStream::new(stream_details, app_state, user, connection_permission, fingerprint, stream_channel, Some(session_token), req_headers)
                .await;
        let stream_resp = if is_stream_shared {
            debug_if_enabled!("Streaming shared stream request from {}",sanitize_sensitive_info(stream_url));
            // Shared Stream response
            let shared_headers = provider_response
                .as_ref()
                .map_or_else(Vec::new, |(h, _, _, _)| h.clone());

            if let Some((broadcast_stream, _shared_provider)) = SharedStreamManager::register_shared_stream(
                app_state,
                stream_url,
                stream,
                &fingerprint.addr,
                shared_headers,
                stream_options.buffer_size,
                provider_handle,
            )
                .await
            {
                let (status_code, header_map) =
                    get_stream_response_with_headers(provider_response.map(|(h, s, _, _)| (h, s)));
                let mut response = axum::response::Response::builder().status(status_code);
                for (key, value) in &header_map {
                    response = response.header(key, value);
                }
                try_unwrap_body!(response.body(axum::body::Body::from_stream(broadcast_stream)))
            } else {
                axum::http::StatusCode::BAD_REQUEST.into_response()
            }
        } else {
            let session_url = provider_response
                .as_ref()
                .and_then(|(_, _, u, _)| u.as_ref())
                .map_or_else(
                    || Cow::Borrowed(stream_url),
                    |url| Cow::Owned(url.to_string()),
                );
            if log_enabled!(log::Level::Debug) {
                if session_url.eq(&stream_url) {
                    debug!("Streaming stream request from {}", sanitize_sensitive_info(stream_url)
                    );
                } else {
                    debug!(
                        "Streaming stream request for {} from {}",
                        sanitize_sensitive_info(stream_url),
                        sanitize_sensitive_info(&session_url)
                    );
                }
            }
            let (status_code, header_map) =
                get_stream_response_with_headers(provider_response.map(|(h, s, _, _)| (h, s)));
            let mut response = axum::response::Response::builder().status(status_code);
            for (key, value) in &header_map {
                response = response.header(key, value);
            }

            if let Some(provider) = provider_name {
                if matches!(
                    item_type,
                    PlaylistItemType::LiveHls
                        | PlaylistItemType::LiveDash
                        | PlaylistItemType::Video
                        | PlaylistItemType::Series
                        | PlaylistItemType::Catchup
                ) {
                    let _ = app_state
                        .active_users
                        .create_user_session(
                            user,
                            session_token,
                            virtual_id,
                            &provider,
                            &session_url,
                            &fingerprint.addr,
                            connection_permission,
                        )
                        .await;
                }
            }

            let body_stream = prepare_body_stream(app_state, item_type, stream);
            try_unwrap_body!(response.body(body_stream))
        };

        return stream_resp.into_response();
    }
    app_state.connection_manager.release_provider_handle(stream_details.provider_handle).await;
    axum::http::StatusCode::BAD_REQUEST.into_response()
}

fn get_stream_throttle(app_state: &AppState) -> u64 {
    app_state
        .app_config
        .config
        .load()
        .reverse_proxy
        .as_ref()
        .and_then(|reverse_proxy| reverse_proxy.stream.as_ref())
        .map(|stream| stream.throttle_kbps)
        .unwrap_or_default()
}

#[allow(clippy::too_many_arguments)]
async fn try_shared_stream_response_if_any(
    app_state: &Arc<AppState>,
    stream_url: &str,
    fingerprint: &Fingerprint,
    user: &ProxyUserCredentials,
    connect_permission: UserConnectionPermission,
    mut stream_channel: StreamChannel,
    session_token: &str,
    req_headers: &HeaderMap,
) -> Option<impl IntoResponse> {
    if let Some((stream, provider)) =
        SharedStreamManager::subscribe_shared_stream(app_state, stream_url, &fingerprint.addr).await
    {
        debug_if_enabled!("Using shared stream {}", sanitize_sensitive_info(stream_url)
        );
        if let Some(headers) = app_state
            .shared_stream_manager
            .get_shared_state_headers(stream_url)
            .await
        {
            let (status_code, header_map) = get_stream_response_with_headers(Some((
                headers.clone(),
                axum::http::StatusCode::OK,
            )));
            let mut stream_details = StreamDetails::from_stream(stream);

            stream_details.provider_name = provider;
            stream_channel.shared = true;
            let stream =
                ActiveClientStream::new(stream_details, app_state, user, connect_permission, fingerprint, stream_channel, Some(session_token), req_headers)
                    .await
                    .boxed();
            let mut response = axum::response::Response::builder().status(status_code);
            for (key, value) in &header_map {
                response = response.header(key, value);
            }
            return response.body(axum::body::Body::from_stream(stream)).ok();
        }
    }
    None
}

pub fn is_stream_share_enabled(item_type: PlaylistItemType, target: &ConfigTarget) -> bool {
    (item_type == PlaylistItemType::Live/* || item_type == PlaylistItemType::LiveHls */)
        && target
        .options
        .as_ref()
        .is_some_and(|opt| opt.share_live_streams)
}

pub type HeaderFilter = Option<Box<dyn Fn(&str) -> bool + Send>>;
pub fn get_headers_from_request(
    req_headers: &HeaderMap,
    filter: &HeaderFilter,
) -> HashMap<String, Vec<u8>> {
    req_headers
        .iter()
        .filter(|(k, _)| match &filter {
            None => true,
            Some(predicate) => predicate(k.as_str()),
        })
        .map(|(k, v)| (k.as_str().to_string(), v.as_bytes().to_vec()))
        .collect()
}

fn get_add_cache_content(
    res_url: &str,
    cache: &Arc<ArcSwapOption<Mutex<LRUResourceCache>>>,
) -> Arc<dyn Fn(usize) + Send + Sync> {
    let resource_url = String::from(res_url);
    let cache = Arc::clone(cache);
    let add_cache_content: Arc<dyn Fn(usize) + Send + Sync> = Arc::new(move |size| {
        let res_url = resource_url.clone();

        // todo spawn, replace with unboundchannel
        let cache = Arc::clone(&cache);
        tokio::spawn(async move {
            if let Some(cache) = cache.load().as_ref() {
                let _ = cache.lock().await.add_content(&res_url, size);
            }
        });
    });
    add_cache_content
}

async fn build_stream_response(
    app_state: &AppState,
    resource_url: &str,
    response: reqwest::Response,
) -> axum::response::Response {
    let sanitized_resource_url = sanitize_sensitive_info(resource_url);
    let status = response.status();
    let mut response_builder =
        axum::response::Response::builder().status(status);
    let has_content_range = response.headers().contains_key(axum::http::header::CONTENT_RANGE);
    for (key, value) in response.headers() {
        let name = key.as_str();
        let is_hop_by_hop = matches!(
            name.to_ascii_lowercase().as_str(),
            "connection"
                | "keep-alive"
                | "proxy-authenticate"
                | "proxy-authorization"
                | "te"
                | "trailer"
                | "transfer-encoding"
                | "upgrade"
        );
        if !is_hop_by_hop {
            response_builder = response_builder.header(key, value);
        }
    }
    let byte_stream = response
        .bytes_stream()
        .map_err(|err| StreamError::reqwest(&err));
    // Cache only complete responses (200 OK without Content-Range)
    let can_cache = status == axum::http::StatusCode::OK && !has_content_range;
    if can_cache {
        debug!( "Caching eligible resource stream {sanitized_resource_url}");
        let cache_resource_path = if let Some(cache) = app_state.cache.load().as_ref() {
            Some(cache.lock().await.store_path(resource_url))
        } else {
            None
        };
        if let Some(resource_path) = cache_resource_path {
            match create_new_file_for_write(&resource_path).await {
                Ok(file) => {
                    debug!("Persisting resource stream {sanitized_resource_url} to {}", resource_path.display());
                    let writer = async_file_writer(file);
                    let add_cache_content = get_add_cache_content(resource_url, &app_state.cache);
                    let tee = tee_stream(byte_stream, writer, &resource_path, add_cache_content);
                    return try_unwrap_body!(response_builder.body(axum::body::Body::from_stream(tee)));
                }
                Err(err) => {
                    warn!("Failed to create cache file {} for {sanitized_resource_url}: {err}", resource_path.display());
                }
            }
        } else {
            debug!("Resource cache unavailable; streaming response for {sanitized_resource_url} without persistence");
        }
    }

    try_unwrap_body!(response_builder.body(axum::body::Body::from_stream(byte_stream)))
}

async fn fetch_resource_with_retry(
    app_state: &AppState,
    url: &Url,
    resource_url: &str,
    req_headers: &HashMap<String, Vec<u8>>,
    input: Option<&ConfigInput>,
) -> Option<axum::response::Response> {
    let config = app_state.app_config.config.load();
    let (max_attempts, backoff_ms, backoff_multiplier) = config
        .reverse_proxy
        .as_ref()
        .map_or_else(ResourceRetryConfig::get_default_retry_values, |rp| rp.resource_retry.get_retry_values());
    let disabled_headers = app_state.get_disabled_headers();
    for attempt in 0..max_attempts {
        let client = request::get_client_request(
            &app_state.http_client.load(),
            input.map_or(InputFetchMethod::GET, |i| i.method),
            input.map(|i| &i.headers),
            url,
            Some(req_headers),
            disabled_headers.as_ref(),
        );
        match client.send().await {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    return Some(
                        build_stream_response(app_state, resource_url, response).await,
                    );
                }
                // Retry only for 408, 425, 429 and all 5xx statuses
                let should_retry = status.is_server_error()
                    || matches!(
                        status,
                        // reqwest::StatusCode::BAD_REQUEST // 400 is typically client error; retrying likely won't help and adds load.
                            reqwest::StatusCode::REQUEST_TIMEOUT
                            | reqwest::StatusCode::TOO_EARLY
                            | reqwest::StatusCode::TOO_MANY_REQUESTS
                    );

                if attempt < max_attempts - 1 && should_retry {
                    let wait_dur = response
                        .headers()
                        .get(RETRY_AFTER)
                        .and_then(|h| h.to_str().ok())
                        .and_then(|s| s.parse::<u64>().ok())
                        .map_or_else(
                            || {
                                let delay = calculate_retry_backoff(backoff_ms, backoff_multiplier, attempt);
                                Duration::from_millis(delay)
                            },
                            Duration::from_secs,
                        );
                    tokio::time::sleep(wait_dur).await;
                    continue;
                }

                // For non-retriable statuses or when attempts are exhausted, return upstream response including body
                debug_if_enabled!(
                    "Failed to open resource got status {status} for {}",
                    sanitize_sensitive_info(resource_url)
                );
                let mut response_builder = axum::response::Response::builder().status(status);
                for (key, value) in response.headers() {
                    response_builder = response_builder.header(key, value);
                }
                let stream = response
                    .bytes_stream()
                    .map_err(|err| StreamError::reqwest(&err));
                return Some(try_unwrap_body!(
                    response_builder.body(axum::body::Body::from_stream(stream))
                ));
            }
            Err(err) => {
                if attempt < max_attempts - 1 {
                    let delay = calculate_retry_backoff(backoff_ms, backoff_multiplier, attempt);
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    continue;
                }
                error!("Received failure from server {}:  {err}", sanitize_sensitive_info(resource_url));
            }
        }
        break;
    }
    None
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
fn calculate_retry_backoff(base_delay_ms: u64, multiplier: f64, attempt: u32) -> u64 {
    let base = base_delay_ms.max(1);
    if multiplier <= 1.0 {
        return base;
    }
    let delay = (base as f64) * multiplier.powi(i32::try_from(attempt).unwrap_or(i32::MAX));
    if !delay.is_finite() || delay < 1.0 {
        base
    } else if delay >= u64::MAX as f64 {
        u64::MAX
    } else {
        delay as u64
    }
}

/// # Panics
pub async fn resource_response(
    app_state: &AppState,
    resource_url: &str,
    req_headers: &HeaderMap,
    input: Option<&ConfigInput>,
) -> impl IntoResponse + Send {
    if resource_url.is_empty() {
        return axum::http::StatusCode::NO_CONTENT.into_response();
    }
    let filter: HeaderFilter = Some(Box::new(|key| {
        key != "if-none-match" && key != "if-modified-since"
    }));
    let req_headers = get_headers_from_request(req_headers, &filter);
    if let Some(cache) = app_state.cache.load().as_ref() {
        let mut guard = cache.lock().await;
        if let Some(resource_path) = guard.get_content(resource_url) {
            trace_if_enabled!("Responding resource from cache {}", sanitize_sensitive_info(resource_url));
            return serve_file(&resource_path, mime::APPLICATION_OCTET_STREAM)
                .await
                .into_response();
        }
    }
    trace_if_enabled!(
        "Try to fetch resource {}",
        sanitize_sensitive_info(resource_url)
    );
    if let Ok(url) = Url::parse(resource_url) {
        if let Some(resp) =
            fetch_resource_with_retry(app_state, &url, resource_url, &req_headers, input).await {
            return resp;
        }
        // Upstream failure after retries
        return axum::http::StatusCode::BAD_GATEWAY.into_response();
    }
    error!("Url is malformed {}", sanitize_sensitive_info(resource_url));
    axum::http::StatusCode::BAD_REQUEST.into_response()
}

pub fn separate_number_and_remainder(input: &str) -> (String, Option<String>) {
    input.rfind('.').map_or_else(
        || (input.to_string(), None),
        |dot_index| {
            let number_part = input[..dot_index].to_string();
            let rest = input[dot_index..].to_string();
            (number_part, if rest.len() < 2 { None } else { Some(rest) })
        },
    )
}

/// # Panics
pub fn empty_json_list_response() -> axum::response::Response {
    try_unwrap_body!(axum::response::Response::builder()
        .status(axum::http::StatusCode::OK)
        .header(axum::http::header::CONTENT_TYPE, mime::APPLICATION_JSON.to_string())
        .body("[]".to_owned()))
}

pub fn get_username_from_auth_header(token: &str, app_state: &Arc<AppState>) -> Option<String> {
    if let Some(web_auth_config) = &app_state
        .app_config
        .config
        .load()
        .web_ui
        .as_ref()
        .and_then(|c| c.auth.as_ref())
    {
        let secret_key: &[u8] = web_auth_config.secret.as_ref();
        if let Ok(token_data) = decode::<Claims>(
            token,
            &DecodingKey::from_secret(secret_key),
            &Validation::new(Algorithm::HS256),
        ) {
            return Some(token_data.claims.username);
        }
    }
    None
}

/// # Panics
pub fn redirect(url: &str) -> impl IntoResponse {
    try_unwrap_body!(axum::response::Response::builder()
        .status(axum::http::StatusCode::FOUND)
        .header(axum::http::header::LOCATION, url)
        .body(axum::body::Body::empty()))
}

pub async fn is_seek_request(cluster: XtreamCluster, req_headers: &HeaderMap) -> bool {
    // seek only for non-live streams
    if cluster == XtreamCluster::Live {
        return false;
    }

    // seek requests contains range header
    let range = req_headers
        .get("range")
        .and_then(|h| h.to_str().ok())
        .map(ToString::to_string);

    if let Some(range) = range {
        // if range.starts_with("bytes=0-") {
        //     return false;
        // }
        if range.starts_with("bytes=") {
            return true;
        }
    }
    false
}

pub fn bin_response<T: Serialize>(data: &T) -> impl IntoResponse + Send {
    match bin_serialize(data) {
        Ok(body) => (
            [(axum::http::header::CONTENT_TYPE, CONTENT_TYPE_BIN)],
            body,
        ).into_response(),
        Err(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub fn json_response<T: Serialize>(data: &T) -> impl IntoResponse + Send {
    (axum::http::StatusCode::OK, axum::Json(data)).into_response()
}

pub fn json_or_bin_response<T: Serialize>(accept: Option<&String>, data: &T) -> impl IntoResponse + Send {
    if accept.is_some_and(|a| a.contains(CONTENT_TYPE_BIN)) {
        return bin_response(data).into_response();
    }
    json_response(data).into_response()
}

pub fn create_session_fingerprint(fingerprint: &str, username: &str, virtual_id: u32) -> String {
    format!("{fingerprint}|{username}|{virtual_id}")
}
