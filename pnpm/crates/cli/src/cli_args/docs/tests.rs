use std::{io, sync::Mutex};

use pnpm_config::Config;
use pnpm_network_web_auth::OpenUrl;
use pnpm_registry::PackageVersion;
use serde_json::json;

use super::{DocsArgs, documentation_url_from_manifest, is_http_url};
use crate::cli_args::view::ViewError;

fn packument() -> String {
    json!({
        "name": "is-negative",
        "homepage": "https://latest.example/docs",
        "dist-tags": { "latest": "2.0.0", "legacy": "1.0.0" },
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

static OPENED_URLS: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct RecordingBrowser;

impl OpenUrl for RecordingBrowser {
    fn open_url(url: &str) -> io::Result<()> {
        OPENED_URLS.lock().unwrap().push(url.to_owned());
        Ok(())
    }
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
    OPENED_URLS.lock().unwrap().clear();
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/is-negative")
        .with_status(200)
        .with_body(packument())
        .create_async()
        .await;
    let args = DocsArgs { package: "is-negative@1.0.0".to_string() };

    args.run::<RecordingBrowser>(&config_for(&server.url())).await.expect("docs URL must open");

    mock.assert_async().await;
    assert_eq!(OPENED_URLS.lock().unwrap().as_slice(), ["https://v1.example/docs"]);
}

#[tokio::test]
async fn unversioned_spec_uses_the_latest_tag_homepage() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/is-negative")
        .with_status(200)
        .with_body(packument())
        .create_async()
        .await;
    let args = DocsArgs { package: "is-negative".to_string() };

    let url =
        args.documentation_url(&config_for(&server.url())).await.expect("docs URL must resolve");

    mock.assert_async().await;
    assert_eq!(url, "https://v2.example/docs");
}

#[tokio::test]
async fn named_tag_uses_the_tagged_version_homepage() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/is-negative")
        .with_status(200)
        .with_body(packument())
        .create_async()
        .await;
    let args = DocsArgs { package: "is-negative@legacy".to_string() };

    let url =
        args.documentation_url(&config_for(&server.url())).await.expect("docs URL must resolve");

    mock.assert_async().await;
    assert_eq!(url, "https://v1.example/docs");
}

#[tokio::test]
async fn semver_range_uses_the_highest_matching_version_homepage() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/is-negative")
        .with_status(200)
        .with_body(packument())
        .create_async()
        .await;
    let args = DocsArgs { package: "is-negative@^1.0.0".to_string() };

    let url =
        args.documentation_url(&config_for(&server.url())).await.expect("docs URL must resolve");

    mock.assert_async().await;
    assert_eq!(url, "https://v1.example/docs");
}

#[test]
fn manifest_without_http_homepage_falls_back_to_npmx() {
    let manifest: PackageVersion = serde_json::from_value(json!({
        "name": "@scope/is-negative",
        "version": "1.0.0",
        "homepage": "git+ssh://git@example.com/docs.git",
        "dist": { "tarball": "https://registry.example/is-negative-1.0.0.tgz" }
    }))
    .expect("manifest must deserialize");

    assert_eq!(
        documentation_url_from_manifest(&manifest),
        "https://npmx.dev/package/@scope/is-negative",
    );
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
