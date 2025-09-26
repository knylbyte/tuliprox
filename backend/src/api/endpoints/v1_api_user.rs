use crate::api::model::AppState;
use crate::model::ProxyUserCredentials;
use crate::repository::user_repository::store_api_user;
use axum::response::IntoResponse;
use axum::Router;
use serde_json::json;
use shared::model::{ApiProxyConfigDto, ProxyUserCredentialsDto};
use std::sync::Arc;

async fn save_config_api_proxy_user(
    method: axum::http::Method,
    axum::extract::State(app_state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path(target_name): axum::extract::Path<String>,
    axum::extract::Json(mut credential): axum::extract::Json<ProxyUserCredentialsDto>,
) -> impl axum::response::IntoResponse + Send {
    credential.prepare();
    if let Err(err) = credential.validate() {
        return (axum::http::StatusCode::BAD_REQUEST, axum::Json(json!({"error": err.to_string()}))).into_response();
    }

    let is_update = method == axum::http::Method::PUT;

    if let Some(old_api_proxy) = app_state.app_config.api_proxy.load().clone() {
        let mut api_proxy = (*old_api_proxy).clone();
        let mut target_found = false;
        for target_user in &api_proxy.user {
            if target_user.target == target_name {
                target_found = true;
            }
            for user in &target_user.credentials {
                if !is_update && user.username == credential.username {
                    return (axum::http::StatusCode::BAD_REQUEST, axum::Json(json!({"error": format!("Duplicate username {}", &credential.username)}))).into_response();
                }
                if let (Some(u), Some(c)) = (&user.token, &credential.token) {
                    if u == c && user.username != credential.username {
                        return (
                            axum::http::StatusCode::BAD_REQUEST,
                            axum::Json(json!({"error": format!("Duplicate token {c}")}))
                        ).into_response();
                    }
                }
            }
        }

        if is_update && !target_found {
            return (axum::http::StatusCode::BAD_REQUEST, axum::Json(json!({"error": format!("Target not found {target_name}")}))).into_response();
        }

        for target in &mut api_proxy.user {
            if target.target == target_name {
                if is_update {
                    let mut updated = false;
                    for user in &mut target.credentials {
                        if user.username == credential.username {
                            *user = ProxyUserCredentials::from(&credential);
                            updated = true;
                            break;
                        }
                    }
                    if !updated {
                        return (axum::http::StatusCode::BAD_REQUEST, axum::Json(json!({"error": format!("User {} not found in target {target_name}", credential.username)}))).into_response();
                    }
                } else {
                    target.credentials.push(ProxyUserCredentials::from(&credential));
                }
            }
        }

        let new_api_proxy = Arc::new(api_proxy);

        if new_api_proxy.use_user_db {
            if let Err(err) = store_api_user(&app_state.app_config, &new_api_proxy.user) {
                return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, axum::Json(json!({"error": err.to_string()}))).into_response();
            }
        } else {
            let config = app_state.app_config.config.load();
            let backup_dir = config.get_backup_dir();
            let paths = app_state.app_config.paths.load();
            if let Some(err) = crate::api::endpoints::v1_api_config::intern_save_config_api_proxy(backup_dir.as_ref(), &ApiProxyConfigDto::from(&*new_api_proxy), paths.api_proxy_file_path.as_str()) {
                return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, axum::Json(json!({"error": err.to_string()}))).into_response();
            }
        }
        // Udate state after successful save
        app_state.app_config.api_proxy.store(Some(Arc::clone(&new_api_proxy)));
    }
    axum::http::StatusCode::OK.into_response()
}

async fn delete_config_api_proxy_user(
    axum::extract::State(app_state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path((target_name, username)): axum::extract::Path<(String, String)>,
) -> impl axum::response::IntoResponse + Send {
    if let Some(old_api_proxy) = app_state.app_config.api_proxy.load().clone() {
        let mut api_proxy = (*old_api_proxy).clone();
        let mut modified = false;

        for target_user in &mut api_proxy.user {
            if target_user.target == target_name {
                let count = target_user.credentials.len();
                target_user.credentials.retain(|user| user.username != username);
                modified = count != target_user.credentials.len();
                break;
            }
        }
        if modified {
            let new_api_proxy = Arc::new(api_proxy);
            app_state.app_config.api_proxy.store(Some(Arc::clone(&new_api_proxy)));

            if new_api_proxy.use_user_db {
                if let Err(err) = store_api_user(&app_state.app_config, &new_api_proxy.user) {
                    return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, axum::Json(json!({"error": err.to_string()}))).into_response();
                }
            } else {
                let config = app_state.app_config.config.load();
                let backup_dir = config.get_backup_dir();
                let paths = app_state.app_config.paths.load();
                if let Some(err) = crate::api::endpoints::v1_api_config::intern_save_config_api_proxy(backup_dir.as_ref(), &ApiProxyConfigDto::from(&*new_api_proxy), paths.api_proxy_file_path.as_str()) {
                    return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, axum::Json(json!({"error": err.to_string()}))).into_response();
                }
            }
        } else {
            return (axum::http::StatusCode::BAD_REQUEST, axum::Json(json!({"error": format!("User not found {username} in target {target_name}")}))).into_response();
        }
    }
    axum::http::StatusCode::OK.into_response()
}

pub fn v1_api_user_register(router: Router<Arc<AppState>>) -> axum::Router<Arc<AppState>> {
    router
        .route("/user/{target}", axum::routing::post(save_config_api_proxy_user))
        .route("/user/{target}", axum::routing::put(save_config_api_proxy_user))
        .route("/user/{target}/{username}", axum::routing::delete(delete_config_api_proxy_user))
}
