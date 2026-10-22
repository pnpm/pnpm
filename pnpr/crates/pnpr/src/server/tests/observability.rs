use std::{sync::Arc, time::Duration};

use axum::{
    body::{Body, to_bytes},
    http::{HeaderValue, Request, StatusCode, header},
};
use tempfile::TempDir;
use tower::ServiceExt;

use super::{app_with_token, record};
use crate::server::{
    RequestId, ServerMetrics, generate_request_id, is_valid_request_id, metrics::DURATION_BUCKETS,
};

fn test_app(tmp: &TempDir) -> axum::Router {
    app_with_token(tmp, "test-token", record(false, &[]))
}

#[tokio::test]
async fn metrics_endpoint_exposes_prometheus_text_format() {
    let tmp = TempDir::new().unwrap();
    let app = test_app(&tmp);

    let request = Request::builder()
        .uri("/-/metrics")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/plain; version=0.0.4; charset=utf-8",
    );

    let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();

    assert!(text.contains("# HELP pnpr_http_requests_total"));
    assert!(text.contains("pnpr_build_info{version="));
    assert!(text.contains("pnpr_http_requests_in_flight 1"));
    assert!(text.contains("pnpr_http_response_bytes_total"));
    assert!(text.contains("pnpr_http_request_duration_seconds"));
}

#[tokio::test]
async fn request_without_request_id_gets_fresh_uuid() {
    let tmp = TempDir::new().unwrap();
    let app = test_app(&tmp);

    let request = Request::builder()
        .uri("/-/live")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let req_id_header = response
        .headers()
        .get("x-request-id")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(req_id_header.len(), 36);
    assert!(is_valid_request_id(req_id_header));
}

#[tokio::test]
async fn request_with_valid_request_id_is_echoed() {
    let tmp = TempDir::new().unwrap();
    let app = test_app(&tmp);

    let custom_id = "custom-trace-id-abc12345";
    let request = Request::builder()
        .uri("/-/live")
        .header("x-request-id", custom_id)
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-request-id"], custom_id);
}

#[tokio::test]
async fn request_with_invalid_request_id_gets_fresh_uuid() {
    let tmp = TempDir::new().unwrap();
    let app = test_app(&tmp);

    let bad_id = "invalid id with spaces";
    let request = Request::builder()
        .uri("/-/live")
        .header("x-request-id", bad_id)
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let header_val = response
        .headers()
        .get("x-request-id")
        .unwrap()
        .to_str()
        .unwrap();
    assert_ne!(header_val, bad_id);
    assert_eq!(header_val.len(), 36);
    assert!(is_valid_request_id(header_val));
}

