use std::net::SocketAddr;
use crate::api::config_file::ConfigFile;
use crate::api::model::{create_panel_api_provisioning_stream_with_stop, create_provider_connections_exhausted_stream, AppState, StreamDetails};
use crate::model::{is_input_expired, ConfigInput, ProxyUserCredentials};
use crate::utils::{debug_if_enabled, persist_source_config, read_sources_file_from_path};
use log::{error, warn};
use serde_json::Value;
use shared::error::{info_err_res, info_err, TuliproxError};
use shared::model::{ConfigInputAliasDto, InputType, PanelApiAliasPoolSizeValue, PanelApiConfigDto, PanelApiProvisioningMethod, PanelApiQueryParamDto, ProxyUserStatus, VirtualId};
use shared::utils::{get_base_url_from_str, get_credentials_from_url, get_credentials_from_url_str, parse_timestamp, sanitize_sensitive_info};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use axum::http::{Method, StatusCode};
use url::Url;
use shared::{concat_string};
use crate::repository::{get_csv_file_path, csv_patch_batch_append, csv_patch_batch_remove_expired, csv_patch_batch_update_credentials, csv_patch_batch_update_exp_date};
use crate::tools::atomic_once_flag::AtomicOnceFlag;
use jsonwebtoken::get_current_timestamp;

#[derive(Debug, Clone)]
struct AccountCredentials {
    name: String,
    username: String,
    password: String,
    exp_date: Option<i64>,
}

fn parse_boolish(value: &Value) -> bool {
    match value {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_i64().unwrap_or(0) != 0,
        Value::String(s) => matches!(s.trim().to_lowercase().as_str(), "true" | "1" | "yes" | "y" | "ok"),
        _ => false,
    }
}

fn extract_stringish(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn is_date_only_yyyy_mm_dd(value: &str) -> bool {
    let value = value.trim();
    if value.len() != 10 {
        return false;
    }
    let bytes = value.as_bytes();
    bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[0..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[8..10].iter().all(u8::is_ascii_digit)
}

fn first_json_object(value: &Value) -> Option<&serde_json::Map<String, Value>> {
    match value {
        Value::Array(arr) => arr.first().and_then(|v| v.as_object()),
        Value::Object(obj) => Some(obj),
        _ => None,
    }
}

fn extract_username_password_from_json(obj: &serde_json::Map<String, Value>) -> Option<(String, String)> {
    let username = obj.get("username").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty());
    let password = obj.get("password").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty());
    match (username, password) {
        (Some(u), Some(p)) => Some((u.to_string(), p.to_string())),
        _ => None,
    }
}

fn validate_type_is_m3u(params: &[PanelApiQueryParamDto]) -> Result<(), TuliproxError> {
    let typ = params
        .iter()
        .find(|p| p.key.trim().eq_ignore_ascii_case("type"))
        .map(|p| p.value.trim().to_string());
    match typ {
        Some(v) if v.eq_ignore_ascii_case("m3u") => Ok(()),
        Some(v) => info_err_res!("panel_api: unsupported type={v}, only m3u is supported"),
        None => info_err_res!("panel_api: missing required query param 'type=m3u'"),
    }
}

fn require_api_key_param(params: &[PanelApiQueryParamDto], section: &str) -> Result<(), TuliproxError> {
    let api_key = params.iter().find(|p| p.key.trim().eq_ignore_ascii_case("api_key"));
    let Some(api_key) = api_key else {
        return info_err_res!("panel_api: {section} must contain query param 'api_key' (use value 'auto')"
        );
    };
    if api_key.value.trim().is_empty() {
        return info_err_res!("panel_api: {section} query param 'api_key' must not be empty (use value 'auto')");
    }
    Ok(())
}

fn require_username_password_params_auto(params: &[PanelApiQueryParamDto], section: &str) -> Result<(), TuliproxError> {
    let username = params.iter().find(|p| p.key.trim().eq_ignore_ascii_case("username"));
    let password = params.iter().find(|p| p.key.trim().eq_ignore_ascii_case("password"));
    if username.is_none() || password.is_none() {
        return info_err_res!("panel_api: {section} must contain query params 'username' and 'password' (use value 'auto')");
    }
    if !username.is_some_and(|p| p.value.trim().eq_ignore_ascii_case("auto"))
        || !password.is_some_and(|p| p.value.trim().eq_ignore_ascii_case("auto"))
    {
        return info_err_res!("panel_api: {section} requires 'username: auto' and 'password: auto' (credentials must not be hardcoded)");
    }
    Ok(())
}

fn validate_client_new_params(params: &[PanelApiQueryParamDto]) -> Result<(), TuliproxError> {
    require_api_key_param(params, "query_parameter.client_new")?;
    validate_type_is_m3u(params)?;
    if params.iter().any(|p| p.key.trim().eq_ignore_ascii_case("user")) {
        return info_err_res!("panel_api: client_new must not contain query param 'user'");
    }
    Ok(())
}

fn validate_client_renew_params(params: &[PanelApiQueryParamDto]) -> Result<(), TuliproxError> {
    require_api_key_param(params, "query_parameter.client_renew")?;
    validate_type_is_m3u(params)?;
    require_username_password_params_auto(params, "query_parameter.client_renew")?;
    Ok(())
}

fn validate_client_info_params(params: &[PanelApiQueryParamDto]) -> Result<(), TuliproxError> {
    require_api_key_param(params, "query_parameter.client_info")?;
    require_username_password_params_auto(params, "query_parameter.client_info")?;
    Ok(())
}

fn validate_account_info_params(params: &[PanelApiQueryParamDto]) -> Result<(), TuliproxError> {
    require_api_key_param(params, "query_parameter.account_info")?;
    let has_user = params.iter().any(|p| p.key.trim().eq_ignore_ascii_case("username"));
    let has_pass = params.iter().any(|p| p.key.trim().eq_ignore_ascii_case("password"));
    if has_user || has_pass {
        require_username_password_params_auto(params, "query_parameter.account_info")?;
    }
    Ok(())
}

fn validate_client_adult_content_params(params: &[PanelApiQueryParamDto]) -> Result<(), TuliproxError> {
    require_api_key_param(params, "query_parameter.client_adult_content")?;
    let has_user = params.iter().any(|p| p.key.trim().eq_ignore_ascii_case("username"));
    let has_pass = params.iter().any(|p| p.key.trim().eq_ignore_ascii_case("password"));
    if has_user || has_pass {
        require_username_password_params_auto(params, "query_parameter.client_adult_content")?;
    }
    Ok(())
}

fn parse_panel_api_provisioning_offset_secs(offset: &str) -> Result<u64, TuliproxError> {
    let raw = offset.trim();
    if raw.is_empty() {
        return Ok(0);
    }
    let lower = raw.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let last = *bytes.last().unwrap_or(&b'\0');
    let (num_part, multiplier) = match last {
        b's' => (&lower[..lower.len().saturating_sub(1)], 1_u64),
        b'm' => (&lower[..lower.len().saturating_sub(1)], 60_u64),
        b'h' => (&lower[..lower.len().saturating_sub(1)], 60_u64 * 60),
        b'd' => (&lower[..lower.len().saturating_sub(1)], 60_u64 * 60 * 24),
        b'0'..=b'9' => (lower.as_str(), 1_u64),
        _ => {
            return info_err_res!(
                "panel_api.provisioning.offset must be a number with optional suffix s/m/h/d (e.g. 30m, 12h), got '{raw}'"
            );
        }
    };
    let num_part = num_part.trim();
    if num_part.is_empty() {
        return info_err_res!(
            "panel_api.provisioning.offset must be a number with optional suffix s/m/h/d (e.g. 30m, 12h), got '{raw}'"
        );
    }
    let value: u64 = num_part
        .parse()
        .map_err(|_| info_err!("panel_api.provisioning.offset is not a valid number: '{raw}'"))?;
    value
        .checked_mul(multiplier)
        .ok_or_else(|| info_err!("panel_api.provisioning.offset is too large: '{raw}'"))
}

fn is_expiring_with_offset(exp_date: Option<i64>, offset_secs: u64) -> bool {
    let Some(exp_date) = exp_date else {
        return false;
    };
    let Ok(exp_ts) = u64::try_from(exp_date) else {
        return true;
    };
    get_current_timestamp().saturating_add(offset_secs) >= exp_ts
}

