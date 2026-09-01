use pnpm_config::Config;
use serde_json::json;

use super::{DocsArgs, is_http_url};
use crate::cli_args::view::ViewError;

fn packument() -> String {
    json!({
        "name": "is-negative",
        "homepage": "https://latest.example/docs",
        "dist-tags": { "latest": "2.0.0" },
        "versions": {
            "1.0.0": {
                "name": "is-negative",
                "version": "1.0.0",
                "homepage": "https://v1.example/docs",
                "dist": { "tarball": "https://registry.example/is-negative-1.0.0.tgz" }
            },
            "2.0.0": {
                "name": "is-negative",
                "version": "2.0.0",
                "homepage": "https://v2.example/docs",
                "dist": { "tarball": "https://registry.example/is-negative-2.0.0.tgz" }
            }
        }
    })
    .to_string()
}

fn config_for(registry: &str) -> Config {
    Config { registry: format!("{registry}/"), ..Config::default() }
}

#[test]
fn test_is_http_url_valid_https() {
    assert!(is_http_url("https://example.com"));
}

#[test]
fn test_is_http_url_valid_http() {
    assert!(is_http_url("http://example.com/package"));
}

#[test]
fn test_is_http_url_empty() {
    assert!(!is_http_url(""));
}

#[test]
fn test_is_http_url_non_url() {
    assert!(!is_http_url("not-a-url"));
}

#[test]
fn test_is_http_url_ftp() {
    assert!(!is_http_url("ftp://example.com"));
}

#[test]
fn test_is_http_url_spaces() {
    assert!(!is_http_url("https://exa mple.com"));
}

#[tokio::test]
async fn requested_version_uses_its_homepage() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/is-negative")
        .with_status(200)
        .with_body(packument())
        .create_async()
        .await;
    let args = DocsArgs { package: "is-negative@1.0.0".to_string() };

    let url =
        args.documentation_url(&config_for(&server.url())).await.expect("docs URL must resolve");

    mock.assert_async().await;
    assert_eq!(url, "https://v1.example/docs");
}

#[tokio::test]
async fn missing_requested_version_fails() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/is-negative")
        .with_status(200)
        .with_body(packument())
        .create_async()
        .await;
    let args = DocsArgs { package: "is-negative@9999.0.0".to_string() };

    let error = args
        .documentation_url(&config_for(&server.url()))
        .await
        .expect_err("a missing version must fail");

    mock.assert_async().await;
    assert!(
        matches!(error.downcast_ref::<ViewError>(), Some(ViewError::PackageNotFound { .. })),
        "unexpected error: {error:?}",
    );
}
