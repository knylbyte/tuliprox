use crate::api::model::{AppState, EventMessage};
use axum::response::IntoResponse;
use log::{debug, error, info};
use std::sync::Arc;
use serde_json::json;
use shared::model::{LibraryScanRequest, LibraryScanSummary, LibraryScanSummaryStatus, LibraryStatus};
use crate::library::{LibraryProcessor};

// Triggers a library scan
async fn scan_library(
    axum::extract::State(app_state): axum::extract::State<Arc<AppState>>,
    axum::Json(request): axum::Json<LibraryScanRequest>,
) -> axum::response::Response {
    debug!("Library scan requested (force_rescan: {})", request.force_rescan);

    // Check if Library is enabled
    let lib_config = match app_state.app_config.config.load().library.as_ref() {
        Some(config) if config.enabled => config.clone(),
        _ => {
            let response = LibraryScanSummary {
                status: LibraryScanSummaryStatus::Error,
                message: "Library is not enabled".to_string(),
                result: None,
            };
            let _ = app_state.event_manager.send_event(EventMessage::LibraryScanProgress(response));
            return (axum::http::StatusCode::BAD_REQUEST, axum::Json(json!({"error": "Library is not enabled".to_string()}))).into_response();
        }
    };

    let client = app_state.http_client.load_full().as_ref().clone();
    tokio::spawn(async move {
        // Create processor and run scan
        let processor = LibraryProcessor::new(lib_config, client);

        match processor.scan(request.force_rescan).await {
            Ok(result) => {
                info!("Library scan completed successfully");
                let response = LibraryScanSummary {
                    status: LibraryScanSummaryStatus::Success,
                    message: format!(
                        "Scan completed: {} files scanned, {} added, {} updated, {} removed",
                        result.files_scanned, result.files_added, result.files_updated, result.files_removed
                    ),
                    result: Some(result),
                };
                let _ = app_state.event_manager.send_event(EventMessage::LibraryScanProgress(response));
            }
            Err(err) => {
                error!("Library scan failed: {err}");
                let response = LibraryScanSummary {
                    status: LibraryScanSummaryStatus::Error,
                    message: format!("Scan failed: {err}"),
                    result: None,
                };
                let _ = app_state.event_manager.send_event(EventMessage::LibraryScanProgress(response));
            }
        }
    });

    axum::http::StatusCode::ACCEPTED.into_response()

}

/// Gets Library status
async fn get_library_status(
    axum::extract::State(app_state): axum::extract::State<Arc<AppState>>,
) -> axum::response::Response {

    if let Some(config) = app_state.app_config.config.load().library.as_ref() {
        if config.enabled {
            let client = app_state.http_client.load_full().as_ref().clone();
            // Get statistics from processor
            let processor = LibraryProcessor::new(config.clone(), client);
            let entries = processor.get_all_entries().await;

            let movies = entries
                .iter()
                .filter(|e| e.metadata.is_movie())
                .count();
            let series = entries
                .iter()
                .filter(|e| e.metadata.is_series())
                .count();

            let response = LibraryStatus {
                enabled: true,
                total_items: entries.len(),
                movies,
                series,
                path: Some(config.metadata.path.clone()),
            };

            return axum::Json(response).into_response();
        }
    }

    let response = LibraryStatus::default();
    axum::Json(response).into_response()

}


/// Registers Library API routes
pub fn library_api_register(router: axum::Router<Arc<AppState>>) -> axum::Router<Arc<AppState>> {
    router
        .route("/library/scan", axum::routing::post(scan_library))
        .route("/library/status", axum::routing::get(get_library_status))
}
