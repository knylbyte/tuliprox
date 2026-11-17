use super::{
    access_lease_id_from_variant_uri, get_response, proxy_session_id_from_variant_uri, response_body,
    shared_request_flow_entry, shared_request_flow_fixture_with_origin, single_variant_uri, test_fingerprint, AppState,
    Arc, HlsAccessLeaseId, ProxySessionId, StatusCode,
};
use crate::api::endpoints::{custom_video_stream_api::cvs_api_register, hls_api::tests::spawn_test_status_origin};
use axum::{body::Body, extract::ConnectInfo, http::Request};
use tower::ServiceExt;
use tuliprox_core::utils::current_time_millis;
use tuliprox_hls::api::{HlsAccessLeasePendingDeadline, HlsAccessLeaseState};

const ASSET: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../test/fixtures/hls/channel_unavailable.ts"));
const ORIGIN_FAILURE: StatusCode = StatusCode::PROXY_AUTHENTICATION_REQUIRED;

fn configure_cvr(app: &AppState, enabled: bool, error_status: StatusCode, asset: bool) -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    if asset {
        std::fs::write(directory.path().join("channel_unavailable.ts"), ASSET).unwrap();
    }
    let mut config = app.app_config.config.load().as_ref().clone();
    config.custom_stream_response_enabled = enabled;
    config.custom_stream_response_error_status = error_status.as_u16();
    config.custom_stream_response_path = Some(directory.path().to_string_lossy().into_owned());
    app.app_config.set_config(config).unwrap();
    let loaded = app.app_config.custom_stream_response.load_full().expect("configured asset directory");
    assert_eq!(loaded.channel_unavailable.is_some(), asset);
    directory
}

#[tokio::test]
async fn cold_start_manifest_uses_configured_502_with_cvr_disabled() {
    assert_manifest_error(false, StatusCode::BAD_GATEWAY, true, StatusCode::BAD_GATEWAY).await;
}

#[tokio::test]
async fn cold_start_manifest_uses_configured_422_with_cvr_disabled() {
    assert_manifest_error(false, StatusCode::UNPROCESSABLE_ENTITY, true, StatusCode::UNPROCESSABLE_ENTITY).await;
}

#[tokio::test]
async fn cold_start_manifest_keeps_404_with_cvr_enabled_and_asset_missing() {
    assert_manifest_error(true, StatusCode::BAD_GATEWAY, false, StatusCode::NOT_FOUND).await;
}

async fn assert_manifest_error(enabled: bool, configured: StatusCode, asset: bool, expected: StatusCode) {
    let origin = spawn_test_status_origin(ORIGIN_FAILURE, b"").await;
    let fixture = shared_request_flow_fixture_with_origin(origin, 1).await;
    let _assets = configure_cvr(&fixture.app_state, enabled, configured, asset);
    let uri = single_variant_uri(shared_request_flow_entry(&fixture, &test_fingerprint()).await).await;
    let response = get_response(Arc::clone(&fixture.app_state), &uri, None).await;
    let status = response.status();
    let body = response_body(response).await;
    let manifest_gets = fixture.origin.manifest_request_count();
    eprintln!(
        "cold_start origin_status={} origin_manifest_gets={manifest_gets} cvr={enabled} configured_status={} asset={asset} client_status={} body_bytes={}",
        ORIGIN_FAILURE.as_u16(),
        configured.as_u16(),
        status.as_u16(),
        body.len()
    );
    assert_eq!(manifest_gets, 1, "the first origin manifest request fails definitively");
    assert_eq!(status, expected);
    assert!(body.is_empty());
}

#[tokio::test]
async fn cold_start_manifest_and_segment_serve_the_enabled_cvr_asset() {
    let origin = spawn_test_status_origin(ORIGIN_FAILURE, b"").await;
    let fixture = shared_request_flow_fixture_with_origin(origin, 1).await;
    let _assets = configure_cvr(&fixture.app_state, true, StatusCode::BAD_GATEWAY, true);
    let uri = single_variant_uri(shared_request_flow_entry(&fixture, &test_fingerprint()).await).await;
    let response = get_response(Arc::clone(&fixture.app_state), &uri, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = String::from_utf8(response_body(response).await.to_vec()).unwrap();
    assert!(body.lines().any(|line| line == "#EXT-X-ENDLIST"));
    let first_media = body.lines().find(|line| !line.is_empty() && !line.starts_with('#')).unwrap();
    let mut request = Request::builder().uri(first_media).body(Body::empty()).unwrap();
    request.extensions_mut().insert(ConnectInfo(test_fingerprint().addr));
    let response = cvs_api_register().with_state(Arc::clone(&fixture.app_state)).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let media = response_body(response).await;
    assert_eq!(media.as_ref(), ASSET);
    assert_eq!(fixture.origin.manifest_request_count(), 1);
    eprintln!(
        "cold_start origin_status=407 origin_manifest_gets=1 cvr=true configured_status=502 asset=true manifest_status=200 endlist=true media_status=200 media_bytes={} asset_bytes_match=true",
        media.len()
    );
}

#[tokio::test]
async fn cold_start_expired_canonical_lease_keeps_404_with_cvr_disabled() {
    let origin = spawn_test_status_origin(ORIGIN_FAILURE, b"").await;
    let fixture = shared_request_flow_fixture_with_origin(origin, 1).await;
    let app = &fixture.app_state;
    let _assets = configure_cvr(app, false, StatusCode::BAD_GATEWAY, true);
    let uri = single_variant_uri(shared_request_flow_entry(&fixture, &test_fingerprint()).await).await;
    let lease_id = HlsAccessLeaseId(access_lease_id_from_variant_uri(&uri).to_owned());
    let proxy_id = ProxySessionId(proxy_session_id_from_variant_uri(&uri).to_owned());
    {
        let mut leases = app.hls.proxy.access_leases().write().await;
        let mut lease = leases.remove_access_lease(&lease_id).expect("the master issued a fresh lease");
        assert_eq!(lease.proxy_session_id, proxy_id);
        assert!(lease.valid_until_ms > current_time_millis());
        let expired_at_ms = current_time_millis().saturating_sub(1);
        lease.pending_deadline = Some(HlsAccessLeasePendingDeadline::Bootstrap { deadline_ms: expired_at_ms });
        lease.valid_until_ms = expired_at_ms;
        leases.prepare_access_lease(lease);
    }
    let expired = app
        .hls
        .proxy
        .access_lease_response_snapshot(&lease_id, &proxy_id, current_time_millis())
        .await
        .expect("the expired lease still has a response snapshot");
    assert_eq!(expired.state, HlsAccessLeaseState::Expired);
    assert!(expired.last_manifest_snapshot.is_none());
    let response = get_response(Arc::clone(app), &uri, None).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = response_body(response).await;
    assert!(body.is_empty());
    eprintln!("cold_start lease_expired=true cvr=false configured_status=502 client_status=404 body_bytes=0");
}