fn validate_panel_api_config(cfg: &PanelApiConfigDto) -> Result<(), TuliproxError> {
    if !cfg.enabled {
        return Ok(());
    }
    if cfg.url.trim().is_empty() {
        return info_err_res!("panel_api: url is missing");
    }
    if cfg.api_key.as_ref().is_none_or(|k| k.trim().is_empty()) {
        return info_err_res!("panel_api: api_key is missing");
    }
    if cfg.query_parameter.client_info.is_empty()
        || cfg.query_parameter.client_new.is_empty()
        || cfg.query_parameter.client_renew.is_empty()
    {
        return info_err_res!("panel_api: query_parameter.client_info/client_new/client_renew must be configured");
    }
    validate_client_info_params(&cfg.query_parameter.client_info)?;
    validate_client_new_params(&cfg.query_parameter.client_new)?;
    validate_client_renew_params(&cfg.query_parameter.client_renew)?;
    let (min_val, max_val) = alias_pool_limit_values(cfg);
    if let Some(PanelApiAliasPoolSizeValue::Number(value)) = min_val {
        if *value == 0 {
            return info_err_res!("panel_api.alias_pool.size.min must be greater than 0");
        }
    }
    if let Some(PanelApiAliasPoolSizeValue::Number(value)) = max_val {
        if *value == 0 {
            return info_err_res!("panel_api.alias_pool.size.max must be greater than 0");
        }
    }
    let min = min_val.and_then(PanelApiAliasPoolSizeValue::as_number);
    let max = max_val.and_then(PanelApiAliasPoolSizeValue::as_number);
    if let (Some(min), Some(max)) = (min, max) {
        if min > max {
            return info_err_res!("panel_api.alias_pool.size.min must be <= panel_api.alias_pool.size.max");
        }
    }
    if cfg.provisioning.probe_interval_sec == 0 {
        return info_err_res!("panel_api.provisioning.probe_interval_sec must be greater than 0");
    }
    if let Some(offset) = cfg.provisioning.offset.as_deref() {
        let _secs = parse_panel_api_provisioning_offset_secs(offset)?;
    }
    Ok(())
}

fn resolve_query_params(
    params: &[PanelApiQueryParamDto],
    api_key: Option<&str>,
    creds: Option<(&str, &str)>,
) -> Result<Vec<(String, String)>, TuliproxError> {
    let mut out = Vec::with_capacity(params.len());
    for p in params {
        let key = p.key.trim();
        if key.is_empty() {
            continue;
        }
        let mut value = p.value.trim().to_string();
        if value.eq_ignore_ascii_case("auto") {
            if key.eq_ignore_ascii_case("api_key") {
                let Some(k) = api_key.filter(|s| !s.trim().is_empty()) else {
                    return info_err_res!("panel_api: query param {key} uses 'auto' but panel_api.api_key is missing");
                };
                value = k.to_string();
            } else if key.eq_ignore_ascii_case("username") {
                let Some((u, _)) = creds else {
                    return info_err_res!("panel_api: query param {key} uses 'auto' but no account username is available");
                };
                value = u.to_string();
            } else if key.eq_ignore_ascii_case("password") {
                let Some((_, pw)) = creds else {
                    return info_err_res!("panel_api: query param {key} uses 'auto' but no account password is available");
                };
                value = pw.to_string();
            }
        }
        out.push((key.to_string(), value));
    }
    Ok(out)
}

fn build_panel_url(base_url: &str, query_params: &[(String, String)]) -> Result<Url, TuliproxError> {
    let mut url = Url::parse(base_url).map_err(|e| info_err!("panel_api: invalid url {base_url}: {e}"))?;
    {
        let mut pairs = url.query_pairs_mut();
        for (k, v) in query_params {
            pairs.append_pair(k, v);
        }
    }
    Ok(url)
}

