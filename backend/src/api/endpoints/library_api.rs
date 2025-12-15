use crate::api::model::AppState;
use axum::response::IntoResponse;
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::library::{LibraryProcessor, LibraryScanResult};

// Request to trigger a Library scan
#[derive(Debug, Deserialize)]
pub struct LibraryScanRequest {
    // Force rescan of all files, ignoring modification timestamps
    #[serde(default)]
    pub force_rescan: bool,
}
// Response for Library scan
#[derive(Debug, Serialize)]
pub struct LibraryScanResponse {
    pub status: String,
    pub message: String,
    pub result: Option<LibraryScanResult>,
}

#[derive(Debug, Serialize)]
pub struct LibraryStatusResponse {
    pub enabled: bool,
    pub total_items: usize,
    pub movies: usize,
    pub series: usize,
    pub path: Option<String>,
}

// Triggers a library scan
async fn scan_library(
    axum::extract::State(app_state): axum::extract::State<Arc<AppState>>,
    axum::Json(request): axum::Json<LibraryScanRequest>,
) -> axum::response::Response {
    info!("Library scan requested (force_rescan: {})", request.force_rescan);

    // Check if VOD is enabled
    let lib_config = match app_state.app_config.config.load().library.as_ref() {
        Some(config) if config.enabled => config.clone(),
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
    let processor = LibraryProcessor::new(lib_config);

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

    if let Some(config) = app_state.app_config.config.load().library.as_ref() {
        if !config.enabled {
            let response = LibraryStatusResponse {
                enabled: false,
                total_items: 0,
                movies: 0,
                series: 0,
                path: None,
            };
            return axum::Json(response).into_response();
        }

        // Get statistics from processor
        let processor = LibraryProcessor::new(config.clone());
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
            path: Some(config.metadata.path.clone()),
        };

        axum::Json(response).into_response()
    } else {
        let response = LibraryStatusResponse {
            enabled: false,
            total_items: 0,
            movies: 0,
            series: 0,
            path: None,
        };
        axum::Json(response).into_response()
    }
}


/// Registers Library API routes
pub fn library_api_register(router: axum::Router<Arc<AppState>>) -> axum::Router<Arc<AppState>> {
    router
        .route("/library/scan", axum::routing::post(scan_library))
        .route("/library/status", axum::routing::get(get_library_status))
}
