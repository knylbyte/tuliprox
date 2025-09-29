use crate::api::model::AppState;
use crate::model::{ApiProxyConfig, InputSource};
use axum::response::IntoResponse;
use axum::Router;
use serde_json::json;
use shared::model::{ApiProxyConfigDto, ApiProxyServerInfoDto, ConfigDto};
use std::sync::Arc;
use log::error;
use shared::error::TuliproxError;
use crate::api::api_utils::try_unwrap_body;
use crate::{utils};
use crate::utils::{prepare_sources_batch, prepare_users};
use crate::utils::request::download_text_content;

pub(in crate::api::endpoints) fn intern_save_config_api_proxy(backup_dir: &str, api_proxy: &ApiProxyConfigDto, file_path: &str) -> Option<TuliproxError> {
    match utils::save_api_proxy(file_path, backup_dir, api_proxy) {
        Ok(()) => {}
        Err(err) => {
            error!("Failed to save api_proxy.yml {err}");
            return Some(err);
        }
    }
    None
}

fn intern_save_config_main(file_path: &str, backup_dir: &str, cfg: &ConfigDto) -> Option<TuliproxError> {
    match utils::save_main_config(file_path, backup_dir, cfg) {
        Ok(()) => {}
        Err(err) => {
            error!("Failed to save config.yml {err}");
            return Some(err);
        }
    }
    None
}

async fn save_config_main(
    axum::extract::State(app_state): axum::extract::State<Arc<AppState>>,
    axum::extract::Json(cfg): axum::extract::Json<ConfigDto>,
) -> impl axum::response::IntoResponse + Send {
    if cfg.is_valid() {
        let paths = app_state.app_config.paths.load();
        let file_path = paths.config_file_path.as_str();
        let config = app_state.app_config.config.load();
        let backup_dir = config.get_backup_dir();
        if let Some(err) = intern_save_config_main(file_path, backup_dir.as_ref(), &cfg) {
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, axum::Json(json!({"error": err.to_string()}))).into_response();
        }
        axum::http::StatusCode::OK.into_response()
    } else {
        (axum::http::StatusCode::BAD_REQUEST, axum::Json(json!({"error": "Invalid content"}))).into_response()
    }
}

async fn save_config_api_proxy_config(
    axum::extract::State(app_state): axum::extract::State<Arc<AppState>>,
    axum::extract::Json(mut req_api_proxy): axum::extract::Json<Vec<ApiProxyServerInfoDto>>,
) -> impl axum::response::IntoResponse + Send {
    for server_info in &mut req_api_proxy {
        if !server_info.validate() {
            return (axum::http::StatusCode::BAD_REQUEST, axum::Json(json!({"error": "Invalid content"}))).into_response();
        }
    }

    // TODO if hot reload is on, it is loaded twice, avoid this
    let mut api_proxy =  if let Some(old_api_proxy) = app_state.app_config.api_proxy.load().clone() {
        (*old_api_proxy).clone()
    } else {
       ApiProxyConfig::default()
    };
    api_proxy.server = req_api_proxy.iter().map(Into::into).collect();
    let new_api_proxy = Arc::new(api_proxy);
    app_state.app_config.api_proxy.store(Some(Arc::clone(&new_api_proxy)));
    let config = app_state.app_config.config.load();
    let backup_dir = config.get_backup_dir();
    let paths = app_state.app_config.paths.load();
    if let Some(err) = intern_save_config_api_proxy(backup_dir.as_ref(), &ApiProxyConfigDto::from(new_api_proxy.as_ref()), paths.api_proxy_file_path.as_str()) {
        return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, axum::Json(json!({"error": err.to_string()}))).into_response();
    }
    axum::http::StatusCode::OK.into_response()
}

async fn config(
    axum::extract::State(app_state): axum::extract::State<Arc<AppState>>,
) -> impl axum::response::IntoResponse + Send {
    let paths = app_state.app_config.paths.load();
    match utils::read_app_config_dto(&paths, true, false) {
        Ok(mut app_config) => {
            if let Err(err) = prepare_sources_batch(&mut app_config.sources, false) {
                error!("Failed to prepare sources batch: {err}");
                axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
            } else if let Err(err) = prepare_users(&mut app_config, &app_state.app_config) {
                error!("Failed to prepare users: {err}");
                axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
            } else {
                axum::response::Json(app_config).into_response()
            }
        }
        Err(err) => {
            error!("Failed to read config files: {err}");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn config_batch_content(
    axum::extract::Path(input_id): axum::extract::Path<u16>,
    axum::extract::State(app_state): axum::extract::State<Arc<AppState>>,
) -> impl axum::response::IntoResponse + Send {
    if let Some(config_input) = app_state.app_config.get_input_by_id(input_id) {
        // The url is changed at this point, we need the raw url for the batch file
        if let Some(batch_url) = config_input.t_batch_url.as_ref() {
            let input_source = InputSource::from(&*config_input).with_url(batch_url.to_owned());
            return match download_text_content(Arc::clone(&app_state.http_client.load()), &input_source, None).await {
                Ok((content, _path)) => {
                    // Return CSV with explicit content-type
                    try_unwrap_body!(axum::response::Response::builder()
                        .status(axum::http::StatusCode::OK)
                        .header(axum::http::header::CONTENT_TYPE, "text/csv; charset=utf-8")
                        .body(content))
                }
                Err(err) => {
                    error!("Failed to read batch file: {err}");
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
                }
            };
        }
    }
    (axum::http::StatusCode::NOT_FOUND, axum::Json(json!({"error": "Input not found or batch URL missing"}))).into_response()
}


pub fn v1_api_config_register(router: Router<Arc<AppState>>) -> axum::Router<Arc<AppState>> {
    router
        .route("/config", axum::routing::get(config))
        .route("/config/batchContent/{input_id}", axum::routing::get(config_batch_content))
        .route("/config/main", axum::routing::post(save_config_main))
        .route("/config/apiproxy", axum::routing::post(save_config_api_proxy_config))
}
