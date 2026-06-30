//! Tests for the production [`Host`] capability impls.
//!
//! These cover only the two impls that carry real branching and can be
//! exercised portably without mutating process-global state: [`OidcFetch`]
//! (driven against a `mockito` server) and [`RunCommand`] (a real
//! subprocess). The remaining impls are deliberately untested here — `EnvVar`,
//! `CiInfo`, and `Clock` are one-line passes through to `std::env` /
//! `SystemTime` whose only test seam is `env::set_var` / a wall clock (the
//! shared-global hazard the `Sys` dependency-injection seam exists to avoid),
//! and `ConfirmPrompt` reads an interactive TTY. Their consumers are covered
//! through fake `Sys` providers instead.

use super::{Host, OidcFetch, OidcMethod, OidcRequest, RunCommand};

#[tokio::test]
async fn fetch_get_returns_the_response_and_sends_accept_auth_and_timeout() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/token")
        .match_header("accept", "application/json")
        .match_header("authorization", "Bearer gh-request-token")
        .with_status(200)
        .with_body(r#"{"value":"id-token"}"#)
        .create_async()
        .await;
    let url = format!("{}/token", server.url());

    let response = Host::fetch(OidcRequest {
        method: OidcMethod::Get,
        url: &url,
        authorization: "Bearer gh-request-token",
        timeout_ms: Some(5_000),
    })
    .await
    .expect("the request reaches the mock server");

    assert!(response.ok);
    assert_eq!(response.status, 200);
    assert_eq!(response.body, r#"{"value":"id-token"}"#);
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_post_sends_a_zero_length_body() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/exchange")
        .match_header("content-length", "0")
        .match_header("authorization", "Bearer id-token")
        .with_status(201)
        .with_body(r#"{"token":"registry-token"}"#)
        .create_async()
        .await;
    let url = format!("{}/exchange", server.url());

    let response = Host::fetch(OidcRequest {
        method: OidcMethod::Post,
        url: &url,
        authorization: "Bearer id-token",
        timeout_ms: None,
    })
    .await
    .expect("the request reaches the mock server");

    assert!(response.ok);
    assert_eq!(response.status, 201);
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_reports_a_non_success_status_without_erroring() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/token")
        .with_status(403)
        .with_body("forbidden")
        .create_async()
        .await;
    let url = format!("{}/token", server.url());

    let response = Host::fetch(OidcRequest {
        method: OidcMethod::Get,
        url: &url,
        authorization: "Bearer t",
        timeout_ms: None,
    })
    .await
    .expect("a 403 is a completed response, not a transport failure");

    assert!(!response.ok);
    assert_eq!(response.status, 403);
    assert_eq!(response.body, "forbidden");
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_maps_a_transport_failure_to_an_error() {
    // Port 1 refuses the connection, so the request never produces a response.
    let error = Host::fetch(OidcRequest {
        method: OidcMethod::Get,
        url: "http://127.0.0.1:1/token",
        authorization: "Bearer t",
        timeout_ms: None,
    })
    .await
    .expect_err("a refused connection is a transport error");

    assert!(!error.reason.is_empty());
}

#[cfg(unix)]
#[test]
fn run_captures_stdout_and_success() {
    let output = Host::run("sh", &["-c", "printf hello"], None).expect("sh runs");
    assert!(output.success);
    assert_eq!(output.stdout, "hello");
}

#[cfg(unix)]
#[test]
fn run_reports_a_non_zero_exit_as_failure() {
    let output = Host::run("sh", &["-c", "exit 3"], None).expect("sh runs");
    assert!(!output.success);
}

#[cfg(unix)]
#[test]
fn run_executes_in_the_given_cwd() {
    let dir = tempfile::tempdir().expect("a temp dir");
    std::fs::write(dir.path().join("marker.txt"), "in-cwd").expect("write the marker");

    let output =
        Host::run("sh", &["-c", "cat marker.txt"], Some(dir.path())).expect("sh runs in the cwd");

    assert!(output.success);
    assert_eq!(output.stdout, "in-cwd");
}

#[cfg(unix)]
#[test]
fn run_surfaces_a_missing_program_as_an_io_error() {
    let result = Host::run("pacquet-no-such-binary-xyzzy", &[], None);
    assert!(result.is_err());
}
