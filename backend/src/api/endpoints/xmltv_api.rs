use axum::response::IntoResponse;
use chrono::{DateTime, Duration, FixedOffset, NaiveDateTime, Offset, TimeZone, Utc};
use chrono_tz::Tz;
use log::{error, trace};
use quick_xml::events::{BytesStart, Event};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;
use crate::api::api_utils::try_unwrap_body;
use crate::api::api_utils::{get_user_target, serve_file};
use crate::api::model::AppState;
use crate::api::model::UserApiRequest;
use crate::model::{Config, EPG_TAG_PROGRAMME};
use crate::model::{ConfigTarget, ProxyUserCredentials, TargetOutput};
use crate::repository::m3u_repository::m3u_get_epg_file_path;
use crate::repository::storage::get_target_storage_path;
use crate::repository::xtream_repository::{xtream_get_epg_file_path, xtream_get_storage_path};
use crate::utils;

pub fn get_empty_epg_response() -> impl axum::response::IntoResponse + Send {
    try_unwrap_body!(axum::response::Response::builder()
        .status(axum::http::StatusCode::OK)
        .header(axum::http::header::CONTENT_TYPE, axum::http::HeaderValue::from_static("text/xml"))
        .body(axum::body::Body::from(r#"<?xml version="1.0" encoding="utf-8" ?><!DOCTYPE tv SYSTEM "xmltv.dtd"><tv generator-info-name="Xtream Codes" generator-info-url=""></tv>"#)))
}

/// Applies an EPG timeshift to an XMLTV programme time, taking the existing timezone into account.
/// @see `https://wiki.xmltv.org/index.php/XMLTVFormat`
///
/// # Arguments
/// * `original` - original XMLTV datetime string, e.g. "20080715003000 -0600"
/// * `shift` - timeshift in minutes as `chrono::Duration`
///
/// # Returns
/// A new datetime string in the same format with the same timezone.
fn time_correct(original: &str, shift: &Duration) -> String {
    let (datetime_part, tz_part) = if let Some((dt, tz)) = original.trim().rsplit_once(' ') {
        (dt, tz)
    } else {
        (original.trim(), "+0000")
    };

    let Ok(naive_dt) = NaiveDateTime::parse_from_str(datetime_part, "%Y%m%d%H%M%S") else { return original.to_string() };

    let tz_offset_minutes = if tz_part.len() == 5 {
        let sign = if &tz_part[0..1] == "-" { -1 } else { 1 };
        let hours: i32 = tz_part[1..3].parse().unwrap_or(0);
        let mins: i32 = tz_part[3..5].parse().unwrap_or(0);
        sign * (hours * 60 + mins)
    } else {
        0
    };

    let tz = FixedOffset::east_opt(tz_offset_minutes * 60).unwrap_or(FixedOffset::east_opt(0).unwrap());

    // Use modern chrono API
    let dt: DateTime<FixedOffset> = tz
        .from_local_datetime(&naive_dt)
        .single()
        .unwrap_or_else(|| tz.from_utc_datetime(&naive_dt));

    let shifted_dt = dt + *shift;

    format!("{} {}",shifted_dt.format("%Y%m%d%H%M%S"), format_offset(tz_offset_minutes))
}

fn format_offset(offset_minutes: i32) -> String {
    let sign = if offset_minutes < 0 { '-' } else { '+' };
    let abs = offset_minutes.abs();
    let hours = abs / 60;
    let mins = abs % 60;
    format!("{sign}{hours:02}{mins:02}")
}

fn get_epg_path_for_target_of_type(target_name: &str, epg_path: PathBuf) -> Option<PathBuf> {
    if utils::path_exists(&epg_path) {
        return Some(epg_path);
    }
    trace!(
        "Cant find epg file for {target_name} target: {}",
        epg_path.to_str().unwrap_or("?")
    );
    None
}

pub (in crate::api) fn get_epg_path_for_target(config: &Config, target: &ConfigTarget) -> Option<PathBuf> {
    // TODO if we have multiple targets, first one serves, this can be problematic when
    // we use m3u playlist but serve xtream target epg

    // TODO if we share the same virtual_id for epg, can we store an epg file for the target ?
    for output in &target.output {
        match output {
            TargetOutput::Xtream(_) => {
                if let Some(storage_path) = xtream_get_storage_path(config, &target.name) {
                    return get_epg_path_for_target_of_type(
                        &target.name,
                        xtream_get_epg_file_path(&storage_path),
                    );
                }
            }
            TargetOutput::M3u(_) => {
                if let Some(target_path) = get_target_storage_path(config, &target.name) {
                    return get_epg_path_for_target_of_type(
                        &target.name,
                        m3u_get_epg_file_path(&target_path),
                    );
                }
            }
            TargetOutput::Strm(_) | TargetOutput::HdHomeRun(_) => {}
        }
    }
    None
}

/// Parses user-defined EPG timeshift configuration.
/// Supports either a numeric offset (e.g. "+2:30", "-1:15")
/// or a timezone name (e.g. "`Europe/Berlin`", "`UTC`", "`America/New_York`").
///
/// Returns the total offset in minutes (i32).
fn parse_timeshift(time_shift: Option<&String>) -> Option<i32> {
    time_shift.and_then(|offset| {
        // Try to parse as timezone name first
        if let Ok(tz) = offset.parse::<Tz>() {
            // Determine the current UTC offset of that timezone (including DST)
            let now = Utc::now();
            let local_time = tz.from_utc_datetime(&now.naive_utc());
            let offset_minutes = local_time.offset().fix().local_minus_utc() / 60;
            return Some(offset_minutes);
        }

        // If not a timezone, try to parse as numeric offset
        let sign_factor = if offset.starts_with('-') { -1 } else { 1 };
        let offset = offset.trim_start_matches(&['-', '+'][..]);

        let parts: Vec<&str> = offset.split(':').collect();
        let hours: i32 = parts.first().and_then(|h| h.parse().ok()).unwrap_or(0);
        let minutes: i32 = parts.get(1).and_then(|m| m.parse().ok()).unwrap_or(0);

        let total_minutes = hours * 60 + minutes;
        (total_minutes > 0).then_some(sign_factor * total_minutes)
    })
}

async fn serve_epg(
    epg_path: &Path,
    user: &ProxyUserCredentials,
) -> impl axum::response::IntoResponse + Send {
    match tokio::fs::File::open(epg_path).await {
        Ok(epg_file) => match parse_timeshift(user.epg_timeshift.as_ref()) {
            None => serve_file(epg_path, mime::TEXT_XML).await.into_response(),
            Some(duration) => serve_epg_with_timeshift(epg_file, duration),
        },
        Err(_) => get_empty_epg_response().into_response(),
    }
}

fn serve_epg_with_timeshift(
    epg_file: tokio::fs::File,
    offset_minutes: i32,
) -> axum::response::Response {
    let reader = tokio::io::BufReader::new(epg_file);
    let (tx, rx) = tokio::io::duplex(8192);
    tokio::spawn(async move {
        let encoder = async_compression::tokio::write::GzipEncoder::new(tx);
        let mut xml_reader = quick_xml::reader::Reader::from_reader(tokio::io::BufReader::new(reader));
        let mut xml_writer = quick_xml::writer::Writer::new(encoder);
        let mut buf = Vec::with_capacity(4096);
        let duration = Duration::minutes(i64::from(offset_minutes));

        loop {
            match xml_reader.read_event_into_async(&mut buf).await {
                Ok(Event::Start(ref e)) if e.name().as_ref() == b"programme" => {
                    // Modify the attributes
                    let mut elem = BytesStart::new(EPG_TAG_PROGRAMME);
                    for attr in e.attributes() {
                        match attr {
                            Ok(attr) if attr.key.as_ref() == b"start" => {
                                if let Ok(start_value) = attr.decode_and_unescape_value(xml_reader.decoder()) {
                                    // Modify the start attribute value as needed
                                    elem.push_attribute(("start", time_correct(&start_value, &duration).as_str()));
                                } else {
                                    // keep original attribute unchanged ?
                                    elem.push_attribute(attr);
                                }
                            }
                            Ok(attr) if attr.key.as_ref() == b"stop" => {
                                if let Ok(stop_value) = attr.decode_and_unescape_value(xml_reader.decoder()) {
                                    // Modify the stop attribute value as needed
                                    elem.push_attribute(("stop", time_correct(&stop_value, &duration).as_str()));
                                } else {
                                    elem.push_attribute(attr);
                                }
                            }
                            Ok(attr) => {
                                // Copy any other attributes as they are
                                elem.push_attribute(attr);
                            }
                            Err(e) => {
                                error!("Error parsing attribute: {e}");
                            }
                        }
                    }

                    // Write the modified start event
                    if let Err(e) = xml_writer.write_event_async(Event::Start(elem)).await {
                        error!("Failed to write Start event: {e}");
                        break;
                    }
                }
                Ok(Event::Eof) => break, // End of file
                Ok(event) => {
                    // Write any other event as is
                    if let Err(e) = xml_writer.write_event_async(event).await {
                        error!("Failed to write event: {e}");
                        break;
                    }
                }
                Err(e) => {
                    error!("Error: {e}");
                    break;
                }
            }

            buf.clear();
        }
        let _ = xml_writer.into_inner().shutdown().await;
    });

    let body_stream = ReaderStream::new(rx);
    try_unwrap_body!(axum::response::Response::builder()
        .header(
            axum::http::header::CONTENT_TYPE,
            mime::TEXT_XML.to_string()
        )
        .header(axum::http::header::CONTENT_ENCODING, "gzip") // Set Content-Encoding header
        .body(axum::body::Body::from_stream(body_stream)))
        .into_response()
}

/// Handles XMLTV EPG API requests, serving the appropriate EPG file with optional time-shifting based on user configuration.
///
/// Returns a 403 Forbidden response if the user or target is invalid or if the user lacks permission. If no EPG file is configured for the target, returns an empty EPG response. Otherwise, serves the EPG file, applying a time shift if specified by the user.
///
/// # Examples
///
/// ```
/// // Example usage within an Axum router:
/// let router = xmltv_api_register();
/// // A GET request to /xmltv.php with valid query parameters will invoke this handler.
/// ```
async fn xmltv_api(
    axum::extract::Query(api_req): axum::extract::Query<UserApiRequest>,
    axum::extract::State(app_state): axum::extract::State<Arc<AppState>>,
) -> impl IntoResponse + Send {
    let Some((user, target)) = get_user_target(&api_req, &app_state) else {
        return axum::http::StatusCode::FORBIDDEN.into_response();
    };

    if user.permission_denied(&app_state) {
        return axum::http::StatusCode::FORBIDDEN.into_response();
    }

    let config = &app_state.app_config.config.load();
    let Some(epg_path) = get_epg_path_for_target(config, &target) else {
        // No epg configured,  No processing or timeshift, epg can't be mapped to the channels.
        // we do not deliver epg
        return get_empty_epg_response().into_response();
    };

    serve_epg(&epg_path, &user).await.into_response()
}

/// Registers the XMLTV EPG API routes for handling HTTP GET requests.
///
/// The returned router maps the `/xmltv.php`, `/update/epg.php`, and `/epg` endpoints to the `xmltv_api` handler, enabling XMLTV EPG data retrieval with optional time-shifting and compression.
///
/// # Examples
///
/// ```
/// let router = xmltv_api_register();
/// // The router can now be used with an Axum server.
/// ```
pub fn xmltv_api_register() -> axum::Router<Arc<AppState>> {
    axum::Router::new()
        .route("/xmltv.php", axum::routing::get(xmltv_api))
        .route("/update/epg.php", axum::routing::get(xmltv_api))
        .route("/epg", axum::routing::get(xmltv_api))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_timeshift() {
        assert_eq!(parse_timeshift(Some(&String::from("2"))), Some(120));
        assert_eq!(parse_timeshift(Some(&String::from("-1:30"))), Some(-90));
        assert_eq!(parse_timeshift(Some(&String::from("+0:15"))), Some(15));
        assert_eq!(parse_timeshift(Some(&String::from("1:45"))), Some(105));
        assert_eq!(parse_timeshift(Some(&String::from(":45"))), Some(45));
        assert_eq!(parse_timeshift(Some(&String::from("-:45"))), Some(-45));
        assert_eq!(parse_timeshift(Some(&String::from("0:30"))), Some(30));
        assert_eq!(parse_timeshift(Some(&String::from(":3"))), Some(3));
        assert_eq!(parse_timeshift(Some(&String::from("2:"))), Some(120));
        assert_eq!(parse_timeshift(Some(&String::from("+2:00"))), Some(120));
        assert_eq!(parse_timeshift(Some(&String::from("-0:10"))), Some(-10));
        assert_eq!(parse_timeshift(Some(&String::from("invalid"))), None);
        assert_eq!(parse_timeshift(Some(&String::from("+abc"))), None);
        assert_eq!(parse_timeshift(Some(&String::new())), None);
        assert_eq!(parse_timeshift(None), None);
    }

    #[test]
    fn test_parse_timezone() {
        // This will depend on current DST; we just check it’s within a valid range
        let berlin = parse_timeshift(Some(&"Europe/Berlin".to_string())).unwrap();
        assert!(berlin == 60 || berlin == 120, "Berlin offset should be 60 or 120, got {}", berlin);

        let new_york = parse_timeshift(Some(&"America/New_York".to_string())).unwrap();
        assert!(new_york == -300 || new_york == -240, "New York offset should be -300 or -240, got {}", new_york);

        let tokyo = parse_timeshift(Some(&"Asia/Tokyo".to_string())).unwrap();
        assert_eq!(tokyo, 540); // always UTC+9

        let utc = parse_timeshift(Some(&"UTC".to_string())).unwrap();
        assert_eq!(utc, 0);
    }
}
