use crate::api::api_utils::try_unwrap_body;
use crate::api::model::AppState;
use crate::vod::processor::{VodProcessor, VodScanResult};
use axum::response::IntoResponse;
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Request to trigger a VOD scan
#[derive(Debug, Deserialize)]
pub struct VodScanRequest {
    /// Force rescan of all files, ignoring modification timestamps
    #[serde(default)]
    pub force_rescan: bool,
}

/// Response for VOD scan
#[derive(Debug, Serialize)]
pub struct VodScanResponse {
    pub status: String,
    pub message: String,
    pub result: Option<VodScanResult>,
}

/// Response for VOD status
#[derive(Debug, Serialize)]
pub struct VodStatusResponse {
    pub enabled: bool,
    pub total_items: usize,
    pub movies: usize,
    pub series: usize,
    pub storage_location: Option<String>,
}

/// Triggers a VOD scan
async fn scan_vod(
    axum::extract::State(app_state): axum::extract::State<Arc<AppState>>,
    axum::Json(request): axum::Json<VodScanRequest>,
) -> axum::response::Response {
    info!("VOD scan requested (force_rescan: {})", request.force_rescan);

    // Check if VOD is enabled
    let vod_config = match app_state.app_config.vod.load_full() {
        Some(config) if config.enabled => config,
        _ => {
            let response = VodScanResponse {
                status: "error".to_string(),
                message: "VOD is not enabled".to_string(),
                result: None,
            };
            return axum::Json(response).into_response();
        }
    };

    // Create processor and run scan
    let processor = VodProcessor::new(vod_config.as_ref().clone());

    match processor.scan(request.force_rescan).await {
        Ok(result) => {
            info!("VOD scan completed successfully");
            let response = VodScanResponse {
                status: "success".to_string(),
                message: format!(
                    "Scan completed: {} files scanned, {} added, {} updated, {} removed",
                    result.files_scanned, result.files_added, result.files_updated, result.files_removed
                ),
                result: Some(result),
            };
            axum::Json(response).into_response()
        }
        Err(err) => {
            error!("VOD scan failed: {}", err);
            let response = VodScanResponse {
                status: "error".to_string(),
                message: format!("Scan failed: {}", err),
                result: None,
            };
            axum::Json(response).into_response()
        }
    }
}

/// Gets VOD status
async fn get_vod_status(
    axum::extract::State(app_state): axum::extract::State<Arc<AppState>>,
) -> axum::response::Response {
    let vod_config = app_state.app_config.vod.load_full();

    if let Some(config) = vod_config {
        if !config.enabled {
            let response = VodStatusResponse {
                enabled: false,
                total_items: 0,
                movies: 0,
                series: 0,
                storage_location: None,
            };
            return axum::Json(response).into_response();
        }

        // Get statistics from processor
        let processor = VodProcessor::new(config.as_ref().clone());
        let entries = processor.get_all_entries().await;

        let movies = entries
            .iter()
            .filter(|e| e.metadata.is_movie())
            .count();
        let series = entries
            .iter()
            .filter(|e| e.metadata.is_series())
            .count();

        let response = VodStatusResponse {
            enabled: true,
            total_items: entries.len(),
            movies,
            series,
            storage_location: Some(config.metadata.storage_location.clone()),
        };

        axum::Json(response).into_response()
    } else {
        let response = VodStatusResponse {
            enabled: false,
            total_items: 0,
            movies: 0,
            series: 0,
            storage_location: None,
        };
        axum::Json(response).into_response()
    }
}

/// Gets a specific VOD item by virtual ID
async fn get_vod_item(
    axum::extract::State(app_state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path(virtual_id): axum::extract::Path<u16>,
) -> axum::response::Response {
    let vod_config = match app_state.app_config.vod.load_full() {
        Some(config) if config.enabled => config,
        _ => {
            return axum::http::StatusCode::NOT_FOUND.into_response();
        }
    };

    let processor = VodProcessor::new(vod_config.as_ref().clone());

    match processor.get_entry_by_virtual_id(virtual_id).await {
        Some(entry) => axum::Json(entry).into_response(),
        None => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}

/// Registers VOD API routes
pub fn vod_api_register(router: axum::Router<Arc<AppState>>) -> axum::Router<Arc<AppState>> {
    router
        .route("/vod/scan", axum::routing::post(scan_vod))
        .route("/vod/status", axum::routing::get(get_vod_status))
        .route("/vod/item/:virtual_id", axum::routing::get(get_vod_item))
}
