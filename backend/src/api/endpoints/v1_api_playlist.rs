use std::sync::Arc;
use axum::response::IntoResponse;
use axum::Router;
use log::error;
use serde_json::json;
use shared::model::{InputType, PlaylistEpgRequest, PlaylistRequest, PlaylistRequestType, WebplayerUrlRequest};
use shared::utils::sanitize_sensitive_info;
use crate::api::endpoints::api_playlist_utils::{get_playlist, get_playlist_for_target};
use crate::api::model::AppState;
use crate::auth::create_access_token;
use crate::model::{parse_xmltv_for_web_ui, ConfigInput, ConfigInputOptions};
use crate::processing::processor::playlist;

fn create_config_input_for_m3u(url: &str) -> ConfigInput {
    ConfigInput {
        id: 0,
        name: String::from("m3u_req"),
        input_type: InputType::M3u,
        url: String::from(url),
        enabled: true,
        options: Some(ConfigInputOptions {
            xtream_skip_live: false,
            xtream_skip_vod: false,
            xtream_skip_series: false,
            xtream_live_stream_without_extension: false,
            xtream_live_stream_use_prefix: true,
        }),
        ..Default::default()
    }
}

fn create_config_input_for_xtream(username: &str, password: &str, host: &str) -> ConfigInput {
    ConfigInput {
        id: 0,
        name: String::from("xc_req"),
        input_type: InputType::Xtream,
        url: String::from(host),
        username: Some(String::from(username)),
        password: Some(String::from(password)),
        enabled: true,
        options: Some(ConfigInputOptions {
            xtream_skip_live: false,
            xtream_skip_vod: false,
            xtream_skip_series: false,
            xtream_live_stream_without_extension: false,
            xtream_live_stream_use_prefix: true,
        }),
        ..Default::default()
    }
}

async fn playlist_update(
    axum::extract::State(app_state): axum::extract::State<Arc<AppState>>,
    axum::extract::Json(targets): axum::extract::Json<Vec<String>>,
) -> impl axum::response::IntoResponse + Send {
    let user_targets = if targets.is_empty() { None } else { Some(targets) };
    let process_targets = app_state.app_config.sources.load().validate_targets(user_targets.as_ref());
    match process_targets {
        Ok(valid_targets) => {
            let app_config = Arc::clone(&app_state.app_config);
            let event_manager = Arc::clone(&app_state.event_manager);
            tokio::spawn(playlist::exec_processing(Arc::clone(&app_state.http_client.load()), app_config, Arc::new(valid_targets), Some(event_manager)));
            axum::http::StatusCode::OK.into_response()
        }
        Err(err) => {
            error!("Failed playlist update {}", sanitize_sensitive_info(err.to_string().as_str()));
            (axum::http::StatusCode::BAD_REQUEST, axum::Json(json!({"error": err.to_string()}))).into_response()
        }
    }
}


async fn playlist_content(
    axum::extract::State(app_state): axum::extract::State<Arc<AppState>>,
    axum::extract::Json(playlist_req): axum::extract::Json<PlaylistRequest>,
) -> impl IntoResponse + Send {
    let config = app_state.app_config.config.load();
    match playlist_req.rtype {
        PlaylistRequestType::Input => {
            if let Some(source_id) = playlist_req.source_id {
                get_playlist(Arc::clone(&app_state.http_client.load()), app_state.app_config.get_input_by_id(source_id).as_deref(), &config).await.into_response()
            } else {
                (axum::http::StatusCode::BAD_REQUEST, axum::Json(json!({"error": "Invalid input"}))).into_response()
            }
        }
        PlaylistRequestType::Target => {
            if let Some(source_id) = playlist_req.source_id {
                get_playlist_for_target(app_state.app_config.get_target_by_id(source_id).as_deref(), &app_state.app_config).await.into_response()
            } else {
                (axum::http::StatusCode::BAD_REQUEST, axum::Json(json!({"error": "Invalid target"}))).into_response()
            }
        }
        PlaylistRequestType::Xtream => {
            if let (Some(url), Some(username), Some(password)) = (playlist_req.url.as_ref(), playlist_req.username.as_ref(), playlist_req.password.as_ref()) {
                let input = create_config_input_for_xtream(username, password, url);
                get_playlist(Arc::clone(&app_state.http_client.load()), Some(&input), &config).await.into_response()
            } else {
                (axum::http::StatusCode::BAD_REQUEST, axum::Json(json!({"error": "Invalid url"}))).into_response()
            }
        }
        PlaylistRequestType::M3U => {
            if let Some(url) = playlist_req.url.as_ref() {
                let input = create_config_input_for_m3u(url);
                get_playlist(Arc::clone(&app_state.http_client.load()), Some(&input), &config).await.into_response()
            } else {
                (axum::http::StatusCode::BAD_REQUEST, axum::Json(json!({"error": "Invalid url"}))).into_response()
            }
        }
    }
}

async fn playlist_webplayer(
    axum::extract::State(app_state): axum::extract::State<Arc<AppState>>,
    axum::extract::Json(playlist_item): axum::extract::Json<WebplayerUrlRequest>,
) -> impl axum::response::IntoResponse + Send {
    let access_token = create_access_token(&app_state.app_config.access_token_secret, 30);
    let config = app_state.app_config.config.load();
    let server_name = config.web_ui.as_ref().and_then(|web_ui| web_ui.player_server.as_ref()).map_or("default", |server_name| server_name.as_str());
    let server_info = app_state.app_config.get_server_info(server_name);
    let base_url = server_info.get_base_url();
    format!("{base_url}/token/{access_token}/{}/{}/{}", playlist_item.target_id, playlist_item.cluster.as_stream_type(), playlist_item.virtual_id).into_response()
}

async fn playlist_epg(
    axum::extract::State(app_state): axum::extract::State<Arc<AppState>>,
    axum::extract::Json(playlist_epg_req): axum::extract::Json<PlaylistEpgRequest>,
) -> impl IntoResponse + Send {
    if let Some(target) = app_state.app_config.get_target_by_id(playlist_epg_req.target_id) {
        let config = &app_state.app_config.config.load();
        if let Some(epg_path) = crate::api::endpoints::xmltv_api::get_epg_path_for_target(config, &target)  {
           if let Ok(epg) = parse_xmltv_for_web_ui(&epg_path) {
               return (axum::http::StatusCode::OK, axum::Json(epg)).into_response();
           }
        }
    }
    axum::http::StatusCode::NO_CONTENT.into_response()
}

pub fn v1_api_playlist_register(router: Router<Arc<AppState>>) -> axum::Router<Arc<AppState>> {
    router
        .route("/playlist/webplayer", axum::routing::post(playlist_webplayer))
        .route("/playlist/update", axum::routing::post(playlist_update))
        .route("/playlist/epg", axum::routing::post(playlist_epg))
        .route("/playlist", axum::routing::post(playlist_content))
}
