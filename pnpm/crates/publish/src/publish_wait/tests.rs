use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use mockito::Matcher;
use pnpm_network::{AuthHeaders, ThrottledClient};
use pnpm_reporter::SilentReporter;
use serde_json::json;

use super::wait_for_published_packages;
use crate::{PublishNetwork, PublishWaitError, parse_supported_registry_url};

async fn wait(server: &mockito::ServerGuard, timeout: Duration) -> Result<(), PublishWaitError> {
    let client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let registry = parse_supported_registry_url(&server.url()).unwrap().normalized_url;
    wait_for_published_packages::<SilentReporter>(
        &[("@scope/pkg", "1.0.0")],
        &registry,
        &PublishNetwork { client: &client, auth_headers: &auth_headers },
        timeout,
    )
    .await
}

fn metadata(tarball: &str) -> String {
    json!({"versions": {"1.0.0": {"name": "@scope/pkg", "version": "1.0.0", "dist": {"tarball": tarball}}}}).to_string()
}

#[tokio::test]
async fn disabled_wait_makes_no_requests() {
    let mut server = mockito::Server::new_async().await;
    let request = server
        .mock("GET", Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    wait(&server, Duration::ZERO).await.unwrap();
    request.assert_async().await;
}

#[tokio::test]
async fn requests_fresh_install_metadata_and_tarball_headers() {
    let mut server = mockito::Server::new_async().await;
    let metadata = server
        .mock("GET", "/@scope%2Fpkg")
        .match_header("accept", "application/vnd.npm.install-v1+json")
        .match_header("cache-control", "no-cache")
        .match_header("if-none-match", Matcher::Missing)
        .match_header("if-modified-since", Matcher::Missing)
        .with_body(metadata(&format!("{}/pkg.tgz", server.url())))
        .expect(1)
        .create_async()
        .await;
    let tarball = server
        .mock("HEAD", "/pkg.tgz")
        .match_header("range", Matcher::Missing)
        .match_header("cache-control", "no-cache")
        .with_header("content-length", "104857600")
        .with_status(200)
        .expect(1)
        .create_async()
        .await;
    wait(&server, Duration::from_secs(2)).await.unwrap();
    metadata.assert_async().await;
    tarball.assert_async().await;
}

#[tokio::test]
async fn exact_version_is_required_and_deadline_bounds_the_sleep() {
    let mut server = mockito::Server::new_async().await;
    let metadata = server
        .mock("GET", "/@scope%2Fpkg")
        .with_body(r#"{"versions":{"2.0.0":{}},"dist-tags":{"latest":"2.0.0"}}"#)
        .expect(1)
        .create_async()
        .await;
    let start = Instant::now();
    let error = wait(&server, Duration::from_millis(100)).await.unwrap_err();
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "deadline must bound the five-second retry delay",
    );
    assert!(error.to_string().contains("@scope/pkg@1.0.0"), "{error}");
    assert!(error.to_string().contains("100ms"), "{error}");
    metadata.assert_async().await;
}

#[tokio::test]
async fn timeout_does_not_issue_second_probe_when_delay_exceeds_deadline() {
    tokio::time::pause();
    let mut server = mockito::Server::new_async().await;
    let request = server
        .mock("GET", "/@scope%2Fpkg")
        .with_status(404)
        .expect(1)
        .create_async()
        .await;
    let error = wait(&server, Duration::from_millis(100)).await.unwrap_err();
    assert!(error.to_string().contains("Timed out"), "{error}");
    request.assert_async().await;
}

#[tokio::test]
async fn unrepresentable_timeout_does_not_panic() {
    let mut server = mockito::Server::new_async().await;
    let metadata = server
        .mock("GET", "/@scope%2Fpkg")
        .with_body(metadata(&format!("{}/pkg.tgz", server.url())))
        .expect(1)
        .create_async()
        .await;
    let tarball = server
        .mock("HEAD", "/pkg.tgz")
        .expect(1)
        .create_async()
        .await;
    wait(&server, Duration::MAX).await.unwrap();
    metadata.assert_async().await;
    tarball.assert_async().await;
}

#[tokio::test]
async fn transient_responses_retry_and_eventually_succeed() {
    let mut server = mockito::Server::new_async().await;
    let missing = server
        .mock("GET", "/@scope%2Fpkg")
        .with_status(404)
        .expect(1)
        .create_async()
        .await;
    let available = server
        .mock("GET", "/@scope%2Fpkg")
        .with_body(metadata(&format!("{}/pkg.tgz", server.url())))
        .expect(1)
        .create_async()
        .await;
    let tarball = server
        .mock("HEAD", "/pkg.tgz")
        .expect(1)
        .create_async()
        .await;
    wait(&server, Duration::from_secs(8)).await.unwrap();
    missing.assert_async().await;
    available.assert_async().await;
    tarball.assert_async().await;
}

#[tokio::test]
async fn permanent_errors_and_invalid_metadata_fail_without_waiting() {
    for (status, body) in [(401, ""), (403, ""), (400, ""), (200, "bad json"), (200, "{}")] {
        let mut server = mockito::Server::new_async().await;
        let request = server
            .mock("GET", "/@scope%2Fpkg")
            .with_status(status)
            .with_body(body)
            .expect(1)
            .create_async()
            .await;
        let start = Instant::now();
        let error = wait(&server, Duration::from_secs(20)).await.unwrap_err();
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "permanent failure must not wait: {error}",
        );
        assert!(error.to_string().contains("Could not confirm"), "{error}");
        request.assert_async().await;
    }
}