fn sanitize_panel_api_json_for_log(value: &Value) -> Value {
    match value {
        Value::Array(arr) => Value::Array(arr.iter().map(sanitize_panel_api_json_for_log).collect()),
        Value::Object(obj) => {
            let mut out = serde_json::Map::with_capacity(obj.len());
            for (k, v) in obj {
                if k.eq_ignore_ascii_case("api_key") || k.eq_ignore_ascii_case("apikey") || k.eq_ignore_ascii_case("token") {
                    out.insert(k.clone(), Value::String("***".to_string()));
                    continue;
                }
                if k.eq_ignore_ascii_case("username") || k.eq_ignore_ascii_case("password") {
                    out.insert(k.clone(), Value::String("***".to_string()));
                    continue;
                }
                if k.eq_ignore_ascii_case("url") {
                    if let Some(s) = v.as_str() {
                        out.insert(k.clone(), Value::String(sanitize_sensitive_info(s).into_owned()));
                        continue;
                    }
                }
                out.insert(k.clone(), sanitize_panel_api_json_for_log(v));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

async fn panel_get_json(app_state: &AppState, url: Url) -> Result<Value, TuliproxError> {
    let client = app_state.http_client.load();
    let sanitized = sanitize_sensitive_info(url.as_str());
    debug_if_enabled!("panel_api request {}", sanitized);
    let resp = client
        .get(url)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e|info_err!("panel_api request failed: {e}"))?;
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| info_err!("panel_api read response failed: {e}"))?;
    let json: Value = serde_json::from_str(&body)
        .map_err(|e| info_err!("panel_api invalid json (http {status}): {e}"))?;
    let json_for_log = sanitize_panel_api_json_for_log(&json);
    if let Ok(json_str) = serde_json::to_string(&json_for_log) {
        debug_if_enabled!("panel_api response (http {}): {}", status, sanitize_sensitive_info(&json_str));
    }
    Ok(json)
}

async fn panel_client_new(app_state: &AppState, cfg: &PanelApiConfigDto) -> Result<(String, String, Option<String>), TuliproxError> {
    validate_client_new_params(&cfg.query_parameter.client_new)?;
    let params = resolve_query_params(&cfg.query_parameter.client_new, cfg.api_key.as_deref(), None)?;
    let url = build_panel_url(cfg.url.as_str(), &params)?;
    let json = panel_get_json(app_state, url).await?;
    let Some(obj) = first_json_object(&json) else {
        return info_err_res!("panel_api: client_new response is not a JSON object/array");
    };
    let status_ok = obj.get("status").is_some_and(parse_boolish);
    if !status_ok {
        return info_err_res!("panel_api: client_new status=false");
    }
    if let Some((u, p)) = extract_username_password_from_json(obj) {
        return Ok((u, p, None));
    }
    if let Some(url_str) = obj.get("url").and_then(|v| v.as_str()) {
        if let (Some(u), Some(p)) = get_credentials_from_url_str(url_str) {
            let base = get_base_url_from_str(url_str);
            return Ok((u, p, base));
        }
    }
    info_err_res!("panel_api: client_new response missing username/password (and no parsable url)")
}

async fn panel_client_renew(app_state: &AppState, cfg: &PanelApiConfigDto, username: &str, password: &str) -> Result<(), TuliproxError> {
    validate_client_renew_params(&cfg.query_parameter.client_renew)?;
    let params = resolve_query_params(
        &cfg.query_parameter.client_renew,
        cfg.api_key.as_deref(),
        Some((username, password)),
    )?;
    let url = build_panel_url(cfg.url.as_str(), &params)?;
    let json = panel_get_json(app_state, url).await?;
    let Some(obj) = first_json_object(&json) else {
        return info_err_res!("panel_api: client_renew response is not a JSON object/array");
    };
    let status_ok = obj.get("status").is_some_and(parse_boolish);
    if !status_ok {
        return info_err_res!("panel_api: client_renew status=false");
    }
    Ok(())
}

async fn panel_client_info(app_state: &AppState, cfg: &PanelApiConfigDto, username: &str, password: &str) -> Result<Option<i64>, TuliproxError> {
    validate_client_info_params(&cfg.query_parameter.client_info)?;
    let params = resolve_query_params(
        &cfg.query_parameter.client_info,
        cfg.api_key.as_deref(),
        Some((username, password)),
    )?;
    let url = build_panel_url(cfg.url.as_str(), &params)?;
    let json = panel_get_json(app_state, url).await?;
    let Some(obj) = first_json_object(&json) else {
        return info_err_res!("panel_api: client_info response is not a JSON object/array");
    };
    let status_ok = obj.get("status").is_some_and(parse_boolish);
    if !status_ok {
        return info_err_res!("panel_api: client_info status=false");
    }
    let expire = obj.get("expire").and_then(|v| v.as_str()).unwrap_or_default().trim();
    let parsed = parse_timestamp(expire).ok().flatten();
    if parsed.is_some() {
        return Ok(parsed);
    }

    // Some panels return only a date ("YYYY-MM-DD") without time. Normalize to midnight.
    if is_date_only_yyyy_mm_dd(expire) {
        let normalized = format!("{expire} 00:00:00");
        return Ok(parse_timestamp(&normalized).ok().flatten());
    }

    Ok(None)
}

async fn panel_account_info(
    app_state: &AppState,
    cfg: &PanelApiConfigDto,
    creds: Option<(&str, &str)>,
) -> Result<Option<String>, TuliproxError> {
    if cfg.query_parameter.account_info.is_empty() {
        return Ok(None);
    }
    validate_account_info_params(&cfg.query_parameter.account_info)?;
    let params = resolve_query_params(
        &cfg.query_parameter.account_info,
        cfg.api_key.as_deref(),
        creds,
    )?;
    let url = build_panel_url(cfg.url.as_str(), &params)?;
    let json = panel_get_json(app_state, url).await?;
    let Some(obj) = first_json_object(&json) else {
        return info_err_res!("panel_api: account_info response is not a JSON object/array");
    };
    let status_ok = obj.get("status").is_some_and(parse_boolish);
    if !status_ok {
        return info_err_res!("panel_api: account_info status=false");
    }
    let Some(credits) = obj.get("credits").and_then(extract_stringish) else {
        return info_err_res!("panel_api: account_info response missing credits");
    };
    Ok(Some(credits))
}

async fn panel_client_adult_content(
    app_state: &AppState,
    cfg: &PanelApiConfigDto,
    creds: Option<(&str, &str)>,
) -> Result<(), TuliproxError> {
    if cfg.query_parameter.client_adult_content.is_empty() {
        return Ok(());
    }
    validate_client_adult_content_params(&cfg.query_parameter.client_adult_content)?;
    let params = resolve_query_params(
        &cfg.query_parameter.client_adult_content,
        cfg.api_key.as_deref(),
        creds,
    )?;
    let url = build_panel_url(cfg.url.as_str(), &params)?;
    let json = panel_get_json(app_state, url).await?;
    let Some(obj) = first_json_object(&json) else {
        return info_err_res!("panel_api: client_adult_content response is not a JSON object/array");
    };
    let status_ok = obj.get("status").is_some_and(parse_boolish);
    if !status_ok {
        return info_err_res!("panel_api: client_adult_content status=false");
    }
    Ok(())
}

fn extract_account_creds_from_input(input: &ConfigInput) -> Option<(String, String)> {
    if let (Some(u), Some(p)) = (input.username.as_deref(), input.password.as_deref()) {
        if !u.trim().is_empty() && !p.trim().is_empty() {
            return Some((u.to_string(), p.to_string()));
        }
    }
    Url::parse(input.url.as_str()).ok().and_then(|u| {
        let (uu, pp) = get_credentials_from_url(&u);
        match (uu, pp) {
            (Some(uu), Some(pp)) if !uu.trim().is_empty() && !pp.trim().is_empty() => Some((uu, pp)),
            _ => None,
        }
    })
}


fn alias_pool_limit_values(cfg: &PanelApiConfigDto) -> (Option<&PanelApiAliasPoolSizeValue>, Option<&PanelApiAliasPoolSizeValue>) {
    let size = cfg.alias_pool.as_ref().and_then(|p| p.size.as_ref());
    let min = size.and_then(|s| s.min.as_ref());
    let max = size.and_then(|s| s.max.as_ref());
    (min, max)
}

#[allow(dead_code)]
fn alias_pool_both_auto(cfg: &PanelApiConfigDto) -> bool {
    let (min, max) = alias_pool_limit_values(cfg);
    min.is_some_and(PanelApiAliasPoolSizeValue::is_auto)
        && max.is_some_and(PanelApiAliasPoolSizeValue::is_auto)
}

fn resolve_alias_pool_limit_value(value: Option<&PanelApiAliasPoolSizeValue>, auto_value: Option<u16>) -> Option<u16> {
    match value {
        Some(PanelApiAliasPoolSizeValue::Number(v)) => Some(*v),
        Some(PanelApiAliasPoolSizeValue::Auto(_)) => auto_value,
        None => None,
    }
}

fn is_proxy_user_enabled(user: &ProxyUserCredentials) -> bool {
    if let Some(status) = user.status {
        if !matches!(status, ProxyUserStatus::Active | ProxyUserStatus::Trial) {
            return false;
        }
    }
    !is_input_expired(user.exp_date)
}

fn find_input_target_names(app_state: &AppState, input_name: &str) -> Vec<String> {
    let sources = app_state.app_config.sources.load();
    for source in &sources.sources {
        if source.inputs.iter().any(|name| name == input_name) {
            return source.targets.iter().map(|t| t.name.clone()).collect();
        }
    }
    vec![]
}

fn count_enabled_proxy_users(app_state: &AppState, input_name: &str) -> usize {
    let api_proxy_guard = app_state.app_config.api_proxy.load();
    let Some(api_proxy) = api_proxy_guard.as_ref() else {
        return 0;
    };
    let target_names = find_input_target_names(app_state, input_name);
    if target_names.is_empty() {
        return 0;
    }
    api_proxy
        .user
        .iter()
        .filter(|target_user| {
            target_names
                .iter()
                .any(|target| target.eq_ignore_ascii_case(&target_user.target))
        })
        .map(|target_user| {
            target_user
                .credentials
                .iter()
                .filter(|cred| is_proxy_user_enabled(cred))
                .count()
        })
        .sum()
}

fn resolve_alias_pool_auto_value(app_state: &AppState, input_name: &str) -> u16 {
    let enabled_users = count_enabled_proxy_users(app_state, input_name);
    u16::try_from(enabled_users).unwrap_or(u16::MAX)
}

#[allow(dead_code)]
pub(crate) fn target_has_alias_pool_auto(app_state: &AppState, target_name: &str) -> bool {
    let sources = app_state.app_config.sources.load();
    for source in &sources.sources {
        let target_match = source
            .targets
            .iter()
            .any(|target| target.name.eq_ignore_ascii_case(target_name));
        if !target_match {
            continue;
        }
        for input_name in &source.inputs {
            let Some(input) = sources.get_input_by_name(input_name) else {
                continue;
            };
            if let Some(panel_cfg) = input.panel_api.as_ref() {
                if !panel_cfg.enabled {
                    continue;
                }
                if alias_pool_both_auto(panel_cfg) {
                    return true;
                }
            }
        }
    }
    false
}

fn resolve_alias_pool_limits(
    app_state: &AppState,
    input_name: &str,
    cfg: &PanelApiConfigDto,
) -> Result<(Option<u16>, Option<u16>), TuliproxError> {
    let (min_val, max_val) = alias_pool_limit_values(cfg);
    if min_val.is_none() && max_val.is_none() {
        return Ok((None, None));
    }
    let min_auto = min_val.is_some_and(PanelApiAliasPoolSizeValue::is_auto);
    let auto_value = min_auto.then(|| resolve_alias_pool_auto_value(app_state, input_name));
    let min = resolve_alias_pool_limit_value(min_val, auto_value);
    let max = match max_val {
        Some(PanelApiAliasPoolSizeValue::Number(value)) => Some(*value),
        Some(PanelApiAliasPoolSizeValue::Auto(_)) | None => None,
    };
    if let (Some(min), Some(max)) = (min, max) {
        if min > max {
            return info_err_res!("panel_api.alias_pool.size.min must be <= panel_api.alias_pool.size.max"
            );
        }
    }
    Ok((min, max))
}

#[allow(dead_code)]
fn resolve_alias_pool_min(app_state: &AppState, input_name: &str, cfg: &PanelApiConfigDto) -> Option<u16> {
    let (min_val, _) = alias_pool_limit_values(cfg);
    let min_val = min_val?;
    let auto_value = min_val
        .is_auto()
        .then(|| resolve_alias_pool_auto_value(app_state, input_name));
    resolve_alias_pool_limit_value(Some(min_val), auto_value)
}

fn alias_pool_remove_expired(cfg: &PanelApiConfigDto) -> bool {
    cfg.alias_pool.as_ref().is_some_and(|p| p.remove_expired)
}

fn collect_accounts(input: &ConfigInput) -> Vec<AccountCredentials> {
    let mut out = Vec::new();
    if let Some((u, p)) = extract_account_creds_from_input(input) {
        out.push(AccountCredentials {
            name: input.name.clone(),
            username: u,
            password: p,
            exp_date: input.exp_date,
        });
    }
    if let Some(aliases) = input.aliases.as_ref() {
        for a in aliases {
            if let (Some(u), Some(p)) = (a.username.as_deref(), a.password.as_deref()) {
                if !u.trim().is_empty() && !p.trim().is_empty() {
                    out.push(AccountCredentials {
                        name: a.name.clone(),
                        username: u.to_string(),
                        password: p.to_string(),
                        exp_date: a.exp_date,
                    });
                }
            }
        }
    }
    out
}

fn count_valid_accounts(accounts: &[AccountCredentials]) -> usize {
    accounts.iter().filter(|acct| !is_input_expired(acct.exp_date)).count()
}

pub(crate) fn is_alias_pool_max_reached(app_state: &AppState, input: &ConfigInput) -> bool {
    let Some(panel_cfg) = input.panel_api.as_ref() else {
        return false;
    };
    if !panel_cfg.enabled {
        return false;
    }
    if panel_cfg.url.trim().is_empty() {
        return false;
    }
    if validate_panel_api_config(panel_cfg).is_err() {
        return false;
    }
    let Ok((_, max_pool)) = resolve_alias_pool_limits(app_state, &input.name, panel_cfg) else {
        return false;
    };
    if let Some(max_pool) = max_pool {
        let valid_count = count_valid_accounts(&collect_accounts(input));
        if valid_count >= max_pool as usize {
            debug_if_enabled!(
                "panel_api: alias_pool.size.max reached for input {} (valid_accounts={}, max={})",
                sanitize_sensitive_info(&input.name),
                valid_count,
                max_pool
            );
            return true;
        }
    }
    false
}

pub(crate) fn can_provision_on_exhausted(app_state: &AppState, input: &ConfigInput) -> bool {
    let Some(panel_cfg) = input.panel_api.as_ref() else {
        return false;
    };
    if !panel_cfg.enabled {
        return false;
    }
    if panel_cfg.url.trim().is_empty() {
        return false;
    }
    if let Err(err) = validate_panel_api_config(panel_cfg) {
        debug_if_enabled!("panel_api config invalid: {}", sanitize_sensitive_info(err.to_string().as_str()));
        return false;
    }
    if is_alias_pool_max_reached(app_state, input) {
        return false;
    }
    true
}

#[allow(dead_code)]
pub(crate) fn find_input_by_name(app_state: &AppState, input_name: &str) -> Option<Arc<ConfigInput>> {
    let sources = app_state.app_config.sources.load();
    sources.get_input_by_name(input_name).map(Arc::clone)
}

pub(crate) fn find_input_by_provider_name(app_state: &AppState, provider_name: &str) -> Option<Arc<ConfigInput>> {
    let sources = app_state.app_config.sources.load();
    for input in &sources.inputs {
        if input.name == provider_name {
            return Some(Arc::clone(input));
        }
        if input
            .aliases
            .as_ref()
            .is_some_and(|aliases| aliases.iter().any(|alias| alias.name == provider_name))
        {
            return Some(Arc::clone(input));
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
async fn patch_source_yml_add_alias(
    app_state: &Arc<AppState>,
    source_file_path: &Path,
    input_name: &str,
    alias_name: &str,
    base_url: &str,
    username: &str,
    password: &str,
    exp_date: Option<i64>,
) -> Result<(), TuliproxError> {
    let mut sources = match read_sources_file_from_path(source_file_path, false, false, None) {
        Ok(sources) => sources,
        Err(e) => return info_err_res!("panel_api: failed to read source file: {e}"),
    };

    let Some(input) = sources.inputs.iter_mut().find(|i| i.name == input_name) else {
        return info_err_res!("panel_api: could not find input '{input_name}' in source.yml");
    };

    let aliases = input.aliases.get_or_insert_with(Vec::new);
    let next_index = aliases.iter().map(|a| a.id).max().unwrap_or(0);
    if next_index == u16::MAX {
        return info_err_res!("panel_api: cannot add alias for '{input_name}': alias id overflow");
    }

    let mut alias = ConfigInputAliasDto {
        id: 0,
        name: alias_name.to_string(),
        url: base_url.to_string(),
        username: Some(username.to_string()),
        password: Some(password.to_string()),
        priority: 0,
        max_connections: 1,
        exp_date,
    };

    alias.prepare(next_index, &input.input_type)?;
    aliases.push(alias);

    persist_source_config(app_state, Some(source_file_path), sources).await?;
    Ok(())
}

async fn patch_source_yml_update_panel_api_credits(
    app_state: &Arc<AppState>,
    source_file_path: &Path,
    input_name: &str,
    credits: &str,
) -> Result<(), TuliproxError> {
    let mut sources = match read_sources_file_from_path(source_file_path, false, false, None) {
        Ok(sources) => sources,
        Err(e) => return info_err_res!("panel_api: failed to read source file: {e}"),
    };

    for input in &mut sources.inputs {
        if input.name == input_name {
            let Some(panel_api) = input.panel_api.as_mut() else {
                return info_err_res!("panel_api: could not find panel_api for input '{input_name}' in source.yml");
            };

            panel_api.credits = Some(credits.to_string());
            persist_source_config(app_state, Some(source_file_path), sources).await?;
            return Ok(());
        }
    }

    info_err_res!("panel_api: could not find input '{input_name}' in source.yml")
}

async fn patch_source_yml_update_exp_date(
    app_state: &Arc<AppState>,
    source_file_path: &Path,
    input_name: &str,
    account_name: &str,
    exp_date: i64,
) -> Result<(), TuliproxError> {
    let mut sources = match read_sources_file_from_path(source_file_path, false, false, None) {
        Ok(sources) => sources,
        Err(e) => return info_err_res!("panel_api: failed to read source file: {e}"),
    };

    let Some(input) = sources.inputs.iter_mut().find(|i| i.name == input_name) else {
        return info_err_res!("panel_api: could not find input '{input_name}' in source.yml");
    };

    if account_name == input_name {
        input.exp_date = Some(exp_date);
        input.enabled = true;
        input.max_connections = 1;
    } else if let Some(aliases) = input.aliases.as_mut() {
        let Some(alias) = aliases.iter_mut().find(|a| a.name == account_name) else {
            return info_err_res!(
                "panel_api: could not find alias '{account_name}' under input '{input_name}' in source.yml"
            );
        };
        alias.exp_date = Some(exp_date);
        alias.max_connections = 1;
    } else {
        return info_err_res!(
            "panel_api: input '{input_name}' has no aliases; cannot update exp_date for '{account_name}'"
        );
    }

    persist_source_config(app_state, Some(source_file_path), sources).await?;
    Ok(())
}

async fn patch_source_yml_update_root_credentials(
    app_state: &Arc<AppState>,
    source_file_path: &Path,
    input_name: &str,
    username: &str,
    password: &str,
    exp_date: Option<i64>,
) -> Result<(), TuliproxError> {
    let mut sources = match read_sources_file_from_path(source_file_path, false, false, None) {
        Ok(sources) => sources,
        Err(e) => return info_err_res!("panel_api: failed to read source file: {e}"),
    };

    let Some(input) = sources.inputs.iter_mut().find(|i| i.name == input_name) else {
        return info_err_res!("panel_api: could not find input '{input_name}' in source.yml");
    };

    input.username = Some(username.to_string());
    input.password = Some(password.to_string());
    input.enabled = true;
    input.max_connections = 1;
    if let Some(exp_date) = exp_date {
        input.exp_date = Some(exp_date);
    }

    if let Ok(mut url) = Url::parse(input.url.as_str()) {
        let mut pairs: Vec<(String, String)> = url
            .query_pairs()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let mut has_user = false;
        let mut has_pass = false;
        for (k, v) in &mut pairs {
            if k.eq_ignore_ascii_case("username") {
                *v = username.to_string();
                has_user = true;
            } else if k.eq_ignore_ascii_case("password") {
                *v = password.to_string();
                has_pass = true;
            }
        }
        if has_user || has_pass {
            if !has_user {
                pairs.push(("username".to_string(), username.to_string()));
            }
            if !has_pass {
                pairs.push(("password".to_string(), password.to_string()));
            }
            url.query_pairs_mut().clear();
            {
                let mut qp = url.query_pairs_mut();
                for (k, v) in pairs {
                    qp.append_pair(k.as_str(), v.as_str());
                }
            }
            input.url = url.to_string();
        }
    }

    persist_source_config(app_state, Some(source_file_path), sources).await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn patch_source_yml_update_alias_credentials(
    app_state: &Arc<AppState>,
    source_file_path: &Path,
    input_name: &str,
    alias_name: &str,
    username: &str,
    password: &str,
    exp_date: Option<i64>,
) -> Result<(), TuliproxError> {
    let mut sources = match read_sources_file_from_path(source_file_path, false, false, None) {
        Ok(sources) => sources,
        Err(e) => return info_err_res!("panel_api: failed to read source file: {e}"),
    };

    let Some(input) = sources.inputs.iter_mut().find(|i| i.name == input_name) else {
        return info_err_res!("panel_api: could not find input '{input_name}' in source.yml");
    };

    let Some(aliases) = input.aliases.as_mut() else {
        return info_err_res!("panel_api: input '{input_name}' has no aliases; cannot update credentials for '{alias_name}'");
    };
    let Some(alias) = aliases.iter_mut().find(|a| a.name == alias_name) else {
        return info_err_res!(
            "panel_api: could not find alias '{alias_name}' under input '{input_name}' in source.yml"
        );
    };

    alias.username = Some(username.to_string());
    alias.password = Some(password.to_string());
    alias.max_connections = 1;
    if let Some(exp_date) = exp_date {
        alias.exp_date = Some(exp_date);
    }

    if let Ok(mut url) = Url::parse(alias.url.as_str()) {
        let mut pairs: Vec<(String, String)> = url
            .query_pairs()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let mut has_user = false;
        let mut has_pass = false;
        for (k, v) in &mut pairs {
            if k.eq_ignore_ascii_case("username") {
                *v = username.to_string();
                has_user = true;
            } else if k.eq_ignore_ascii_case("password") {
                *v = password.to_string();
                has_pass = true;
            }
        }
        if has_user || has_pass {
            if !has_user {
                pairs.push(("username".to_string(), username.to_string()));
            }
            if !has_pass {
                pairs.push(("password".to_string(), password.to_string()));
            }
            url.query_pairs_mut().clear();
            {
                let mut qp = url.query_pairs_mut();
                for (k, v) in pairs {
                    qp.append_pair(k.as_str(), v.as_str());
                }
            }
            alias.url = url.to_string();
        }
    }

    persist_source_config(app_state, Some(source_file_path), sources).await?;
    Ok(())
}

async fn patch_source_yml_remove_expired_aliases(
    app_state: &Arc<AppState>,
    source_file_path: &Path,
    input_name: &str,
) -> Result<bool, TuliproxError> {
    let mut sources = match read_sources_file_from_path(source_file_path, false, false, None) {
        Ok(sources) => sources,
        Err(e) => return info_err_res!("panel_api: failed to read source file: {e}"),
    };

    let Some(input) = sources.inputs.iter_mut().find(|i| i.name == input_name) else {
        return info_err_res!("panel_api: could not find input '{input_name}' in source.yml");
    };

    if let Some(aliases) = input.aliases.as_mut() {
        let before_len = aliases.len();
        aliases.retain(|alias| !is_input_expired(alias.exp_date));
        if aliases.len() == before_len {
            return Ok(false);
        }
    }

    persist_source_config(app_state, Some(source_file_path), sources).await?;
    Ok(true)
}

const MAX_ALIAS_NAME_ATTEMPTS: usize = 1000;

fn derive_unique_alias_name(existing: &[String], input_name: &str, username: &str) -> String {
    let base = format!("{input_name}-{username}");
    if !existing.contains(&base) {
        return base;
    }
    for i in 2..MAX_ALIAS_NAME_ATTEMPTS {
        let cand = format!("{base}-{i}");
        if !existing.contains(&cand) {
            return cand;
        }
    }
    warn!("derive_unique_alias_name: exhausted {MAX_ALIAS_NAME_ATTEMPTS} attempts for base '{base}'; returning potentially duplicate name");
    base
}

#[derive(Debug, Clone)]
pub(crate) enum PanelApiProvisionOutcome {
    Renewed { username: String, password: String },
    Created { username: String, password: String },
}

impl PanelApiProvisionOutcome {
    pub(crate) fn credentials(&self) -> (&str, &str) {
        match self {
            Self::Renewed { username, password } | Self::Created { username, password } => {
                (username.as_str(), password.as_str())
            }
        }
    }

    pub(crate) fn kind_label(&self) -> &'static str {
        match self {
            Self::Renewed { .. } => "client_renew",
            Self::Created { .. } => "client_new",
        }
    }
}

async fn try_renew_expired_account(
    app_state: &Arc<AppState>,
    input: &ConfigInput,
    panel_cfg: &PanelApiConfigDto,
    is_batch: bool,
    sources_path: &Path,
    treat_missing_exp_date_as_expired: bool,
) -> Option<PanelApiProvisionOutcome> {
    let mut candidates = collect_accounts(input);
    for acct in &mut candidates {
        if treat_missing_exp_date_as_expired && acct.exp_date.is_none() {
            acct.exp_date = panel_client_info(app_state, panel_cfg, acct.username.as_str(), acct.password.as_str())
                .await
                .ok()
                .flatten();
        }
    }
    candidates.sort_by_key(|a| a.exp_date.unwrap_or(i64::MAX));

    for acct in &candidates {
        // Only attempt renew/new when the account is *known* to be expired.
        // If exp_date is missing (even after an optional client_info refresh), we skip renewal.
        let expired = is_input_expired(acct.exp_date);
        if !expired {
            continue;
        }
        match panel_client_renew(app_state, panel_cfg, acct.username.as_str(), acct.password.as_str()).await {
            Ok(()) => {
                if let Err(err) = panel_client_adult_content(
                    app_state,
                    panel_cfg,
                    Some((acct.username.as_str(), acct.password.as_str())),
                )
                .await
                {
                    debug_if_enabled!(
                        "panel_api client_adult_content failed for {}: {}",
                        sanitize_sensitive_info(&acct.name),
                        sanitize_sensitive_info(err.to_string().as_str())
                    );
                }
                let refreshed_exp = panel_client_info(app_state, panel_cfg, acct.username.as_str(), acct.password.as_str())
                    .await
                    .ok()
                    .flatten();

                if let Some(new_exp) = refreshed_exp.or(acct.exp_date) {
                    if is_batch {
                        let batch_url = input.t_batch_url.as_deref().unwrap_or_default();
                        if let Ok(csv_path) = get_csv_file_path(batch_url) {
                            let _csv_lock = app_state.app_config.file_locks.write_lock(&csv_path).await;
                            if let Err(err) = csv_patch_batch_update_exp_date(input.input_type, &csv_path, &acct.name, &acct.username, &acct.password, new_exp).await {
                                debug_if_enabled!("panel_api failed to persist renew exp_date to csv: {}", err);
                            }
                        }
                    } else {
                        let _src_lock = app_state.app_config.file_locks.write_lock(sources_path).await;
                        if let Err(err) = patch_source_yml_update_exp_date(app_state, sources_path, &input.name, &acct.name, new_exp).await {
                            debug_if_enabled!("panel_api failed to persist renew exp_date to source.yml: {}", err);
                        }
                    }
                }

                if let Err(err) = ConfigFile::load_sources(app_state).await {
                    debug_if_enabled!("panel_api reload sources failed: {}", err);
                }
                return Some(PanelApiProvisionOutcome::Renewed {
                    username: acct.username.clone(),
                    password: acct.password.clone(),
                });
            }
            Err(err) => {
                debug_if_enabled!(
                    "panel_api client_renew failed for {}: {}",
                    sanitize_sensitive_info(&acct.name),
                    sanitize_sensitive_info(err.to_string().as_str())
                );
            }
        }
    }
    None
}

async fn try_create_new_account(
    app_state: &Arc<AppState>,
    input: &ConfigInput,
    panel_cfg: &PanelApiConfigDto,
    is_batch: bool,
    sources_path: &Path,
) -> Option<PanelApiProvisionOutcome> {
    match panel_client_new(app_state, panel_cfg).await {
        Ok((username, password, base_url_from_resp)) => {
            let base_url = base_url_from_resp.unwrap_or_else(|| input.url.clone());
            let base_url = get_base_url_from_str(base_url.as_str()).unwrap_or_else(|| base_url.clone());

            let mut existing_names: Vec<String> = vec![input.name.clone()];
            if let Some(aliases) = input.aliases.as_ref() {
                existing_names.extend(aliases.iter().map(|a| a.name.clone()));
            }
            let alias_name = derive_unique_alias_name(&existing_names, &input.name, &username);

            if let Err(err) = panel_client_adult_content(app_state, panel_cfg, Some((&username, &password))).await {
                debug_if_enabled!(
                    "panel_api client_adult_content failed for {}: {}",
                    sanitize_sensitive_info(&alias_name),
                    sanitize_sensitive_info(err.to_string().as_str())
                );
            }

            let exp_date = panel_client_info(app_state, panel_cfg, &username, &password)
                .await
                .ok()
                .flatten();

            if is_batch {
                let batch_url = input.t_batch_url.as_deref().unwrap_or_default();
                match get_csv_file_path(batch_url) {
                    Ok(csv_path) => {
                        let batch_type = if input.input_type == InputType::Xtream {
                            InputType::XtreamBatch
                        } else {
                            InputType::M3uBatch
                        };
                        let _csv_lock = app_state.app_config.file_locks.write_lock(&csv_path).await;
                        if let Err(err) =
                            csv_patch_batch_append(&csv_path, batch_type, &alias_name, &base_url, &username, &password, exp_date).await
                        {
                            warn!("panel_api failed to append new account to csv: {err}");
                            return None;
                        }
                    }
                    Err(err) => {
                        warn!(
                            "panel_api cannot resolve batch csv path {}: {}",
                            sanitize_sensitive_info(batch_url),
                            err
                        );
                        return None;
                    }
                }
            } else {
                let _src_lock = app_state.app_config.file_locks.write_lock(sources_path).await;
                if let Err(err) =
                    patch_source_yml_add_alias(app_state, sources_path, &input.name, &alias_name, &base_url, &username, &password, exp_date).await
                {
                    warn!("panel_api failed to persist new alias to source.yml: {err}");
                    return None;
                }
            }

            if let Err(err) = ConfigFile::load_sources(app_state).await {
                error!("panel_api reload sources failed: {err}");
                return None;
            }
            Some(PanelApiProvisionOutcome::Created { username, password })
        }
        Err(err) => {
            debug_if_enabled!("panel_api client_new failed: {}", sanitize_sensitive_info(err.to_string().as_str()));
            None
        }
    }
}

pub async fn try_provision_account_on_exhausted(
    app_state: &Arc<AppState>,
    input: &ConfigInput,
) -> Option<PanelApiProvisionOutcome> {
    let Some(panel_cfg) = input.panel_api.as_ref() else {
        debug_if_enabled!("panel_api: skipped (no panel_api config) for input {}", sanitize_sensitive_info(&input.name));
        return None;
    };
    if !panel_cfg.enabled {
        debug_if_enabled!("panel_api: skipped (panel_api.enabled false) for input {}", sanitize_sensitive_info(&input.name));
        return None;
    }
    if panel_cfg.url.trim().is_empty() {
        debug_if_enabled!("panel_api: skipped (panel_api.url empty) for input {}", sanitize_sensitive_info(&input.name));
        return None;
    }

    let _input_lock = app_state
        .app_config
        .file_locks
        .write_lock_str(format!("panel_api:{}", input.name).as_str())
        .await;

    if let Err(err) = validate_panel_api_config(panel_cfg) {
        debug_if_enabled!("panel_api config invalid: {}", sanitize_sensitive_info(err.to_string().as_str()));
        return None;
    }
    if is_alias_pool_max_reached(app_state, input) {
        return None;
    }

    debug_if_enabled!(
        "panel_api: exhausted -> provisioning for input {} (aliases={})",
        sanitize_sensitive_info(&input.name),
        input.aliases.as_ref().map_or(0, Vec::len)
    );

    let is_batch = input.t_batch_url.as_ref().is_some_and(|u| !u.trim().is_empty());
    let sources_file_path = app_state.app_config.paths.load().sources_file_path.clone();
    let sources_path = PathBuf::from(&sources_file_path);

    if let Some(outcome) =
        try_renew_expired_account(app_state, input, panel_cfg, is_batch, sources_path.as_path(), true).await
    {
        debug_if_enabled!(
            "panel_api: provisioning succeeded via client_renew for input {}",
            sanitize_sensitive_info(&input.name)
        );
        return Some(outcome);
    }
    let created = try_create_new_account(app_state, input, panel_cfg, is_batch, sources_path.as_path()).await;
    debug_if_enabled!(
        "panel_api: provisioning via client_new for input {} => {}",
        sanitize_sensitive_info(&input.name),
        if created.is_some() { "success" } else { "failed" }
    );
    created
}

async fn ensure_alias_pool_min(
    app_state: &Arc<AppState>,
    input_name: &str,
    panel_cfg: &PanelApiConfigDto,
    min_pool: u16,
    sources_path: &Path,
) -> bool {
    if min_pool == 0 {
        return false;
    }

    let mut changed = false;
    let max_attempts = usize::from(min_pool).saturating_add(10);
    for _ in 0..max_attempts {
        let sources = app_state.app_config.sources.load();
        let Some(input) = sources.get_input_by_name(input_name) else {
            break;
        };

        if is_alias_pool_max_reached(app_state.as_ref(), input.as_ref()) {
            break;
        }

        let accounts = collect_accounts(input.as_ref());
        let current_valid = count_valid_accounts(&accounts);
        if current_valid >= min_pool as usize {
            break;
        }

        let is_batch = input.t_batch_url.as_ref().is_some_and(|u| !u.trim().is_empty());

        if try_renew_expired_account(app_state, input.as_ref(), panel_cfg, is_batch, sources_path, true)
            .await
            .is_some()
        {
            changed = true;
            continue;
        }

        if try_create_new_account(app_state, input.as_ref(), panel_cfg, is_batch, sources_path)
            .await
            .is_some()
        {
            changed = true;
            continue;
        }

        break;
    }

    changed
}

#[allow(clippy::too_many_lines)]
async fn sync_panel_api_for_input_on_boot(
    app_state: &Arc<AppState>,
    input: &Arc<ConfigInput>,
    sources_path: &Path,
) -> bool {
    let Some(panel_cfg) = input.panel_api.as_ref() else {
        return false;
    };
    if !panel_cfg.enabled || panel_cfg.url.trim().is_empty() {
        return false;
    }

    if let Err(err) = validate_panel_api_config(panel_cfg) {
        debug_if_enabled!(
            "panel_api boot sync skipped for {}: {}",
            sanitize_sensitive_info(&input.name),
            sanitize_sensitive_info(err.to_string().as_str())
        );
        return false;
    }

    let input_name = input.name.as_str();
    let _input_lock = app_state
        .app_config
        .file_locks
        .write_lock_str(format!("panel_api:{input_name}").as_str())
        .await;

    let mut any_change = false;
    let is_batch = input.t_batch_url.as_ref().is_some_and(|u| !u.trim().is_empty());
    let batch_url = input.t_batch_url.as_deref().unwrap_or_default();
    let csv_path = if is_batch { get_csv_file_path(batch_url).ok() } else { None };
    let mut input_changed = false;

    let mut accounts = collect_accounts(input.as_ref());

    if !panel_cfg.query_parameter.account_info.is_empty() {
        let creds = accounts.first().map(|acct| (acct.username.as_str(), acct.password.as_str()));
        match panel_account_info(app_state.as_ref(), panel_cfg, creds).await {
            Ok(Some(credits)) => {
                let normalized = credits.trim().to_string();
                if !normalized.is_empty()
                    && panel_cfg.credits.as_deref().map(str::trim) != Some(normalized.as_str())
                {
                    let _src_lock = app_state.app_config.file_locks.write_lock(sources_path).await;
                    if let Err(err) = patch_source_yml_update_panel_api_credits(
                        app_state,
                        sources_path,
                        &input.name,
                        normalized.as_str(),
                    )
                    .await
                    {
                        debug_if_enabled!("panel_api boot sync failed to persist credits to source.yml: {}", err);
                    } else {
                        any_change = true;
                        input_changed = true;
                    }
                }
            }
            Ok(None) => {}
            Err(err) => {
                debug_if_enabled!(
                    "panel_api account_info failed for {}: {}",
                    sanitize_sensitive_info(&input.name),
                    sanitize_sensitive_info(err.to_string().as_str())
                );
            }
        }
    }

    for acct in &mut accounts {
        let new_exp = match panel_client_info(app_state.as_ref(), panel_cfg, &acct.username, &acct.password).await {
            Ok(v) => v,
            Err(err) => {
                debug_if_enabled!(
                    "panel_api client_info failed for {}: {}",
                    sanitize_sensitive_info(&acct.name),
                    sanitize_sensitive_info(err.to_string().as_str())
                );
                None
            }
        };
        let Some(new_exp) = new_exp else { continue; };
        if acct.exp_date == Some(new_exp) {
            continue;
        }

        if let Some(csv_path) = csv_path.as_ref() {
            let _csv_lock = app_state.app_config.file_locks.write_lock(csv_path).await;
            if let Err(err) = csv_patch_batch_update_exp_date(
                input.input_type,
                csv_path,
                &acct.name,
                &acct.username,
                &acct.password,
                new_exp,
            )
            .await
            {
                debug_if_enabled!("panel_api boot sync failed to persist exp_date to csv: {}", err);
                continue;
            }
        } else {
            let _src_lock = app_state.app_config.file_locks.write_lock(sources_path).await;
            if let Err(err) =
                patch_source_yml_update_exp_date(app_state, sources_path, &input.name, &acct.name, new_exp).await
            {
                debug_if_enabled!("panel_api boot sync failed to persist exp_date to source.yml: {}", err);
                continue;
            }
        }
        acct.exp_date = Some(new_exp);
        any_change = true;
        input_changed = true;
    }

    // On boot/update, also try to renew the root input account (not only aliases),
    // so expired/missing exp_date root credentials don't keep the provider disabled.
    let offset_secs = panel_cfg
        .provisioning
        .offset
        .as_deref()
        .and_then(|v| parse_panel_api_provisioning_offset_secs(v).ok())
        .unwrap_or(0);
    {
        for acct in &mut accounts {
            let account_name = acct.name.clone();
            let old_username = acct.username.clone();
            let old_password = acct.password.clone();

            if acct.exp_date.is_none() {
                continue;
            }
            if !is_expiring_with_offset(acct.exp_date, offset_secs) {
                continue;
            }

            let is_root = account_name == input.name;
            debug_if_enabled!(
                "panel_api boot sync renewing account {} for input {} (exp_date={:?}, offset={}s)",
                sanitize_sensitive_info(&account_name),
                sanitize_sensitive_info(&input.name),
                acct.exp_date,
                offset_secs
            );

            let (active_username, active_password, creds_changed) =
                match panel_client_renew(app_state.as_ref(), panel_cfg, old_username.as_str(), old_password.as_str()).await {
                    Ok(()) => (old_username.clone(), old_password.clone(), false),
                    Err(err) => {
                        debug_if_enabled!(
                            "panel_api client_renew failed for {}: {}",
                            sanitize_sensitive_info(&account_name),
                            sanitize_sensitive_info(err.to_string().as_str())
                        );
                        match panel_client_new(app_state.as_ref(), panel_cfg).await {
                            Ok((new_username, new_password, _base_url_from_resp)) => {
                                if let Some(csv_path) = csv_path.as_ref() {
                                    let _csv_lock = app_state.app_config.file_locks.write_lock(csv_path).await;
                                    if let Err(err) = csv_patch_batch_update_credentials(
                                        input.input_type,
                                        csv_path,
                                        &account_name,
                                        &old_username,
                                        &old_password,
                                        &new_username,
                                        &new_password,
                                        None,
                                    )
                                    .await
                                    {
                                        debug_if_enabled!(
                                            "panel_api boot sync failed to persist credentials to csv for {}: {}",
                                            sanitize_sensitive_info(&account_name),
                                            err
                                        );
                                    } else {
                                        any_change = true;
                                        input_changed = true;
                                    }
                                } else if is_root {
                                    let _src_lock = app_state.app_config.file_locks.write_lock(sources_path).await;
                                    if let Err(err) = patch_source_yml_update_root_credentials(
                                        app_state,
                                        sources_path,
                                        &input.name,
                                        &new_username,
                                        &new_password,
                                        None,
                                    )
                                    .await
                                    {
                                        debug_if_enabled!(
                                            "panel_api boot sync failed to persist root credentials to source.yml: {}",
                                            err
                                        );
                                    } else {
                                        any_change = true;
                                        input_changed = true;
                                    }
                                } else {
                                    let _src_lock = app_state.app_config.file_locks.write_lock(sources_path).await;
                                    if let Err(err) = patch_source_yml_update_alias_credentials(
                                        app_state,
                                        sources_path,
                                        &input.name,
                                        &account_name,
                                        &new_username,
                                        &new_password,
                                        None,
                                    )
                                    .await
                                    {
                                        debug_if_enabled!(
                                            "panel_api boot sync failed to persist alias credentials to source.yml: {}",
                                            err
                                        );
                                    } else {
                                        any_change = true;
                                        input_changed = true;
                                    }
                                }

                                acct.username.clone_from(&new_username);
                                acct.password.clone_from(&new_password);
                                (new_username, new_password, true)
                            }
                            Err(err) => {
                                debug_if_enabled!(
                                    "panel_api client_new failed for input {}: {}",
                                    sanitize_sensitive_info(&input.name),
                                    sanitize_sensitive_info(err.to_string().as_str())
                                );
                                continue;
                            }
                        }
                    }
                };

            if let Err(err) = panel_client_adult_content(
                app_state.as_ref(),
                panel_cfg,
                Some((active_username.as_str(), active_password.as_str())),
            )
            .await
            {
                debug_if_enabled!(
                    "panel_api client_adult_content failed for {}: {}",
                    sanitize_sensitive_info(&account_name),
                    sanitize_sensitive_info(err.to_string().as_str())
                );
            }

            let ready = wait_for_panel_api_account_ready(
                app_state,
                input.as_ref(),
                panel_cfg,
                account_name.as_str(),
                active_username.as_str(),
                active_password.as_str(),
            )
            .await;
            if !ready {
                debug_if_enabled!(
                    "panel_api boot sync probe timeout for {}; skipping exp_date refresh",
                    sanitize_sensitive_info(&account_name)
                );
                continue;
            }

            let refreshed_exp = panel_client_info(app_state.as_ref(), panel_cfg, active_username.as_str(), active_password.as_str())
                .await
                .ok()
                .flatten();

            if let Some(new_exp) = refreshed_exp {
                if let Some(csv_path) = csv_path.as_ref() {
                    let _csv_lock = app_state.app_config.file_locks.write_lock(csv_path).await;
                    let result = if creds_changed {
                        csv_patch_batch_update_credentials(
                            input.input_type,
                            csv_path,
                            &account_name,
                            &old_username,
                            &old_password,
                            active_username.as_str(),
                            active_password.as_str(),
                            Some(new_exp),
                        )
                        .await
                    } else {
                        csv_patch_batch_update_exp_date(
                            input.input_type,
                            csv_path,
                            &account_name,
                            &old_username,
                            &old_password,
                            new_exp,
                        )
                        .await
                    };
                    if let Err(err) = result {
                        debug_if_enabled!(
                            "panel_api boot sync failed to persist exp_date to csv for {}: {}",
                            sanitize_sensitive_info(&account_name),
                            err
                        );
                    } else {
                        acct.exp_date = Some(new_exp);
                        any_change = true;
                        input_changed = true;
                    }
                } else {
                    let _src_lock = app_state.app_config.file_locks.write_lock(sources_path).await;
                    let res = if creds_changed {
                        if is_root {
                            patch_source_yml_update_root_credentials(
                                app_state,
                                sources_path,
                                &input.name,
                                active_username.as_str(),
                                active_password.as_str(),
                                Some(new_exp),
                            )
                            .await
                        } else {
                            patch_source_yml_update_alias_credentials(
                                app_state,
                                sources_path,
                                &input.name,
                                &account_name,
                                active_username.as_str(),
                                active_password.as_str(),
                                Some(new_exp),
                            )
                            .await
                        }
                    } else {
                        patch_source_yml_update_exp_date(app_state, sources_path, &input.name, &account_name, new_exp).await
                    };
                    if let Err(err) = res {
                        debug_if_enabled!(
                            "panel_api boot sync failed to persist exp_date to source.yml for {}: {}",
                            sanitize_sensitive_info(&account_name),
                            err
                        );
                    } else {
                        acct.exp_date = Some(new_exp);
                        any_change = true;
                        input_changed = true;
                    }
                }
            } else {
                debug_if_enabled!(
                    "panel_api boot sync renew/create succeeded but exp_date refresh failed for {}",
                    sanitize_sensitive_info(&account_name)
                );
            }
        }
    }

    if !panel_cfg.query_parameter.client_adult_content.is_empty() {
        for acct in &accounts {
            if let Err(err) = panel_client_adult_content(
                app_state.as_ref(),
                panel_cfg,
                Some((acct.username.as_str(), acct.password.as_str())),
            )
            .await
            {
                debug_if_enabled!(
                    "panel_api client_adult_content failed for {}: {}",
                    sanitize_sensitive_info(&acct.name),
                    sanitize_sensitive_info(err.to_string().as_str())
                );
            }
        }
    }

    let min_pool = resolve_alias_pool_min(app_state.as_ref(), &input.name, panel_cfg);
    if let Some(min_pool_value) = min_pool {
        if alias_pool_both_auto(panel_cfg) {
            let enabled_users = count_enabled_proxy_users(app_state.as_ref(), &input.name);
            let current_valid = count_valid_accounts(&accounts);
            let current_valid_u16 = u16::try_from(current_valid).unwrap_or(u16::MAX);
            let needed = min_pool_value.saturating_sub(current_valid_u16);
            debug_if_enabled!(
                "panel_api boot/update alias pool auto for input {}: enabled_users={}, valid_accounts={}, to_provision={}",
                sanitize_sensitive_info(&input.name),
                enabled_users,
                current_valid,
                needed
            );
        }
    }
    let min_pool = min_pool.filter(|m| *m > 0);
    if let Some(min_pool) = min_pool {
        if input_changed {
            if let Err(err) = ConfigFile::load_sources(app_state).await {
                debug_if_enabled!(
                    "panel_api boot sync reload sources failed before alias pool min: {}",
                    err
                );
            }
        }
        if ensure_alias_pool_min(app_state, &input.name, panel_cfg, min_pool, sources_path).await {
            any_change = true;
        }
    }

    if alias_pool_remove_expired(panel_cfg) {
        if let Some(csv_path) = csv_path.as_ref() {
            let _csv_lock = app_state.app_config.file_locks.write_lock(csv_path).await;
            match csv_patch_batch_remove_expired(input.input_type, csv_path).await {
                Ok(true) => {
                    any_change = true;
                }
                Ok(false) => {}
                Err(err) => debug_if_enabled!("panel_api boot sync failed to remove expired csv accounts: {}", err),
            }
        } else {
            let _src_lock = app_state.app_config.file_locks.write_lock(sources_path).await;
            match patch_source_yml_remove_expired_aliases(app_state, sources_path, &input.name).await {
                Ok(true) => {
                    any_change = true;
                }
                Ok(false) => {}
                Err(err) => debug_if_enabled!("panel_api boot sync failed to remove expired source accounts: {}", err),
            }
        }
    }

    any_change
}

pub(crate) async fn sync_panel_api_exp_dates_on_boot(app_state: &Arc<AppState>) {
    let sources_file_path = app_state.app_config.paths.load().sources_file_path.clone();
    let sources_path = PathBuf::from(&sources_file_path);
    let mut any_change = false;

    let sources = app_state.app_config.sources.load();
    for input in &sources.inputs {
        if sync_panel_api_for_input_on_boot(app_state, input, sources_path.as_path()).await {
            any_change = true;
        }
    }

    if any_change {
        if let Err(err) = ConfigFile::load_sources(app_state).await {
            debug_if_enabled!("panel_api boot sync reload sources failed: {}", err);
        }
    }
}

pub(crate) async fn sync_panel_api_alias_pool_for_target(app_state: &Arc<AppState>, target_name: &str) {
    let sources_file_path = app_state.app_config.paths.load().sources_file_path.clone();
    let sources_path = PathBuf::from(&sources_file_path);
    let mut any_change = false;

    let sources = app_state.app_config.sources.load();
    for source in &sources.sources {
        let target_match = source
            .targets
            .iter()
            .any(|target| target.name.eq_ignore_ascii_case(target_name));
        if !target_match {
            continue;
        }

        for input_name in &source.inputs {
            let Some(input) = sources.get_input_by_name(input_name) else {
                continue;
            };
            let Some(panel_cfg) = input.panel_api.as_ref() else {
                continue;
            };
            if !panel_cfg.enabled || panel_cfg.url.trim().is_empty() {
                continue;
            }
            if !alias_pool_both_auto(panel_cfg) {
                continue;
            }
            if let Err(err) = validate_panel_api_config(panel_cfg) {
                debug_if_enabled!(
                    "panel_api user sync skipped for {}: {}",
                    sanitize_sensitive_info(&input.name),
                    sanitize_sensitive_info(err.to_string().as_str())
                );
                continue;
            }

            if sync_panel_api_for_input_on_boot(app_state, input, sources_path.as_path()).await {
                any_change = true;
            }
        }
    }

    if any_change {
        if let Err(err) = ConfigFile::load_sources(app_state).await {
            debug_if_enabled!("panel_api user sync reload sources failed: {}", err);
        }
    }
}


fn provisioning_method_to_reqwest(method: PanelApiProvisioningMethod) -> Method {
    match method {
        PanelApiProvisioningMethod::Head => Method::HEAD,
        PanelApiProvisioningMethod::Get => Method::GET,
        PanelApiProvisioningMethod::Post => Method::POST,
    }
}

fn build_panel_api_test_url(base_url: &str, username: &str, password: &str) -> Option<Url> {
    let url = Url::parse(base_url).ok()?;
    let host = url.host_str()?;
    let scheme = url.scheme();
    let mut base = concat_string!(scheme, "://", host);
    if let Some(port) = url.port() {
        base.push(':');
        base.push_str(port.to_string().as_str());
    }
    base.push_str("/player_api.php");
    let mut test_url = Url::parse(&base).ok()?;
    test_url
        .query_pairs_mut()
        .append_pair("username", username)
        .append_pair("password", password)
        .append_pair("action", "account_info");
    Some(test_url)
}

async fn probe_panel_api_test_url(
    app_state: &Arc<AppState>,
    test_url: &Url,
    method: PanelApiProvisioningMethod,
) -> Result<StatusCode, reqwest::Error> {
    let client = app_state.http_client.load();
    let request_method = provisioning_method_to_reqwest(method);
    let response = client.request(request_method, test_url.clone()).send().await?;
    Ok(response.status())
}

async fn wait_for_panel_api_account_ready(
    app_state: &Arc<AppState>,
    input: &ConfigInput,
    panel_cfg: &PanelApiConfigDto,
    account_name: &str,
    username: &str,
    password: &str,
) -> bool {
    let max_wait_secs = panel_cfg.provisioning.timeout_sec;
    let probe_interval_secs = panel_cfg.provisioning.probe_interval_sec.max(1);
    let probe_method = panel_cfg.provisioning.method;

    let Some(test_url) = build_panel_api_test_url(input.url.as_str(), username, password) else {
        return false;
    };

    debug_if_enabled!(
        "panel_api probe start for {} (input={} timeout={}s interval={}s method={}) url={}",
        sanitize_sensitive_info(account_name),
        sanitize_sensitive_info(&input.name),
        max_wait_secs,
        probe_interval_secs,
        probe_method,
        sanitize_sensitive_info(test_url.as_str())
    );

    let deadline = Instant::now() + Duration::from_secs(max_wait_secs);
    let probe_delay = Duration::from_secs(probe_interval_secs);
    let mut attempt = 0u64;
    loop {
        attempt += 1;
        match probe_panel_api_test_url(app_state, &test_url, probe_method).await {
            Ok(status) => {
                debug_if_enabled!(
                    "panel_api probe status: '{}' url: {} attempt={}",
                    status,
                    sanitize_sensitive_info(test_url.as_str()),
                    attempt
                );
                if status.is_success() {
                    return true;
                }
            }
            Err(err) => {
                if err.is_timeout() {
                    debug_if_enabled!(
                        "panel_api probe timeout for {} attempt={}",
                        sanitize_sensitive_info(test_url.as_str()),
                        attempt
                    );
                } else {
                    debug_if_enabled!(
                        "panel_api probe failed for {} attempt={}: {err}",
                        sanitize_sensitive_info(test_url.as_str()),
                        attempt
                    );
                }
            }
        }

        if max_wait_secs == 0 {
            return false;
        }
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        let remaining = deadline.checked_duration_since(now).unwrap_or_default();
        let sleep_for = if remaining < probe_delay { remaining } else { probe_delay };
        tokio::time::sleep(sleep_for).await;
    }
}

#[allow(clippy::too_many_lines)]
pub(crate) async fn run_panel_api_provisioning_probe(
    app_state: Arc<AppState>,
    input: ConfigInput,
    stop_signal: Arc<AtomicOnceFlag>,
    addr: SocketAddr,
    virtual_id: VirtualId,
) {
    let Some(panel_cfg) = input.panel_api.as_ref() else {
        debug_if_enabled!("panel_api provisioning probe skipped (missing config) for input {}", sanitize_sensitive_info(&input.name));
        stop_signal.notify();
        let _ = app_state.connection_manager.kick_connection(&addr, virtual_id, 0).await;
        return;
    };
    if !panel_cfg.enabled {
        debug_if_enabled!("panel_api provisioning probe skipped (panel_api.enabled false) for input {}", sanitize_sensitive_info(&input.name));
        stop_signal.notify();
        let _ = app_state.connection_manager.kick_connection(&addr, virtual_id, 0).await;
        return;
    }
    if panel_cfg.url.trim().is_empty() {
        debug_if_enabled!("panel_api provisioning probe skipped (panel_api.url empty) for input {}", sanitize_sensitive_info(&input.name));
        stop_signal.notify();
        let _ = app_state.connection_manager.kick_connection(&addr, virtual_id, 0).await;
        return;
    }

    let max_wait_secs = panel_cfg.provisioning.timeout_sec;
    let probe_interval_secs = panel_cfg.provisioning.probe_interval_sec.max(1);
    let probe_method = panel_cfg.provisioning.method;

    debug_if_enabled!(
        "panel_api provisioning probe start for input {} (timeout={}s interval={}s method={})",
        sanitize_sensitive_info(&input.name),
        max_wait_secs,
        probe_interval_secs,
        probe_method
    );

    let deadline = Instant::now() + Duration::from_secs(max_wait_secs);
    let outcome = try_provision_account_on_exhausted(&app_state, &input).await;
    let credentials = outcome.as_ref().map(PanelApiProvisionOutcome::credentials);

    if let Some(outcome) = outcome.as_ref() {
        debug_if_enabled!(
            "panel_api provisioning {} completed for input {}",
            outcome.kind_label(),
            sanitize_sensitive_info(&input.name)
        );
    } else {
        debug_if_enabled!("panel_api provisioning failed for input {}; waiting for timeout", sanitize_sensitive_info(&input.name));
    }

    let Some((username, password)) = credentials else {
        if max_wait_secs > 0 {
            tokio::time::sleep(Duration::from_secs(max_wait_secs)).await;
        }
        debug_if_enabled!("panel_api provisioning probe timeout reached for input {} (no credentials)", sanitize_sensitive_info(&input.name));
        stop_signal.notify();
        debug_if_enabled!(
            "panel_api provisioning closing client connection for input {} addr={}",
            sanitize_sensitive_info(&input.name),
            sanitize_sensitive_info(addr.to_string().as_str())
        );
        let _ = app_state.connection_manager.kick_connection(&addr, virtual_id, 0).await;
        return;
    };

    let Some(test_url) = build_panel_api_test_url(input.url.as_str(), username, password) else {
        if max_wait_secs > 0 {
            tokio::time::sleep(Duration::from_secs(max_wait_secs)).await;
        }
        debug_if_enabled!("panel_api provisioning probe failed to build test url for input {}", sanitize_sensitive_info(&input.name) );
        stop_signal.notify();
        let _ = app_state.connection_manager.kick_connection(&addr, virtual_id, 0).await;
        return;
    };

    let probe_delay = Duration::from_secs(probe_interval_secs);
    let mut attempt = 0u64;
    let mut ready = false;
    while Instant::now() < deadline {
        attempt += 1;
        match probe_panel_api_test_url(&app_state, &test_url, probe_method).await {
            Ok(status) => {
                debug_if_enabled!(
                    "panel_api provisioning probe status: '{}' url: {} attempt={}",
                    status,
                    sanitize_sensitive_info(test_url.as_str()),
                    attempt
                );
                if status.is_success() {
                    ready = true;
                    break;
                }
            }
            Err(err) => {
                if err.is_timeout() {
                    debug_if_enabled!(
                        "panel_api provisioning probe timeout for {} attempt={}",
                        sanitize_sensitive_info(test_url.as_str()),
                        attempt
                    );
                } else {
                    debug_if_enabled!(
                        "panel_api provisioning probe failed for {} attempt={}: {err}",
                        sanitize_sensitive_info(test_url.as_str()),
                        attempt
                    );
                }
            }
        }

        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let remaining = deadline.checked_duration_since(now).unwrap_or_default();
        let sleep_for = if remaining < probe_delay { remaining } else { probe_delay };
        tokio::time::sleep(sleep_for).await;
    }

    if ready {
        debug_if_enabled!(
            "panel_api provisioning ready for input {} (attempts={})",
            sanitize_sensitive_info(&input.name),
            attempt
        );
    } else {
        debug_if_enabled!(
            "panel_api provisioning probe timeout reached for input {} (attempts={})",
            sanitize_sensitive_info(&input.name),
            attempt
        );
    }
    stop_signal.notify();
    debug_if_enabled!(
        "panel_api provisioning closing client connection for input {} addr={}",
        sanitize_sensitive_info(&input.name),
        sanitize_sensitive_info(addr.to_string().as_str())
    );
    let _ = app_state
        .connection_manager
        .kick_connection(&addr, virtual_id, 0)
        .await;
}

pub fn create_panel_api_provisioning_stream_details(
    app_state: &Arc<AppState>,
    input: &ConfigInput,
    provider_name: Option<String>,
    grace_period_millis: u64,
    addr: SocketAddr,
    virtual_id: VirtualId,
) -> StreamDetails {
    let stop_signal = Arc::new(AtomicOnceFlag::new());
    let headers = [("connection".to_string(), "close".to_string())];
    let (stream, stream_info) = create_panel_api_provisioning_stream_with_stop(
        &app_state.app_config,
        &headers,
        Arc::clone(&stop_signal),
    );

    if stream.is_none() {
        debug_if_enabled!("panel_api provisioning stream missing; falling back to provider exhausted for input {}", sanitize_sensitive_info(&input.name));
        let (stream, stream_info) = create_provider_connections_exhausted_stream(&app_state.app_config, &[]);
        return StreamDetails {
            stream,
            stream_info,
            provider_name,
            grace_period_millis,
            disable_provider_grace: true,
            reconnect_flag: None,
            provider_handle: None,
        };
    }

    let app_state_clone = Arc::clone(app_state);
    let input_clone = input.clone();
    let stop_clone = Arc::clone(&stop_signal);
    tokio::spawn(async move {
        run_panel_api_provisioning_probe(app_state_clone, input_clone, stop_clone, addr, virtual_id).await;
    });

    StreamDetails {
        stream,
        stream_info,
        provider_name,
        grace_period_millis,
        disable_provider_grace: true,
        reconnect_flag: None,
        provider_handle: None,
    }
}
