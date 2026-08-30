use std::sync::Arc;

use pnpm_config::Config;
use pnpm_lockfile::{LockfileResolution, PkgName, TarballResolution};
use pnpm_network::ThrottledClient;
use pnpm_resolving_resolver_base::{ResolutionVerification, VerifyCtx};
use ssri::Integrity;
use tempfile::TempDir;

use super::{BuildVerifiersError, build_resolution_verifiers};

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
    let no_network = server.mock("GET", "/acme").with_status(500).expect(0).create_async().await;

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

    let result = verifiers[0]
        .verify(&resolution, VerifyCtx { name: &name, version: "1.0.0", registry_name: None })
        .await;

    let ResolutionVerification::FetchFailed { message } = result else {
        panic!("expected offline metadata failure, got {result:?}");
    };
    assert!(message.contains("ERR_PNPM_NO_OFFLINE_META"));
    no_network.assert_async().await;
}
