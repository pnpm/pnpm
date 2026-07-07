use std::path::Path;

use pacquet_config::{Config, TrustPolicy};

use super::resolve_pnpm_version;

/// `Accept` header the resolver sends for full metadata
/// (`ACCEPT_FULL_DOC`); only the full packument carries the per-version
/// `time` map and trust evidence the no-downgrade check reads.
const ACCEPT_FULL: &str = "application/json; q=1.0, */*";

/// `Accept` header the resolver sends for abbreviated metadata
/// (`ACCEPT_ABBREVIATED_DOC`) — what the probe would use if it did *not*
/// derive full metadata from the trust policy.
const ACCEPT_ABBREVIATED: &str =
    "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*";

/// Full `pnpm` packument: carries the per-version `time` map. `1.0.0` has
/// stronger trust evidence (`trustedPublisher` + provenance) than the
/// later `1.1.0` (none) — a trust downgrade the no-downgrade check must
/// reject once it can see `time`.
const FULL_DOWNGRADE_BODY: &str = r#"{
    "name": "pnpm",
    "dist-tags": { "latest": "1.1.0" },
    "time": {
        "1.0.0": "2024-01-10T08:30:00.000Z",
        "1.1.0": "2024-12-10T08:30:00.000Z"
    },
    "versions": {
        "1.0.0": {
            "name": "pnpm",
            "version": "1.0.0",
            "_npmUser": {
                "name": "alice",
                "trustedPublisher": { "id": "github", "oidcConfigId": "release" }
            },
            "dist": {
                "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                "shasum": "0000000000000000000000000000000000000000",
                "tarball": "https://registry/pnpm-1.0.0.tgz",
                "attestations": {
                    "provenance": { "predicateType": "https://slsa.dev/provenance/v1" }
                }
            }
        },
        "1.1.0": {
            "name": "pnpm",
            "version": "1.1.0",
            "dist": {
                "integrity": "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
                "shasum": "1111111111111111111111111111111111111111",
                "tarball": "https://registry/pnpm-1.1.0.tgz"
            }
        }
    }
}"#;

/// Full `pnpm` packument with no trust downgrade: neither version carries
/// trust evidence, and `time` is present. Resolving `^1.0.0` picks `1.1.0`
/// and the no-downgrade check passes.
const FULL_CLEAN_BODY: &str = r#"{
    "name": "pnpm",
    "dist-tags": { "latest": "1.1.0" },
    "time": {
        "1.0.0": "2024-01-10T08:30:00.000Z",
        "1.1.0": "2024-12-10T08:30:00.000Z"
    },
    "versions": {
        "1.0.0": {
            "name": "pnpm",
            "version": "1.0.0",
            "dist": {
                "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                "shasum": "0000000000000000000000000000000000000000",
                "tarball": "https://registry/pnpm-1.0.0.tgz"
            }
        },
        "1.1.0": {
            "name": "pnpm",
            "version": "1.1.0",
            "dist": {
                "integrity": "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
                "shasum": "1111111111111111111111111111111111111111",
                "tarball": "https://registry/pnpm-1.1.0.tgz"
            }
        }
    }
}"#;

/// Abbreviated packument: no `time` map. Served only to the abbreviated
/// `Accept` header, so the probe reaches it only if it *fails* to request
/// full metadata — in which case the trust check fails closed with
/// "missing time". A test that instead sees a "trust downgrade" (or a
/// clean resolve) proves the probe requested full metadata.
const ABBREVIATED_BODY: &str = r#"{
    "name": "pnpm",
    "dist-tags": { "latest": "1.1.0" },
    "versions": {
        "1.0.0": {
            "name": "pnpm",
            "version": "1.0.0",
            "dist": {
                "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                "shasum": "0000000000000000000000000000000000000000",
                "tarball": "https://registry/pnpm-1.0.0.tgz"
            }
        },
        "1.1.0": {
            "name": "pnpm",
            "version": "1.1.0",
            "dist": {
                "integrity": "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
                "shasum": "1111111111111111111111111111111111111111",
                "tarball": "https://registry/pnpm-1.1.0.tgz"
            }
        }
    }
}"#;

fn no_downgrade_config(registry: String, cache_dir: &Path) -> Config {
    let mut config = Config {
        trust_policy: TrustPolicy::NoDowngrade,
        cache_dir: cache_dir.to_path_buf(),
        ..Config::default()
    };
    config.package_manager_bootstrap.registry = registry;
    config
}

/// Under `trustPolicy=no-downgrade`, the self-update probe must resolve
/// against **full** metadata — the same as a regular install — so the
/// no-downgrade check actually runs. It reaches the full packument (with
/// `time`), sees the downgrade, and rejects it. If the probe fetched
/// abbreviated metadata it would instead fail closed with "missing time";
/// if it skipped the check it would resolve `1.1.0` with no error.
#[tokio::test]
async fn resolve_pnpm_version_fetches_full_metadata_and_rejects_a_downgrade() {
    let mut server = mockito::Server::new_async().await;
    let _full = server
        .mock("GET", "/pnpm")
        .match_header("accept", ACCEPT_FULL)
        .with_status(200)
        .with_body(FULL_DOWNGRADE_BODY)
        .create_async()
        .await;
    let _abbreviated = server
        .mock("GET", "/pnpm")
        .match_header("accept", ACCEPT_ABBREVIATED)
        .with_status(200)
        .with_body(ABBREVIATED_BODY)
        .create_async()
        .await;
    let cache_dir = tempfile::TempDir::new().expect("cache tempdir");
    let config = no_downgrade_config(format!("{}/", server.url()), cache_dir.path());

    let err = resolve_pnpm_version(&config, "^1.0.0")
        .await
        .expect_err("a trust downgrade must be rejected");
    let report = format!("{err:?}");
    assert!(
        report.contains("trust downgrade"),
        "expected a trust-downgrade rejection, got: {report}",
    );
    assert!(
        !report.contains("missing time"),
        "the probe must fetch full metadata, not fail closed on abbreviated: {report}",
    );
}

/// A normal (non-downgrade) target still resolves under
/// `trustPolicy=no-downgrade`: the probe fetches full metadata, the check
/// passes, and it picks the highest in range.
#[tokio::test]
async fn resolve_pnpm_version_resolves_a_clean_update_under_no_downgrade() {
    let mut server = mockito::Server::new_async().await;
    let _full = server
        .mock("GET", "/pnpm")
        .match_header("accept", ACCEPT_FULL)
        .with_status(200)
        .with_body(FULL_CLEAN_BODY)
        .create_async()
        .await;
    let _abbreviated = server
        .mock("GET", "/pnpm")
        .match_header("accept", ACCEPT_ABBREVIATED)
        .with_status(200)
        .with_body(ABBREVIATED_BODY)
        .create_async()
        .await;
    let cache_dir = tempfile::TempDir::new().expect("cache tempdir");
    let config = no_downgrade_config(format!("{}/", server.url()), cache_dir.path());

    let resolved = resolve_pnpm_version(&config, "^1.0.0")
        .await
        .expect("a clean update must resolve")
        .expect("a matching pnpm version resolves");

    assert_eq!(resolved.version, "1.1.0");
    assert!(!resolved.policy_violation);
}
