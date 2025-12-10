use crate::api::model::AppState;
use axum::response::IntoResponse;
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::library::{LibraryProcessor, LibraryScanResult};

/// Request to trigger a VOD scan
#[derive(Debug, Deserialize)]
pub struct LibraryScanRequest {
    /// Force rescan of all files, ignoring modification timestamps
    #[serde(default)]
    pub force_rescan: bool,
}

/// Response for VOD scan
#[derive(Debug, Serialize)]
pub struct LibraryScanResponse {
    pub status: String,
    pub message: String,
    pub result: Option<LibraryScanResult>,
}

/// Response for VOD status
#[derive(Debug, Serialize)]
pub struct LibraryStatusResponse {
    pub enabled: bool,
    pub total_items: usize,
    pub movies: usize,
    pub series: usize,
    pub storage_location: Option<String>,
}

/// Triggers a library scan
async fn scan_library(
    axum::extract::State(app_state): axum::extract::State<Arc<AppState>>,
    axum::Json(request): axum::Json<LibraryScanRequest>,
) -> axum::response::Response {
    info!("VOD scan requested (force_rescan: {})", request.force_rescan);

    // Check if VOD is enabled
    let vod_config = match app_state.app_config.library.load_full() {
        Some(config) if config.enabled => config,
        _ => {
            let response = LibraryScanResponse {
                status: "error".to_string(),
                message: "VOD is not enabled".to_string(),
                result: None,
            };
            return axum::Json(response).into_response();
        }
    };

    // Create processor and run scan
    let processor = LibraryProcessor::new(vod_config.as_ref().clone());

    match processor.scan(request.force_rescan).await {
        Ok(result) => {
            info!("Library scan completed successfully");
            let response = LibraryScanResponse {
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
            error!("Library scan failed: {err}");
            let response = LibraryScanResponse {
                status: "error".to_string(),
                message: format!("Scan failed: {err}"),
                result: None,
            };
            axum::Json(response).into_response()
        }
    }
}

/// Gets Library status
async fn get_library_status(
    axum::extract::State(app_state): axum::extract::State<Arc<AppState>>,
) -> axum::response::Response {
    let library_config = app_state.app_config.library.load_full();

    if let Some(config) = library_config {
        if !config.enabled {
            let response = LibraryStatusResponse {
                enabled: false,
                total_items: 0,
                movies: 0,
                series: 0,
                storage_location: None,
            };
            return axum::Json(response).into_response();
        }

        // Get statistics from processor
        let processor = LibraryProcessor::new(config.as_ref().clone());
        let entries = processor.get_all_entries().await;

        let movies = entries
            .iter()
            .filter(|e| e.metadata.is_movie())
            .count();
        let series = entries
            .iter()
            .filter(|e| e.metadata.is_series())
            .count();

        let response = LibraryStatusResponse {
            enabled: true,
            total_items: entries.len(),
            movies,
            series,
            storage_location: Some(config.metadata.storage.location.clone()),
        };

        axum::Json(response).into_response()
    } else {
        let response = LibraryStatusResponse {
            enabled: false,
            total_items: 0,
            movies: 0,
            series: 0,
            storage_location: None,
        };
        axum::Json(response).into_response()
    }
}

/// Gets a specific library item by virtual ID
async fn get_library_item(
    axum::extract::State(app_state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path(virtual_id): axum::extract::Path<u16>,
) -> axum::response::Response {
    let vod_config = match app_state.app_config.library.load_full() {
        Some(config) if config.enabled => config,
        _ => {
            return axum::http::StatusCode::NOT_FOUND.into_response();
        }
    };

    let processor = LibraryProcessor::new(vod_config.as_ref().clone());

    match processor.get_entry_by_virtual_id(virtual_id).await {
        Some(entry) => axum::Json(entry).into_response(),
        None => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}

/// Registers VOD API routes
pub fn library_api_register(router: axum::Router<Arc<AppState>>) -> axum::Router<Arc<AppState>> {
    router
        .route("/library/scan", axum::routing::post(scan_library))
        .route("/library/status", axum::routing::get(get_library_status))
        .route("/library/item/{virtual_id}", axum::routing::get(get_library_item))
}
