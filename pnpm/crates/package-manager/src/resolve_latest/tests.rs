use super::{LatestPicker, ResolveLatestError};
use crate::resolution_policy::PickPolicy;
use pnpm_config::Config;
use pnpm_network::ThrottledClient;
use pnpm_resolving_npm_resolver::shared_packument_fetch_locker;
use std::sync::Arc;
use tempfile::tempdir;

/// A packument that lists `latest` and serves it an unreadable manifest.
/// `dist.integrity` decodes strictly — a tarball hash pnpm cannot read
/// is one it cannot verify — so the version is listed yet unhydratable,
/// which is exactly the state a dangling tag is indistinguishable from.
fn undecodable_latest_packument(latest: &str) -> String {
    format!(
        r#"{{
            "name": "acme",
            "dist-tags": {{ "latest": "{latest}" }},
            "time": {{ "{latest}": "2020-01-10T08:30:00.000Z" }},
            "versions": {{
                "{latest}": {{
                    "name": "acme",
                    "version": "{latest}",
                    "dist": {{
                        "tarball": "https://registry/acme.tgz",
                        "integrity": "not-an-integrity"
                    }}
                }}
            }}
        }}"#,
    )
}

async fn resolve_latest_error(latest: &str) -> ResolveLatestError {
    let dir = tempdir().expect("tempdir");
    let mut server = mockito::Server::new_async().await;
    let _packument = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(undecodable_latest_packument(latest))
        .create_async()
        .await;

    let mut config = Config::new();
    config.cache_dir = dir.path().join("cache");
    config.registry = format!("{}/", server.url());
    // A maturity cutoff routes the resolve through the package picker
    // rather than the dist-tag endpoint, which is the only path that can
    // tell an undecodable manifest from an empty tag.
    config.minimum_release_age = Some(24 * 60);

    let http_client = ThrottledClient::default();
    let policy = PickPolicy::from_config(&config).expect("derive pick policy");
    let picker = LatestPicker::new(
        &config,
        &http_client,
        policy,
        Arc::default(),
        shared_packument_fetch_locker(),
    );

    picker.resolve("acme", true).await.expect_err("latest cannot resolve")
}

#[tokio::test]
async fn an_undecodable_latest_manifest_is_reported_instead_of_an_empty_tag() {
    let error = resolve_latest_error("1.0.0").await;

    let ResolveLatestError::UndecodableLatestManifest { name, version, error } = error else {
        panic!("expected an undecodable-manifest error, got: {error}");
    };
    assert_eq!(name, "acme");
    assert_eq!(version, "1.0.0");
    assert!(error.contains("integrity"), "the error names the field pnpm choked on: {error}");
}

/// `dist-tags.latest` and the decoder's quoting of the value it rejected
/// are both the registry's text. Rendering either verbatim would let a
/// packument redraw the diagnostic with escape sequences or split it
/// across lines.
#[tokio::test]
async fn registry_text_reaches_the_diagnostic_stripped_of_control_characters() {
    let error = resolve_latest_error(r"1.0.0\u001b[31m\nnot the error").await;

    let ResolveLatestError::UndecodableLatestManifest { version, .. } = &error else {
        panic!("expected an undecodable-manifest error, got: {error}");
    };
    assert_eq!(
        version, "1.0.0[31mnot the error",
        "the version keeps its text and loses the control characters",
    );

    let rendered = error.to_string();
    assert!(
        !rendered.chars().any(char::is_control),
        "no control character survives into the diagnostic: {rendered:?}",
    );
}