#[tokio::test]
async fn unavailable_tarballs_do_not_confirm_readiness() {
    for status in [404, 408, 429, 503] {
        let mut server = mockito::Server::new_async().await;
        let metadata = server
            .mock("GET", "/@scope%2Fpkg")
            .with_body(metadata(&format!("{}/pkg.tgz", server.url())))
            .create_async()
            .await;
        let tarball = server
            .mock("HEAD", "/pkg.tgz")
            .with_status(status)
            .expect(1)
            .create_async()
            .await;
        let error = wait(&server, Duration::from_millis(100)).await.unwrap_err();
        assert!(error.to_string().contains("Timed out"), "{error}");
        metadata.assert_async().await;
        tarball.assert_async().await;
    }
}

#[tokio::test]
async fn deadline_cancels_an_in_flight_response_body() {
    let mut server = mockito::Server::new_async().await;
    let request = server
        .mock("GET", "/@scope%2Fpkg")
        .with_chunked_body(|writer| {
            std::thread::sleep(Duration::from_millis(500));
            let _ = writer.write_all(b"{}");
            Ok(())
        })
        .create_async()
        .await;
    let start = Instant::now();
    let error = wait(&server, Duration::from_millis(100)).await.unwrap_err();
    assert!(start.elapsed() < Duration::from_millis(450), "request must be cancelled: {error}");
    request.assert_async().await;
}

#[tokio::test]
async fn redirect_uses_destination_credentials_and_preserves_head() {
    let mut server = mockito::Server::new_async().await;
    let mut cdn = mockito::Server::new_async().await;
    let metadata = server
        .mock("GET", "/@scope%2Fpkg")
        .match_header("authorization", "Bearer registry-secret")
        .with_body(metadata(&format!("{}/pkg.tgz", server.url())))
        .create_async()
        .await;
    let redirect = server
        .mock("HEAD", "/pkg.tgz")
        .match_header("authorization", "Bearer registry-secret")
        .with_status(302)
        .with_header("location", &format!("{}/pkg.tgz", cdn.url()))
        .create_async()
        .await;
    let tarball = cdn
        .mock("HEAD", "/pkg.tgz")
        .match_header("authorization", Matcher::Missing)
        .match_header("range", Matcher::Missing)
        .with_status(200)
        .create_async()
        .await;
    let client = ThrottledClient::default();
    let auth_headers = AuthHeaders::from_map(HashMap::from([(
        format!("{}/", server.url().trim_start_matches("http:")),
        "Bearer registry-secret".to_owned(),
    )]));
    let registry = parse_supported_registry_url(&server.url()).unwrap().normalized_url;
    wait_for_published_packages::<SilentReporter>(
        &[("@scope/pkg", "1.0.0")],
        &registry,
        &PublishNetwork { client: &client, auth_headers: &auth_headers },
        Duration::from_secs(2),
    )
    .await
    .unwrap();
    metadata.assert_async().await;
    redirect.assert_async().await;
    tarball.assert_async().await;
}

