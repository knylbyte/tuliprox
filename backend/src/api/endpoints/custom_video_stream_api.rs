use axum::response::IntoResponse;
use std::str::FromStr;
use std::sync::Arc;
use crate::api::model::{create_custom_video_stream_response, AppState, CustomVideoStreamType};

async fn cvs_api(
    axum::extract::Path((username, password, stream_type)): axum::extract::Path<(
        String,
        String,
        String,
    )>,
    axum::extract::State(app_state): axum::extract::State<Arc<AppState>>,
) -> impl IntoResponse + Send {

    let Ok(custom_video_type) = CustomVideoStreamType::from_str(&stream_type) else {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    };

    let Some((user, _target)) =  app_state.app_config.get_target_for_user(&username, &password) else {
        return axum::http::StatusCode::FORBIDDEN.into_response();
    };

    if user.permission_denied(&app_state) {
        return axum::http::StatusCode::FORBIDDEN.into_response();
    }

    create_custom_video_stream_response(
        &app_state.app_config,
        custom_video_type
    ).into_response()
}

pub fn cvs_api_register() -> axum::Router<Arc<AppState>> {
    axum::Router::new()
        .route("/cvs/{username}/{password}/{stream_type}", axum::routing::get(cvs_api))
}
