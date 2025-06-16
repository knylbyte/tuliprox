use crate::tuliprox_error::{create_tuliprox_error_result, TuliproxError, TuliproxErrorKind};
use reqwest::StatusCode;

/// Handle Trakt API response status and convert to appropriate error
pub fn handle_trakt_api_error(status: StatusCode, user: &str, list_slug: &str) -> Result<(), TuliproxError> {
    match status.as_u16() {
        404 => create_tuliprox_error_result!(TuliproxErrorKind::Notify, "Trakt list not found: {user}:{list_slug}"),
        401 => create_tuliprox_error_result!(TuliproxErrorKind::Notify, "Trakt API key is invalid or expired"),
        429 => create_tuliprox_error_result!(TuliproxErrorKind::Notify, "Trakt API rate limit exceeded"),
        _ => create_tuliprox_error_result!(TuliproxErrorKind::Notify,  "Trakt API error {status}: {}", status.canonical_reason().unwrap_or("Unknown"))
    }
}
