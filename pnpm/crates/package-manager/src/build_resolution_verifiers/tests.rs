use std::sync::Arc;

use pnpm_config::Config;
use pnpm_lockfile::{
    LockfileResolution,
    PkgName,
    TarballResolution,
};
use pnpm_network::ThrottledClient;
use pnpm_resolving_resolver_base::{
    ResolutionVerification,
    VerifyCtx,
};
use ssri::Integrity;
use tempfile::TempDir;

use super::{
    BuildVerifiersError,
    build_resolution_verifiers,
};

const FAKE_INTEGRITY: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

/// Verifiers are built before the resolver chain that also validates
/// `namedRegistries`, and on the frozen path that chain never runs, so a
/// bad name has to surface here as a diagnostic rather than a panic.
#[test]
fn reserved_named_registry_is_an_error_not_a_panic() {
    let mut config = Config::default();
    config.registries_by_prefix.insert("workspace".to_string(), "https://npm.example/".to_string());

    let result = build_resolution_verifiers(
        &config,
        Arc::new(ThrottledClient::default()),
        None,
        None,
        None,
        None,
        None,
    );

    assert!(
        matches!(result, Err(BuildVerifiersError::InvalidNamedRegistries { .. })),
        "expected a diagnostic, got {:?}",
        result.map(|verifiers| verifiers.len()),
    );
}

#[tokio::test]
async fn offline_config_threads_to_resolution_verifier() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let no_network = server
        .mock("GET", "/acme")
        .with_status(500)
        .expect(0)
        .create_async()
        .await;

    let cache_dir = TempDir::new().expect("tempdir");
    let config = Config {
        registry: registry.clone(),
        cache_dir: cache_dir.path().to_path_buf(),
        offline: true,
        ..Default::default()
    };

    let verifiers = build_resolution_verifiers(
        &config,
        Arc::new(ThrottledClient::default()),
        None,
        None,
        None,
        None,
        None,
    )
    .expect("build verifiers");
    let name: PkgName = "acme".parse().expect("parse name");
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: format!("{registry}acme/-/acme-1.0.0.tgz"),
        integrity: Some(FAKE_INTEGRITY.parse::<Integrity>().expect("parse integrity")),
        revision: None,
        git_hosted: None,
        path: None,
    });

    let result = verifiers[0].verify(
        &resolution,
        VerifyCtx { name: &name, version: "1.0.0", registry_name: None },
    )
    .await;

    let ResolutionVerification::FetchFailed { message } = result else {
        panic!("expected offline metadata failure, got {result:?}");
    };
    assert!(message.contains("ERR_PNPM_NO_OFFLINE_META"));
    no_network.assert_async().await;
}

/// Verifies a lockfile entry for `acme@1.0.0`, published at
/// 2020-01-01T12:00:00Z, under a one-day `minimumReleaseAge` counted back
/// from `now`.
async fn verify_at(now: &str) -> ResolutionVerification {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let tarball = format!("{registry}acme/-/acme-1.0.0.tgz");
    let packument = serde_json::json!({
        "name": "acme",
        "dist-tags": {"latest": "1.0.0"},
        "versions": {"1.0.0": {
            "name": "acme",
            "version": "1.0.0",
            "dist": {"integrity": FAKE_INTEGRITY, "tarball": tarball},
        }},
        "time": {"1.0.0": "2020-01-01T12:00:00.000Z"},
    });
    let _meta = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .create_async()
        .await;

    let cache_dir = TempDir::new().expect("tempdir");
    let config = Config {
        registry: registry.clone(),
        cache_dir: cache_dir.path().to_path_buf(),
        minimum_release_age: Some(24 * 60),
        reference_time: Some(pnpm_time::parse_rfc3339(now).expect("valid timestamp")),
        ..Default::default()
    };
    let verifiers = build_resolution_verifiers(
        &config,
        Arc::new(ThrottledClient::default()),
        None,
        None,
        None,
        None,
        None,
    )
    .expect("build verifiers");
    let name: PkgName = "acme".parse().expect("parse name");
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: tarball.clone(),
        integrity: Some(FAKE_INTEGRITY.parse::<Integrity>().expect("parse integrity")),
        revision: None,
        git_hosted: None,
        path: None,
    });

    verifiers[0].verify(
        &resolution,
        VerifyCtx { name: &name, version: "1.0.0", registry_name: None },
    )
    .await
}

#[tokio::test]
async fn reference_time_sets_the_cutoff() {
    let early = verify_at("2020-01-02T00:00:00Z").await;
    let ResolutionVerification::Err { code, .. } = early else {
        panic!("expected a release-age violation 12 hours after the publish, got {early:?}");
    };
    assert_eq!(code, "MINIMUM_RELEASE_AGE_VIOLATION");

    let late = verify_at("2020-01-03T00:00:00Z").await;
    assert!(
        matches!(late, ResolutionVerification::Ok),
        "expected a pass 36 hours after the publish, got {late:?}",
    );
}