#[tokio::test]
async fn metrics_records_request_count_and_duration() {
    let tmp = TempDir::new().unwrap();
    let app = test_app(&tmp);

    let req1 = Request::builder()
        .uri("/-/live")
        .body(Body::empty())
        .unwrap();
    let res1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(res1.status(), StatusCode::OK);

    let req2 = Request::builder()
        .uri("/-/ping")
        .body(Body::empty())
        .unwrap();
    let res2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(res2.status(), StatusCode::OK);

    let req_metrics = Request::builder()
        .uri("/-/metrics")
        .body(Body::empty())
        .unwrap();
    let res_metrics = app.oneshot(req_metrics).await.unwrap();
    assert_eq!(res_metrics.status(), StatusCode::OK);

    let body = to_bytes(res_metrics.into_body(), 64 * 1024).await.unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();

    assert!(text.contains(r#"pnpr_http_requests_total{method="GET",status="200"}"#));
    assert!(text.contains("pnpr_http_response_bytes_total"));
    assert!(text.contains("pnpr_http_request_duration_seconds_count"));
}

#[tokio::test]
async fn metrics_and_the_npm_package_named_metrics_have_separate_paths() {
    let tmp = TempDir::new().unwrap();
    let app = test_app(&tmp);
    let npm_accept = HeaderValue::from_static(
        "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8",
    );

    let scrape_req = Request::builder()
        .uri("/metrics")
        .header(header::ACCEPT, npm_accept.clone())
        .body(Body::empty())
        .unwrap();
    let scrape_res = app
        .clone()
        .oneshot(scrape_req)
        .await
        .unwrap();
    assert_eq!(scrape_res.status(), StatusCode::OK);
    assert_eq!(
        scrape_res.headers()[header::CONTENT_TYPE],
        "text/plain; version=0.0.4; charset=utf-8",
    );

    let npm_req = Request::builder()
        .uri("/npm/metrics")
        .header(header::ACCEPT, npm_accept)
        .body(Body::empty())
        .unwrap();
    let npm_res = app.oneshot(npm_req).await.unwrap();
    // The empty test store has no package named "metrics".
    assert_eq!(npm_res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn authenticated_request_records_user_and_echoes_request_id() {
    let tmp = TempDir::new().unwrap();
    let app = test_app(&tmp);

    let req = Request::builder()
        .uri("/-/whoami")
        .header(header::AUTHORIZATION, "Bearer test-token")
        .header("x-request-id", "auth-req-42")
        .body(Body::empty())
        .unwrap();

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["x-request-id"], "auth-req-42");

    let body = to_bytes(res.into_body(), 1024).await.unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains(r#""username":"alice""#));
}

#[test]
fn request_id_display_and_as_str() {
    let req_id = RequestId(Arc::from("sample-req-123"));
    assert_eq!(req_id.as_str(), "sample-req-123");
    assert_eq!(req_id.to_string(), "sample-req-123");
}

#[test]
fn validates_reasonable_request_ids() {
    assert!(is_valid_request_id("c1a2b3c4-d5e6-7f8a-9b0c-1d2e3f4a5b6c"));
    assert!(is_valid_request_id("req_1234567890abcdef"));
    assert!(is_valid_request_id("cluster-a:node-1:req-99"));
}

#[test]
fn rejects_invalid_request_ids() {
    assert!(!is_valid_request_id(""));
    assert!(!is_valid_request_id("has space"));
    assert!(!is_valid_request_id("has\nnewline"));
    assert!(!is_valid_request_id("has\r\ninjection"));
    assert!(!is_valid_request_id("has\tcontrol"));
    assert!(!is_valid_request_id(&"a".repeat(129)));
}

#[test]
fn generates_valid_uuid_v4() {
    let id = generate_request_id();
    assert_eq!(id.len(), 36);
    assert!(is_valid_request_id(&id));
    let parts: Vec<&str> = id.split('-').collect();
    assert_eq!(parts.len(), 5);
    assert_eq!(parts[0].len(), 8);
    assert_eq!(parts[1].len(), 4);
    assert_eq!(parts[2].len(), 4);
    assert_eq!(parts[3].len(), 4);
    assert_eq!(parts[4].len(), 12);
    assert!(parts[2].starts_with('4'));
    let first_variant_char = parts[3].chars().next().unwrap();
    assert!(matches!(first_variant_char, '8' | '9' | 'a' | 'b'));
}

#[test]
fn in_flight_guard_tracks_and_decrements() {
    let metrics = ServerMetrics::new();
    assert_eq!(metrics.in_flight.load(std::sync::atomic::Ordering::Relaxed), 0);
    {
        let _guard = metrics.start_in_flight();
        assert_eq!(metrics.in_flight.load(std::sync::atomic::Ordering::Relaxed), 1);
    }
    assert_eq!(metrics.in_flight.load(std::sync::atomic::Ordering::Relaxed), 0);
}

#[test]
fn records_request_and_renders_prometheus() {
    let metrics = ServerMetrics::new();
    metrics.record_request("GET", 200, Duration::from_millis(20), 1024);
    metrics.record_request("GET", 200, Duration::from_millis(80), 2048);
    metrics.record_request("POST", 401, Duration::from_millis(5), 128);
    metrics.record_cache_hit();
    metrics.record_cache_miss();
    metrics.record_login_throttled();

    let output = metrics.render_prometheus();
    assert!(output.contains("pnpr_build_info{version="));
    assert!(output.contains(r#"pnpr_http_requests_total{method="GET",status="200"} 2"#));
    assert!(output.contains(r#"pnpr_http_requests_total{method="POST",status="401"} 1"#));
    assert!(output.contains("pnpr_http_response_bytes_total 3200"));
    assert!(output.contains("pnpr_http_request_duration_seconds_count 3"));
    assert!(output.contains("pnpr_cache_hits_total 1"));
    assert!(output.contains("pnpr_cache_misses_total 1"));
    assert!(output.contains("pnpr_login_rate_limits_total 1"));

    for bound in DURATION_BUCKETS {
        assert!(output.contains(&format!(
            r#"pnpr_http_request_duration_seconds_bucket{{le="{bound}"}}"#,
        )));
    }
}

#[test]
fn records_unknown_methods_as_other_to_bound_cardinality() {
    let metrics = ServerMetrics::new();
    for i in 0..100 {
        metrics.record_request(&format!("CUSTOM_{i}"), 405, Duration::from_millis(1), 0);
    }
    let output = metrics.render_prometheus();
    assert!(output.contains(r#"pnpr_http_requests_total{method="OTHER",status="405"} 100"#));
    assert!(!output.contains("CUSTOM_"));
}