#[tokio::test]
async fn retry_after_supports_seconds_and_http_dates() {
    for value in [
        "30".to_owned(),
        httpdate::fmt_http_date(std::time::SystemTime::now() + Duration::from_secs(30)),
    ] {
        let mut server = mockito::Server::new_async().await;
        let request = server
            .mock("GET", "/@scope%2Fpkg")
            .with_status(429)
            .with_header("retry-after", &value)
            .create_async()
            .await;
        let client = ThrottledClient::default();
        let auth_headers = AuthHeaders::default();
        let registry = parse_supported_registry_url(&server.url()).unwrap().normalized_url;
        let result = super::probe::probe_package(
            "@scope/pkg",
            "1.0.0",
            &registry,
            &PublishNetwork { client: &client, auth_headers: &auth_headers },
        )
        .await
        .unwrap();
        assert!(
            matches!(result, super::probe::ProbeResult::Pending(delay) if delay >= Duration::from_secs(28)),
            "{result:?}",
        );
        request.assert_async().await;
    }
}

#[tokio::test]
async fn excessive_retry_after_cannot_exceed_the_deadline() {
    let mut server = mockito::Server::new_async().await;
    let request = server
        .mock("GET", "/@scope%2Fpkg")
        .with_status(429)
        .with_header("retry-after", "18446744073709551615")
        .expect(1)
        .create_async()
        .await;
    let start = Instant::now();
    let error = wait(&server, Duration::from_millis(100)).await.unwrap_err();
    assert!(start.elapsed() < Duration::from_secs(2), "{error}");
    assert!(error.to_string().contains("Timed out"), "{error}");
    request.assert_async().await;
}

#[tokio::test]
async fn every_package_is_probed_in_a_bounded_round_under_one_deadline() {
    let mut server = mockito::Server::new_async().await;
    let mut requests = Vec::new();
    let names = (0..8)
        .map(|index| format!("pkg-{index}"))
        .collect::<Vec<_>>();
    for name in &names {
        requests.push(
            server
                .mock("GET", format!("/{name}").as_str())
                .with_status(404)
                .expect(1)
                .create_async()
                .await,
        );
    }
    let packages = names
        .iter()
        .map(|name| (name.as_str(), "1.0.0"))
        .collect::<Vec<_>>();
    let client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let registry = parse_supported_registry_url(&server.url()).unwrap().normalized_url;
    let start = Instant::now();
    let error = wait_for_published_packages::<SilentReporter>(
        &packages,
        &registry,
        &PublishNetwork { client: &client, auth_headers: &auth_headers },
        Duration::from_millis(300),
    )
    .await
    .unwrap_err();
    assert!(start.elapsed() < Duration::from_secs(2), "one deadline for the group: {error}");
    for request in requests {
        request.assert_async().await;
    }
}

#[tokio::test]
async fn rejects_invalid_tarball_urls_without_exposing_credentials() {
    for tarball in ["file:///tmp/pkg.tgz", "http://secret:password@localhost/pkg.tgz", "not-a-url"]
    {
        let mut server = mockito::Server::new_async().await;
        let request = server
            .mock("GET", "/@scope%2Fpkg")
            .with_body(metadata(tarball))
            .expect(1)
            .create_async()
            .await;
        let error = wait(&server, Duration::from_secs(2)).await.unwrap_err();
        assert!(error.to_string().contains("dist.tarball"), "{error}");
        assert!(!error.to_string().contains("secret"), "credentials must be redacted: {error}");
        assert!(!error.to_string().contains("password"), "credentials must be redacted: {error}");
        request.assert_async().await;
    }
}

#[tokio::test]
async fn timeout_lists_only_versions_still_pending_in_an_incomplete_round() {
    let mut server = mockito::Server::new_async().await;
    let ready = server
        .mock("GET", "/ready")
        .with_body(metadata(&format!("{}/pkg.tgz", server.url())))
        .expect(1)
        .create_async()
        .await;
    let tarball = server
        .mock("HEAD", "/pkg.tgz")
        .expect(1)
        .create_async()
        .await;
    let pending = server
        .mock("GET", "/pending")
        .with_chunked_body(|writer| {
            std::thread::sleep(Duration::from_secs(2));
            let _ = writer.write_all(b"{}");
            Ok(())
        })
        .expect(1)
        .create_async()
        .await;
    let client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let registry = parse_supported_registry_url(&server.url()).unwrap().normalized_url;
    let error = wait_for_published_packages::<SilentReporter>(
        &[("ready", "1.0.0"), ("pending", "1.0.0")],
        &registry,
        &PublishNetwork { client: &client, auth_headers: &auth_headers },
        Duration::from_millis(500),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("pending@1.0.0"), "{error}");
    assert!(!error.to_string().contains("ready@1.0.0"), "{error}");
    ready.assert_async().await;
    tarball.assert_async().await;
    pending.assert_async().await;
}
