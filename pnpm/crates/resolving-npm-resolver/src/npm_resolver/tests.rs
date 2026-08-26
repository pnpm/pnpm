use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::TimeZone;
use pnpm_config::{TrustPolicy, version_policy::create_package_version_policy};
use pnpm_lockfile::{LockfileResolution, RegistryResolution, TarballRevision};
use pnpm_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pnpm_resolving_resolver_base::{
    CurrentPkg, LatestQuery, PackageVersionGuard, PackageVersionGuardDecision,
    PackageVersionGuardFuture, PkgResolutionId, ResolveOptions, Resolver, UpdateBehavior,
    WantedDependency, WorkspacePackage, WorkspacePackages, WorkspacePackagesByVersion,
};
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

use crate::{
    errors::{
        InvalidRevisionSpecifierError, InvalidTarballIntegrityError, MalformedRevisionHistoryError,
        NoMatchingRevisionError,
    },
    npm_resolver::{NpmResolver, is_not_found_error},
    pick_package::{
        InMemoryPackageMetaCache, shared_packument_fetch_locker, shared_picked_manifest_cache,
    },
    resolve_from_workspace::ResolveFromWorkspaceError,
    violation_codes::MINIMUM_RELEASE_AGE_VIOLATION_CODE,
};

const PACKAGE_BODY: &str = r#"{
    "name": "acme",
    "dist-tags": { "latest": "1.1.0" },
    "modified": "2025-01-15T12:00:00.000Z",
    "time": {
        "1.0.0-canary.1": "2024-01-05T08:30:00.000Z",
        "1.0.0": "2024-01-10T08:30:00.000Z",
        "1.1.0": "2024-12-10T08:30:00.000Z"
    },
    "versions": {
        "1.0.0-canary.1": {
            "name": "acme",
            "version": "1.0.0-canary.1",
            "dist": {
                "integrity": "sha512-EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE==",
                "shasum": "4444444444444444444444444444444444444444",
                "tarball": "https://registry/acme-1.0.0-canary.1.tgz"
            }
        },
        "1.0.0": {
            "name": "acme",
            "version": "1.0.0",
            "dist": {
                "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                "shasum": "0000000000000000000000000000000000000000",
                "tarball": "https://registry/acme-1.0.0.tgz"
            }
        },
        "1.1.0": {
            "name": "acme",
            "version": "1.1.0",
            "dist": {
                "integrity": "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
                "shasum": "1111111111111111111111111111111111111111",
                "tarball": "https://registry/acme-1.1.0.tgz"
            }
        }
    }
}"#;

fn build_resolver(registry: &str) -> (NpmResolver<InMemoryPackageMetaCache>, TempDir) {
    let mut registries = HashMap::new();
    registries.insert("default".to_string(), registry.to_string());
    build_resolver_with_registries(registries)
}

fn build_resolver_with_registries(
    registries: HashMap<String, String>,
) -> (NpmResolver<InMemoryPackageMetaCache>, TempDir) {
    let cache_dir = TempDir::new().expect("tempdir");
    let resolver = NpmResolver {
        registries,
        registries_by_prefix: HashMap::new(),
        http_client: Arc::new(ThrottledClient::default()),
        auth_headers: Arc::new(AuthHeaders::default()),
        meta_cache: Arc::new(InMemoryPackageMetaCache::default()),
        fetch_locker: shared_packument_fetch_locker(),
        picked_manifest_cache: shared_picked_manifest_cache(),
        cache_dir: Some(cache_dir.path().to_path_buf()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: false,
        needs_full_metadata_for: None,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };
    (resolver, cache_dir)
}

#[derive(Debug)]
struct RejectVersions {
    versions: HashSet<String>,
}

impl PackageVersionGuard for RejectVersions {
    fn check<'a>(&'a self, _name: &'a str, version: &'a str) -> PackageVersionGuardFuture<'a> {
        Box::pin(async move {
            if self.versions.contains(version) {
                Ok(PackageVersionGuardDecision::Reject { reason: format!("{version} is blocked") })
            } else {
                Ok(PackageVersionGuardDecision::Allow)
            }
        })
    }
}

fn reject_versions(versions: &[&str]) -> Arc<dyn PackageVersionGuard> {
    Arc::new(RejectVersions {
        versions: versions.iter().map(|version| (*version).to_string()).collect(),
    })
}

/// Packument body for `@jsr/foo__bar` — the npm-shaped name JSR
/// serves `@foo/bar`.
const JSR_PACKAGE_BODY: &str = r#"{
    "name": "@jsr/foo__bar",
    "dist-tags": { "latest": "1.1.0" },
    "modified": "2025-01-15T12:00:00.000Z",
    "time": {
        "1.0.0": "2024-01-10T08:30:00.000Z",
        "1.1.0": "2024-12-10T08:30:00.000Z"
    },
    "versions": {
        "1.0.0": {
            "name": "@jsr/foo__bar",
            "version": "1.0.0",
            "dist": {
                "integrity": "sha512-CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC==",
                "shasum": "2222222222222222222222222222222222222222",
                "tarball": "https://registry/foo__bar-1.0.0.tgz"
            }
        },
        "1.1.0": {
            "name": "@jsr/foo__bar",
            "version": "1.1.0",
            "dist": {
                "integrity": "sha512-DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD==",
                "shasum": "3333333333333333333333333333333333333333",
                "tarball": "https://registry/foo__bar-1.1.0.tgz"
            }
        }
    }
}"#;

/// Packument where the earlier-published `1.0.0` carries the strongest
/// trust evidence available here (`trustedPublisher` + provenance) and
/// the later `1.1.0` carries none — a trust downgrade. Resolving
/// `^1.0.0` picks `1.1.0` (the max), so the resolver-time gate must
/// reject it under `trustPolicy='no-downgrade'`.
const TRUST_DOWNGRADE_PACKAGE_BODY: &str = r#"{
    "name": "acme",
    "dist-tags": { "latest": "1.1.0" },
    "modified": "2025-01-15T12:00:00.000Z",
    "time": {
        "1.0.0": "2024-01-10T08:30:00.000Z",
        "1.1.0": "2024-12-10T08:30:00.000Z"
    },
    "versions": {
        "1.0.0": {
            "name": "acme",
            "version": "1.0.0",
            "_npmUser": {
                "name": "alice",
                "trustedPublisher": { "id": "github", "oidcConfigId": "release" }
            },
            "dist": {
                "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                "shasum": "0000000000000000000000000000000000000000",
                "tarball": "https://registry/acme-1.0.0.tgz",
                "attestations": {
                    "provenance": { "predicateType": "https://slsa.dev/provenance/v1" }
                }
            }
        },
        "1.1.0": {
            "name": "acme",
            "version": "1.1.0",
            "dist": {
                "integrity": "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
                "shasum": "1111111111111111111111111111111111111111",
                "tarball": "https://registry/acme-1.1.0.tgz"
            }
        }
    }
}"#;

#[tokio::test]
async fn range_specifier_picks_max_in_range() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();
    let name_ver = result.name_ver.as_ref().expect("npm resolver fills name_ver");
    assert_eq!(name_ver.name.to_string(), "acme");
    assert_eq!(name_ver.suffix.to_string(), "1.1.0");
    assert_eq!(result.id.as_str(), "acme@1.1.0");
    assert_eq!(result.latest.as_deref(), Some("1.1.0"));
    assert_eq!(result.resolved_via, "npm-registry");
    assert_eq!(result.alias.as_deref(), Some("acme"));
    assert!(result.policy_violation.is_none());
    assert!(matches!(result.resolution, LockfileResolution::Tarball(_)));
}

#[tokio::test]
async fn empty_specifier_resolves_to_the_max_published_version() {
    // Regression test for pnpm/pnpm#13673.
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some(String::new()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();
    assert_eq!(result.id.as_str(), "acme@1.1.0");
    assert_eq!(result.resolved_via, "npm-registry");
}

/// Regression test for pnpm/pnpm#14096: npm strips build metadata when
/// it publishes a version, so a selector carrying it must still resolve
/// to the published version — for a prerelease as much as for a stable
/// release.
#[tokio::test]
async fn exact_version_with_build_metadata_resolves_to_the_published_version() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    for (bare_specifier, expected_id) in
        [("1.0.0+build1", "acme@1.0.0"), ("1.0.0-canary.1+build1", "acme@1.0.0-canary.1")]
    {
        let wanted = WantedDependency {
            alias: Some("acme".to_string()),
            bare_specifier: Some(bare_specifier.to_string()),
            ..WantedDependency::default()
        };
        let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();
        assert_eq!(result.id.as_str(), expected_id, "for {bare_specifier:?}");
        assert_eq!(result.resolved_via, "npm-registry", "for {bare_specifier:?}");
    }
}

#[tokio::test]
async fn package_version_guard_excludes_rejected_versions_and_repicks() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let opts = ResolveOptions {
        package_version_guard: Some(reject_versions(&["1.1.0"])),
        ..ResolveOptions::default()
    };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };

    let result = resolver.resolve(&wanted, &opts).await.unwrap().unwrap();
    let name_ver = result.name_ver.as_ref().expect("name_ver");
    assert_eq!(name_ver.suffix.to_string(), "1.0.0");
    assert_eq!(result.latest.as_deref(), Some("1.0.0"));
}

#[tokio::test]
async fn package_version_guard_repopulates_latest_tag() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let opts = ResolveOptions {
        package_version_guard: Some(reject_versions(&["1.1.0"])),
        ..ResolveOptions::default()
    };
    let wanted =
        WantedDependency { alias: Some("acme".to_string()), ..WantedDependency::default() };

    let result = resolver.resolve(&wanted, &opts).await.unwrap().unwrap();
    assert_eq!(result.name_ver.as_ref().expect("name_ver").suffix.to_string(), "1.0.0");
    assert_eq!(result.latest.as_deref(), Some("1.0.0"));
}

#[tokio::test]
async fn package_version_guard_blocking_every_version_errors() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let opts = ResolveOptions {
        package_version_guard: Some(reject_versions(&["1.0.0", "1.1.0"])),
        ..ResolveOptions::default()
    };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };

    // Every matching version is rejected, so the resolver must surface a
    // clear guard error rather than Ok(None) (which would read as an
    // unsupported spec downstream).
    let err = resolver.resolve(&wanted, &opts).await.expect_err("expected a guard error");
    let message = err.to_string();
    assert!(message.contains("acme"), "{message}");
    assert!(message.contains("rejected by the resolver guard"), "{message}");
}

/// Packument whose `1.5.0+build` key carries a manifest `version` of
/// `1.5.0` — i.e. the version-map key differs from the parsed manifest
/// version, the case a malformed/malicious registry can produce.
const MISMATCHED_KEY_BODY: &str = r#"{
    "name": "acme",
    "dist-tags": { "latest": "1.5.0+build" },
    "modified": "2025-01-15T12:00:00.000Z",
    "time": {
        "1.0.0": "2024-01-10T08:30:00.000Z",
        "1.5.0+build": "2024-12-10T08:30:00.000Z"
    },
    "versions": {
        "1.0.0": {
            "name": "acme",
            "version": "1.0.0",
            "dist": {
                "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                "shasum": "0000000000000000000000000000000000000000",
                "tarball": "https://registry/acme-1.0.0.tgz"
            }
        },
        "1.5.0+build": {
            "name": "acme",
            "version": "1.5.0",
            "dist": {
                "integrity": "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
                "shasum": "1111111111111111111111111111111111111111",
                "tarball": "https://registry/acme-1.5.0.tgz"
            }
        }
    }
}"#;

#[tokio::test]
async fn package_version_guard_blocks_the_packument_key_not_the_parsed_version() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(MISMATCHED_KEY_BODY)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    // The guard rejects the parsed manifest version `1.5.0`, whose
    // packument key is `1.5.0+build`. The repick must still exclude that
    // entry and fall back to `1.0.0`, rather than wrongly reporting that
    // every version is blocked.
    let opts = ResolveOptions {
        package_version_guard: Some(reject_versions(&["1.5.0"])),
        ..ResolveOptions::default()
    };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };

    let result = resolver.resolve(&wanted, &opts).await.unwrap().unwrap();
    assert_eq!(result.name_ver.as_ref().expect("name_ver").suffix.to_string(), "1.0.0");
}

#[tokio::test]
async fn workspace_path_form_falls_through_to_local_resolver() {
    let server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("workspace:./acme".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn workspace_version_without_workspace_packages_surfaces_error() {
    let server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("workspace:*".to_string()),
        ..WantedDependency::default()
    };
    let err = resolver
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .expect_err("workspace_packages must be populated for workspace: specifiers");
    let message = err.to_string();
    assert!(
        message.contains("workspace packages were not loaded"),
        "unexpected error message: {message}",
    );
}

#[tokio::test]
async fn missing_bare_specifier_synthesizes_default_tag_query() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted =
        WantedDependency { alias: Some("acme".to_string()), ..WantedDependency::default() };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();
    assert_eq!(result.name_ver.as_ref().expect("name_ver").suffix.to_string(), "1.1.0");
}

#[tokio::test]
async fn surfaces_min_release_age_violation_inline() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    // Cutoff sits between 1.0.0 (2024-01-10) and 1.1.0 (2024-12-10):
    // the picker should fall back to 1.0.0 as the highest mature
    // version and the picked result should *not* trip a violation.
    // To force a violation we set the cutoff before both versions.
    let published_by = Some(chrono::Utc.with_ymd_and_hms(2023, 12, 1, 0, 0, 0).unwrap());
    let opts = ResolveOptions { published_by, ..ResolveOptions::default() };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().unwrap();
    let violation = result.policy_violation.expect("violation surfaced");
    assert_eq!(violation.code, MINIMUM_RELEASE_AGE_VIOLATION_CODE);
}

#[tokio::test]
async fn trust_downgrade_at_resolve_time_fails_under_no_downgrade() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(TRUST_DOWNGRADE_PACKAGE_BODY)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let opts = ResolveOptions {
        trust_policy: Some(TrustPolicy::NoDowngrade),
        ..ResolveOptions::default()
    };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let err = resolver.resolve(&wanted, &opts).await.expect_err("trust downgrade should fail");
    assert!(err.to_string().contains("trust downgrade"), "got {err}");
}

/// [`TRUST_DOWNGRADE_PACKAGE_BODY`] as a registry that strips the
/// per-version `time` field serves it.
fn trust_downgrade_body_without_time() -> String {
    let mut body: serde_json::Value =
        serde_json::from_str(TRUST_DOWNGRADE_PACKAGE_BODY).expect("parse fixture packument");
    body.as_object_mut().expect("packument is an object").remove("time");
    body.to_string()
}

#[tokio::test]
async fn trust_check_fails_at_resolve_time_when_the_registry_serves_no_time_field() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(trust_downgrade_body_without_time())
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let opts = ResolveOptions {
        trust_policy: Some(TrustPolicy::NoDowngrade),
        ..ResolveOptions::default()
    };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let err = resolver.resolve(&wanted, &opts).await.expect_err("missing time should fail closed");
    assert!(err.to_string().contains(r#"missing the "time" field"#), "got {err}");
}

#[tokio::test]
async fn trust_check_skipped_at_resolve_time_when_missing_time_is_ignored() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(trust_downgrade_body_without_time())
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (mut resolver, _tempdir) = build_resolver(&registry);
    resolver.ignore_missing_time_field = true;

    let opts = ResolveOptions {
        trust_policy: Some(TrustPolicy::NoDowngrade),
        ..ResolveOptions::default()
    };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().unwrap();
    assert_eq!(result.name_ver.as_ref().expect("name_ver").suffix.to_string(), "1.1.0");
}

#[tokio::test]
async fn trust_downgrade_ignored_when_trust_policy_off() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(TRUST_DOWNGRADE_PACKAGE_BODY)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();
    assert_eq!(result.name_ver.as_ref().expect("name_ver").suffix.to_string(), "1.1.0");
}

#[tokio::test]
async fn resolve_latest_returns_picked_manifest() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let query = LatestQuery {
        wanted_dependency: WantedDependency {
            alias: Some("acme".to_string()),
            bare_specifier: Some("^1.0.0".to_string()),
            ..WantedDependency::default()
        },
        compatible: false,
    };
    let info = resolver
        .resolve_latest(&query, &ResolveOptions::default())
        .await
        .unwrap()
        .expect("latest info");
    let manifest = info.latest_manifest.expect("manifest present");
    assert_eq!(manifest["version"].as_str(), Some("1.1.0"));
}

#[tokio::test]
async fn resolve_latest_under_compatible_does_not_override_update_to_latest() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let query = LatestQuery {
        wanted_dependency: WantedDependency {
            alias: Some("acme".to_string()),
            bare_specifier: Some("^1.0.0".to_string()),
            ..WantedDependency::default()
        },
        compatible: true,
    };
    let opts = ResolveOptions { update: UpdateBehavior::Off, ..ResolveOptions::default() };
    let info = resolver.resolve_latest(&query, &opts).await.unwrap().expect("latest info");
    let manifest = info.latest_manifest.expect("manifest present");
    assert_eq!(manifest["version"].as_str(), Some("1.1.0"));
}

#[tokio::test]
async fn jsr_specifier_routes_through_jsr_registry() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/@jsr%2Ffoo__bar")
        .with_status(200)
        .with_body(JSR_PACKAGE_BODY)
        .create_async()
        .await;
    let jsr_registry = format!("{}/", server.url());
    let mut registries = HashMap::new();
    registries.insert("default".to_string(), "https://registry.npmjs.org/".to_string());
    registries.insert("@jsr".to_string(), jsr_registry);
    let (resolver, _tempdir) = build_resolver_with_registries(registries);

    let wanted = WantedDependency {
        alias: Some("@foo/bar".to_string()),
        bare_specifier: Some("jsr:@foo/bar@^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();
    let name_ver = result.name_ver.as_ref().expect("npm resolver fills name_ver");
    assert_eq!(name_ver.name.to_string(), "@jsr/foo__bar");
    assert_eq!(name_ver.suffix.to_string(), "1.1.0");
    assert_eq!(result.resolved_via, "jsr-registry");
    assert_eq!(result.alias.as_deref(), Some("@foo/bar"));
    assert_eq!(result.latest.as_deref(), Some("1.1.0"));
    assert!(matches!(result.resolution, LockfileResolution::Tarball(_)));
}

#[tokio::test]
async fn jsr_specifier_without_selector_uses_default_tag() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/@jsr%2Ffoo__bar")
        .with_status(200)
        .with_body(JSR_PACKAGE_BODY)
        .create_async()
        .await;
    let jsr_registry = format!("{}/", server.url());
    let mut registries = HashMap::new();
    registries.insert("default".to_string(), "https://registry.npmjs.org/".to_string());
    registries.insert("@jsr".to_string(), jsr_registry);
    let (resolver, _tempdir) = build_resolver_with_registries(registries);

    let wanted = WantedDependency {
        alias: Some("@foo/bar".to_string()),
        bare_specifier: Some("jsr:@foo/bar".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();
    assert_eq!(
        result.name_ver.as_ref().expect("npm resolver fills name_ver").suffix.to_string(),
        "1.1.0",
    );
    assert_eq!(result.resolved_via, "jsr-registry");
}

/// `optionalDependencies` and `peerDependenciesMeta` round-trip from the
/// registry's per-version manifest into [`ResolveResult::manifest`]
/// (a [`serde_json::Value`]). Downstream
/// `extract_children` reads the optional-dep edges and
/// `extract_peer_dependencies` reads the per-peer `optional` flag;
/// dropping either field silently treats optional peers as required
/// (so `autoInstallPeers` cascades them in) and skips
/// `optionalDependencies` entirely. See pnpm/pnpm#11934.
#[tokio::test]
async fn resolved_manifest_carries_optional_dependencies_and_peer_dependencies_meta() {
    const BODY: &str = r#"{
        "name": "consumer",
        "dist-tags": { "latest": "1.0.0" },
        "modified": "2025-01-15T12:00:00.000Z",
        "versions": {
            "1.0.0": {
                "name": "consumer",
                "version": "1.0.0",
                "peerDependencies": {
                    "@vercel/kv": "^1 || ^2 || ^3",
                    "ioredis": "^5.4.2"
                },
                "peerDependenciesMeta": {
                    "@vercel/kv": { "optional": true },
                    "ioredis": { "optional": true }
                },
                "optionalDependencies": {
                    "sharp": "^0.34.0"
                },
                "dist": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": "https://registry/consumer-1.0.0.tgz"
                }
            }
        }
    }"#;

    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/consumer").with_status(200).with_body(BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted = WantedDependency {
        alias: Some("consumer".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();
    let manifest = result.manifest.as_ref().expect("npm resolver populates manifest");

    let optional = manifest
        .get("optionalDependencies")
        .and_then(serde_json::Value::as_object)
        .expect("optionalDependencies present");
    assert_eq!(optional.get("sharp").and_then(serde_json::Value::as_str), Some("^0.34.0"));

    let peer_meta = manifest
        .get("peerDependenciesMeta")
        .and_then(serde_json::Value::as_object)
        .expect("peerDependenciesMeta present");
    assert_eq!(
        peer_meta
            .get("@vercel/kv")
            .and_then(|v| v.get("optional"))
            .and_then(serde_json::Value::as_bool),
        Some(true),
    );
    assert_eq!(
        peer_meta
            .get("ioredis")
            .and_then(|v| v.get("optional"))
            .and_then(serde_json::Value::as_bool),
        Some(true),
    );
}

#[tokio::test]
async fn jsr_specifier_with_invalid_scope_propagates_parser_error() {
    let server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted = WantedDependency {
        alias: Some("foo".to_string()),
        bare_specifier: Some("jsr:foo@^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let err = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap_err();
    let msg = err.to_string();
    // Asserting the error message ties the test to the public
    // `ERR_PNPM_MISSING_JSR_PACKAGE_SCOPE` contract; the resolver seam
    // returns the parser error as a boxed `dyn Error` so we can't
    // downcast to the variant directly.
    assert_eq!(msg, "Package names from JSR must have a scope", "unexpected error message: {msg}");
}

/// Two `NpmResolvers` pointing at different registries, sharing the
/// same `picked_manifest_cache`, must not hand each other the
/// other's manifest when both happen to pick `acme@1.0.0`. Two
/// registries can serve different artifacts under the same
/// `name@version` (a public + private package collision, or a
/// fork), and collapsing the cache key to `name@version` alone
/// would propagate one registry's manifest into the other
/// resolver's `ResolveResult`, breaking the downstream dependency
/// graph / peer extraction / lockfile metadata.
#[tokio::test]
async fn shared_manifest_cache_does_not_leak_across_registries() {
    fn body_with_dep(dep_name: &str, dep_range: &str) -> String {
        format!(
            r#"{{
                "name": "acme",
                "dist-tags": {{ "latest": "1.0.0" }},
                "modified": "2025-01-15T12:00:00.000Z",
                "time": {{ "1.0.0": "2024-01-10T08:30:00.000Z" }},
                "versions": {{
                    "1.0.0": {{
                        "name": "acme",
                        "version": "1.0.0",
                        "dependencies": {{ "{dep_name}": "{dep_range}" }},
                        "dist": {{
                            "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                            "shasum": "0000000000000000000000000000000000000000",
                            "tarball": "https://registry/acme-1.0.0.tgz"
                        }}
                    }}
                }}
            }}"#,
        )
    }

    let mut server_a = mockito::Server::new_async().await;
    let _mock_a = server_a
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(body_with_dep("left-pad", "^1.0.0"))
        .create_async()
        .await;
    let mut server_b = mockito::Server::new_async().await;
    let _mock_b = server_b
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(body_with_dep("right-pad", "^2.0.0"))
        .create_async()
        .await;

    // Shared cache — the leak path. The fix is the cache key
    // including the registry; without it, whichever resolver runs
    // second would return the other's manifest.
    let shared_picked_cache = shared_picked_manifest_cache();
    let shared_fetch_locker = shared_packument_fetch_locker();

    let make_resolver = |registry: String| -> (NpmResolver<InMemoryPackageMetaCache>, TempDir) {
        let mut registries = HashMap::new();
        registries.insert("default".to_string(), registry);
        let cache_dir = TempDir::new().expect("tempdir");
        let resolver = NpmResolver {
            registries,
            registries_by_prefix: HashMap::new(),
            http_client: Arc::new(ThrottledClient::default()),
            auth_headers: Arc::new(AuthHeaders::default()),
            meta_cache: Arc::new(InMemoryPackageMetaCache::default()),
            fetch_locker: Arc::clone(&shared_fetch_locker),
            picked_manifest_cache: Arc::clone(&shared_picked_cache),
            cache_dir: Some(cache_dir.path().to_path_buf()),
            offline: false,
            prefer_offline: false,
            ignore_missing_time_field: false,
            full_metadata: false,
            needs_full_metadata_for: None,
            filter_metadata: false,
            retry_opts: RetryOpts::default(),
        };
        (resolver, cache_dir)
    };

    let (resolver_a, _cache_dir_a) = make_resolver(format!("{}/", server_a.url()));
    let (resolver_b, _cache_dir_b) = make_resolver(format!("{}/", server_b.url()));

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("1.0.0".to_string()),
        ..WantedDependency::default()
    };

    let result_a = resolver_a
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .expect("resolver A")
        .expect("resolver A picks");
    let result_b = resolver_b
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .expect("resolver B")
        .expect("resolver B picks");

    let deps_a = result_a
        .manifest
        .as_ref()
        .and_then(|m| m.get("dependencies"))
        .and_then(|d| d.as_object())
        .expect("resolver A manifest carries dependencies");
    let deps_b = result_b
        .manifest
        .as_ref()
        .and_then(|m| m.get("dependencies"))
        .and_then(|d| d.as_object())
        .expect("resolver B manifest carries dependencies");

    assert!(deps_a.contains_key("left-pad"), "resolver A keeps its own manifest: {deps_a:?}");
    assert!(
        deps_b.contains_key("right-pad"),
        "resolver B got its own manifest, not resolver A's: {deps_b:?}",
    );
    assert!(
        !deps_b.contains_key("left-pad"),
        "resolver B must not see resolver A's `left-pad`: {deps_b:?}",
    );
}

fn single_version_body(version: &str, integrity: &str) -> String {
    format!(
        r#"{{
            "name": "acme",
            "dist-tags": {{ "latest": "{version}" }},
            "modified": "2025-01-15T12:00:00.000Z",
            "time": {{ "{version}": "2024-01-10T08:30:00.000Z" }},
            "versions": {{
                "{version}": {{
                    "name": "acme",
                    "version": "{version}",
                    "dist": {{
                        "integrity": "{integrity}",
                        "shasum": "0000000000000000000000000000000000000000",
                        "tarball": "https://registry/acme-{version}.tgz"
                    }}
                }}
            }}
        }}"#,
    )
}

fn build_workspace_packages(name: &str, versions: &[&str]) -> WorkspacePackages {
    let mut by_version: WorkspacePackagesByVersion = BTreeMap::new();
    for version in versions {
        by_version.insert(
            (*version).to_string(),
            WorkspacePackage {
                root_dir: PathBuf::from(format!("/repo/packages/{name}")),
                manifest: json!({ "name": name, "version": version }),
            },
        );
    }
    let mut packages: WorkspacePackages = BTreeMap::new();
    packages.insert(name.to_string(), by_version);
    packages
}

fn workspace_resolve_options(packages: WorkspacePackages) -> ResolveOptions {
    ResolveOptions {
        project_dir: Path::new("/repo/packages/consumer").to_path_buf(),
        lockfile_dir: Path::new("/repo").to_path_buf(),
        workspace_packages: Some(std::sync::Arc::new(packages)),
        always_try_workspace_packages: true,
        ..ResolveOptions::default()
    }
}

/// The case behind [#11929] (babylon's `@dev/build-tools` isn't on
/// npm, so bare-semver must resolve via the workspace).
///
/// [#11929]: https://github.com/pnpm/pnpm/issues/11929
#[tokio::test]
async fn falls_back_to_workspace_when_registry_returns_404() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server.mock("GET", "/acme").with_status(404).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace fallback");
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.id.as_str(), "link:../acme");
    match &result.resolution {
        LockfileResolution::Directory(dir) => assert_eq!(dir.directory, "../acme"),
        other => panic!("expected directory resolution, got {other:?}"),
    }
}

#[tokio::test]
async fn revision_qualified_selector_does_not_fall_back_to_workspace() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server.mock("GET", "/acme").with_status(404).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("1.0.0+r1".to_string()),
        ..WantedDependency::default()
    };
    let err = resolver
        .resolve(&wanted, &opts)
        .await
        .expect_err("a registry revision cannot resolve to an unversioned workspace artifact");
    assert!(is_not_found_error(err.as_ref()), "expected registry 404, got: {err}");
}

#[tokio::test]
async fn revision_refresh_preserves_an_implicit_workspace_resolution() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server.mock("GET", "/acme").with_status(404).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let mut opts = workspace_resolve_options(packages);
    opts.update = UpdateBehavior::Patches;

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace fallback");
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.id.as_str(), "link:../acme");
}

#[tokio::test]
async fn revision_refresh_does_not_replace_a_registry_resolution_with_a_workspace_package() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(single_version_body(
            "1.0.0",
            "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
        ))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let mut opts = workspace_resolve_options(packages);
    opts.update = UpdateBehavior::Patches;
    opts.current_pkg = Some(CurrentPkg {
        id: PkgResolutionId::from("acme@1.0.0"),
        name: Some("acme".to_string()),
        version: Some("1.0.0".to_string()),
        resolution: LockfileResolution::Registry(RegistryResolution {
            integrity: "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="
                .parse()
                .unwrap(),
            revision: None,
        }),
        published_at: None,
    });

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("registry pick");
    assert_eq!(result.resolved_via, "npm-registry");
    assert_eq!(result.id.as_str(), "acme@1.0.0");
    mock.assert_async().await;
}

#[tokio::test]
async fn revision_refresh_revalidates_a_warm_packument_without_update_checksums() {
    let mut server = mockito::Server::new_async().await;
    let first_mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("1.0.0".to_string()),
        ..WantedDependency::default()
    };

    resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap();
    first_mock.assert_async().await;
    first_mock.remove_async().await;

    let refresh_mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;
    let opts = ResolveOptions {
        update: UpdateBehavior::Patches,
        update_checksums: false,
        ..ResolveOptions::default()
    };
    resolver.resolve(&wanted, &opts).await.unwrap();

    refresh_mock.assert_async().await;
}

#[tokio::test]
async fn workspace_shadows_registry_when_name_and_version_match() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(single_version_body(
            "1.0.0",
            "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
        ))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace shadow");
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.id.as_str(), "link:../acme");
    // `latest` is back-stamped from the registry packument so the
    // install layer can still surface upgrade hints.
    assert_eq!(result.latest.as_deref(), Some("1.0.0"));
}

#[tokio::test]
async fn always_try_workspace_packages_false_skips_workspace_match() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(single_version_body(
            "1.0.0",
            "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
        ))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let mut opts = workspace_resolve_options(packages);
    opts.always_try_workspace_packages = false;

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("registry pick");
    assert_eq!(result.resolved_via, "npm-registry");
    assert_eq!(result.id.as_str(), "acme@1.0.0");
}

#[tokio::test]
async fn registry_version_higher_than_workspace_keeps_registry_pick() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(single_version_body(
            "1.1.0",
            "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
        ))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("registry pick");
    assert_eq!(result.resolved_via, "npm-registry");
    assert_eq!(result.id.as_str(), "acme@1.1.0");
}

#[tokio::test]
async fn prefer_workspace_packages_keeps_workspace_over_newer_registry() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(single_version_body(
            "1.1.0",
            "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
        ))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let mut opts = workspace_resolve_options(packages);
    opts.prefer_workspace_packages = true;

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace pick");
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.id.as_str(), "link:../acme");
}

#[tokio::test]
async fn prefer_workspace_packages_skips_the_registry_entirely() {
    let mut server = mockito::Server::new_async().await;
    let mock = server.mock("GET", "/acme").expect(0).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let mut opts = workspace_resolve_options(packages);
    opts.prefer_workspace_packages = true;

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace pick");
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.id.as_str(), "link:../acme");
    assert_eq!(result.latest, None);
    mock.assert_async().await;
}

#[tokio::test]
async fn prefer_workspace_packages_still_consults_registry_for_several_local_copies() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(single_version_body(
            "1.1.0",
            "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
        ))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages_at(
        "acme",
        &[("1.0.0", "/repo/packages/acme-1"), ("1.1.0", "/repo/packages/acme-11")],
    );
    let mut opts = workspace_resolve_options(packages);
    opts.prefer_workspace_packages = true;

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace pick");
    mock.assert_async().await;
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.id.as_str(), "link:../acme-11");
    assert_eq!(result.latest.as_deref(), Some("1.1.0"));
}

#[tokio::test]
async fn prefer_workspace_packages_still_consults_registry_for_injected_deps() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(single_version_body(
            "1.1.0",
            "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
        ))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let mut opts = workspace_resolve_options(packages);
    opts.prefer_workspace_packages = true;

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        injected: Some(true),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace pick");
    mock.assert_async().await;
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.latest.as_deref(), Some("1.1.0"));
}

#[tokio::test]
async fn prefer_workspace_packages_does_not_engage_without_a_matching_local_version() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(single_version_body(
            "2.0.0",
            "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
        ))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let mut opts = workspace_resolve_options(packages);
    opts.prefer_workspace_packages = true;

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^2.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("registry pick");
    mock.assert_async().await;
    assert_eq!(result.resolved_via, "npm-registry");
    assert_eq!(result.id.as_str(), "acme@2.0.0");
}

#[tokio::test]
async fn prefer_workspace_packages_still_consults_registry_under_no_downgrade() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(single_version_body(
            "1.1.0",
            "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
        ))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let mut opts = workspace_resolve_options(packages);
    opts.prefer_workspace_packages = true;
    opts.trust_policy = Some(TrustPolicy::NoDowngrade);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace pick");
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.id.as_str(), "link:../acme");
    assert_eq!(result.latest.as_deref(), Some("1.1.0"));
    mock.assert_async().await;
}

#[tokio::test]
async fn prefer_workspace_packages_still_consults_registry_when_updating_checksums() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(single_version_body(
            "1.1.0",
            "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
        ))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let mut opts = workspace_resolve_options(packages);
    opts.prefer_workspace_packages = true;
    opts.update_checksums = true;

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace pick");
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.id.as_str(), "link:../acme");
    assert_eq!(result.latest.as_deref(), Some("1.1.0"));
    mock.assert_async().await;
}

#[tokio::test]
async fn prefer_workspace_packages_still_consults_registry_when_injecting_workspace_packages() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(single_version_body(
            "1.1.0",
            "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
        ))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let mut opts = workspace_resolve_options(packages);
    opts.prefer_workspace_packages = true;
    opts.inject_workspace_packages = true;

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace pick");
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.latest.as_deref(), Some("1.1.0"));
    mock.assert_async().await;
}

#[tokio::test]
async fn workspace_higher_version_shadows_registry_pick() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(single_version_body(
            "1.0.0",
            "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
        ))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["2.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some(">=1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace shadow");
    assert_eq!(result.resolved_via, "workspace");
}

/// Per-version `root_dir`s so the resolved `link:` path identifies
/// which workspace entry the resolver picked.
fn build_workspace_packages_at(name: &str, entries: &[(&str, &str)]) -> WorkspacePackages {
    let mut by_version: WorkspacePackagesByVersion = BTreeMap::new();
    for (version, dir) in entries {
        by_version.insert(
            (*version).to_string(),
            WorkspacePackage {
                root_dir: PathBuf::from(*dir),
                manifest: json!({ "name": name, "version": version }),
            },
        );
    }
    let mut packages: WorkspacePackages = BTreeMap::new();
    packages.insert(name.to_string(), by_version);
    packages
}

#[tokio::test]
async fn injected_workspace_match_emits_file_resolution() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(single_version_body(
            "1.0.0",
            "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
        ))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("1.0.0".to_string()),
        injected: Some(true),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace shadow");
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.id.as_str(), "file:packages/acme");
    match &result.resolution {
        LockfileResolution::Directory(dir) => assert_eq!(dir.directory, "packages/acme"),
        other => panic!("expected directory resolution, got {other:?}"),
    }
}

#[tokio::test]
async fn workspace_fallback_picks_highest_version_for_latest_tag() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server.mock("GET", "/acme").with_status(404).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages_at(
        "acme",
        &[
            ("1.0.0", "/repo/packages/acme-1.0.0"),
            ("1.1.0", "/repo/packages/acme-1.1.0"),
            ("2.0.0", "/repo/packages/acme-2.0.0"),
        ],
    );
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("latest".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace fallback");
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.id.as_str(), "link:../acme-2.0.0");
}

/// Exercises the `includePrerelease` arm of `resolve_workspace_range`.
#[tokio::test]
async fn workspace_fallback_picks_local_prerelease_for_latest_tag() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server.mock("GET", "/acme").with_status(404).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["3.0.0-alpha.1.2.3"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("latest".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace fallback");
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.id.as_str(), "link:../acme");
}

#[tokio::test]
async fn workspace_fallback_resolves_specific_version_request() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server.mock("GET", "/acme").with_status(404).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages_at(
        "acme",
        &[
            ("1.0.0", "/repo/packages/acme-1.0.0"),
            ("1.1.0", "/repo/packages/acme-1.1.0"),
            ("2.0.0", "/repo/packages/acme-2.0.0"),
        ],
    );
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("1.1.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace fallback");
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.id.as_str(), "link:../acme-1.1.0");
}

/// Covers the `Ok(None)` fallback arm (200 + no matching version),
/// distinct from the `Err` 404 arm.
#[tokio::test]
async fn workspace_fallback_kicks_in_when_registry_lacks_requested_version() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(single_version_body(
            "1.0.0",
            "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
        ))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["100.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("100.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace fallback");
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.id.as_str(), "link:../acme");
}

#[tokio::test]
async fn workspace_version_mismatch_surfaces_for_exact_request_on_registry_404() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server.mock("GET", "/acme").with_status(404).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("2.0.0".to_string()),
        ..WantedDependency::default()
    };
    let err = resolver
        .resolve(&wanted, &opts)
        .await
        .expect_err("workspace can't satisfy 2.0.0; workspace version mismatch must surface");
    assert!(
        err.downcast_ref::<ResolveFromWorkspaceError>().is_some_and(|ws_err| matches!(
            ws_err,
            ResolveFromWorkspaceError::NoMatchingVersionInsideWorkspace { .. }
        )),
        "expected NoMatchingVersionInsideWorkspace, got: {err}",
    );
    let err_msg = err.to_string();
    assert!(
        err_msg.contains("No matching version found for acme@2.0.0 inside the workspace"),
        "expected the workspace mismatch message, got: {err_msg}",
    );
    assert!(
        err_msg.contains("Available versions: 1.0.0"),
        "expected error to list available workspace versions, got: {err_msg}",
    );
}

#[tokio::test]
async fn registry_pick_wins_when_workspace_version_does_not_match() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(single_version_body(
            "3.1.0",
            "sha512-CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC==",
        ))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("3.1.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("registry pick");
    assert_eq!(result.resolved_via, "npm-registry");
    assert_eq!(result.id.as_str(), "acme@3.1.0");
}

#[tokio::test]
async fn workspace_version_mismatch_surfaces_for_range_request_on_registry_404() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server.mock("GET", "/acme").with_status(404).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^2.0.0".to_string()),
        ..WantedDependency::default()
    };
    let err = resolver
        .resolve(&wanted, &opts)
        .await
        .expect_err("registry 404 and no matching workspace version must fail");
    let err_msg = err.to_string();
    assert!(
        err_msg.contains("No matching version found for acme@^2.0.0 inside the workspace"),
        "expected the workspace mismatch message, got: {err_msg}",
    );
    assert!(
        err_msg.contains("Available versions: 1.0.0"),
        "expected error to list available workspace versions, got: {err_msg}",
    );
}

#[tokio::test]
async fn workspace_version_mismatch_surfaces_when_registry_lacks_matching_version() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(single_version_body(
            "2.0.0",
            "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
        ))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^3.0.0".to_string()),
        ..WantedDependency::default()
    };
    let err = resolver
        .resolve(&wanted, &opts)
        .await
        .expect_err("neither registry nor workspace satisfies ^3.0.0");
    let err_msg = err.to_string();
    assert!(
        err_msg.contains("No matching version found for acme@^3.0.0 inside the workspace"),
        "expected the workspace mismatch message, got: {err_msg}",
    );
    assert!(
        err_msg.contains("Available versions: 1.0.0"),
        "expected error to list available workspace versions, got: {err_msg}",
    );
}

#[tokio::test]
async fn registry_404_propagates_when_package_not_in_workspace() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server.mock("GET", "/acme").with_status(404).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("other-pkg", &["1.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let err = resolver
        .resolve(&wanted, &opts)
        .await
        .expect_err("package absent from both registry and workspace must fail");
    let err_msg = err.to_string();
    assert!(err_msg.contains("404"), "expected the 404 to propagate, got: {err_msg}");
    assert!(
        !err_msg.contains("inside the workspace"),
        "workspace mismatch must not surface when the package is not in the workspace, got: {err_msg}",
    );
}

#[tokio::test]
async fn workspace_fallback_succeeds_for_range_request_on_registry_404() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server.mock("GET", "/acme").with_status(404).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace fallback");
    assert_eq!(result.resolved_via, "workspace");
}

#[tokio::test]
async fn non_404_registry_error_not_masked_by_workspace_version_mismatch() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server.mock("GET", "/acme").with_status(500).create_async().await;
    let registry = format!("{}/", server.url());
    let (mut resolver, _tempdir) = build_resolver(&registry);
    // A 5xx is retried with backoff; skip the retries so the test
    // doesn't spend over a minute sleeping.
    resolver.retry_opts = RetryOpts { retries: 0, ..RetryOpts::default() };

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^2.0.0".to_string()),
        ..WantedDependency::default()
    };
    let err = resolver
        .resolve(&wanted, &opts)
        .await
        .expect_err("a 500 registry response must propagate as an error");
    let err_msg = err.to_string();
    assert!(err_msg.contains("500"), "expected the 500 to propagate, got: {err_msg}");
    assert!(
        !err_msg.contains("inside the workspace"),
        "workspace mismatch must not mask a non-404 registry error, got: {err_msg}",
    );
}

#[tokio::test]
async fn latest_is_suppressed_when_published_by_holds_back_raw_latest() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    // PACKAGE_BODY has 1.0.0 (2024-01-10) and 1.1.0 (2024-12-10),
    // dist-tags.latest = 1.1.0. Cutoff 2024-06-01 leaves 1.1.0 immature:
    // the hint must not fire rather than name a non-latest version.
    let published_by = Some(chrono::Utc.with_ymd_and_hms(2024, 6, 1, 0, 0, 0).unwrap());
    let opts = ResolveOptions { published_by, ..ResolveOptions::default() };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().unwrap();
    assert_eq!(result.name_ver.as_ref().expect("name_ver").suffix.to_string(), "1.0.0");
    assert!(result.latest.is_none(), "immature dist-tags.latest suppresses the hint");
    assert!(result.policy_violation.is_none(), "1.0.0 is mature, no violation");
}

#[tokio::test]
async fn latest_is_raw_registry_tag_when_it_satisfies_published_by() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    // Cutoff 2025-01-01 is after both versions, so the pinned 1.0.0 install
    // still advertises the mature 1.1.0.
    let published_by = Some(chrono::Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap());
    let opts = ResolveOptions { published_by, ..ResolveOptions::default() };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().unwrap();
    assert_eq!(result.name_ver.as_ref().expect("name_ver").suffix.to_string(), "1.0.0");
    assert_eq!(result.latest.as_deref(), Some("1.1.0"));
}

#[tokio::test]
async fn latest_is_raw_registry_tag_when_published_by_is_none() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();
    assert_eq!(result.name_ver.as_ref().expect("name_ver").suffix.to_string(), "1.1.0");
    assert_eq!(result.latest.as_deref(), Some("1.1.0"));
}

#[tokio::test]
async fn latest_is_raw_registry_tag_when_published_by_exclude_matches_package() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    // The exclude policy disables the maturity policy for `acme` entirely, so
    // neither the pick nor the latest hint may be affected by the cutoff.
    let published_by = Some(chrono::Utc.with_ymd_and_hms(2024, 6, 1, 0, 0, 0).unwrap());
    let exclude = create_package_version_policy(["acme"]).expect("policy");
    let opts = ResolveOptions {
        published_by,
        published_by_exclude: Some(exclude),
        ..ResolveOptions::default()
    };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().unwrap();
    assert_eq!(result.name_ver.as_ref().expect("name_ver").suffix.to_string(), "1.1.0");
    assert_eq!(result.latest.as_deref(), Some("1.1.0"));
    assert!(result.policy_violation.is_none(), "excluded package has no violation");
}

#[tokio::test]
async fn latest_is_raw_registry_tag_when_published_by_exclude_trusts_that_version() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let published_by = Some(chrono::Utc.with_ymd_and_hms(2024, 6, 1, 0, 0, 0).unwrap());
    let exclude = create_package_version_policy(["acme@1.1.0"]).expect("policy");
    let opts = ResolveOptions {
        published_by,
        published_by_exclude: Some(exclude),
        ..ResolveOptions::default()
    };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().unwrap();
    assert_eq!(result.name_ver.as_ref().expect("name_ver").suffix.to_string(), "1.1.0");
    assert_eq!(result.latest.as_deref(), Some("1.1.0"));
}

#[tokio::test]
async fn latest_is_suppressed_when_all_versions_are_immature_fallback_case() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    // Cutoff 2023-12-01 is before both versions → the pick falls back to the
    // lowest version; latest stays suppressed because the raw tag is immature.
    let published_by = Some(chrono::Utc.with_ymd_and_hms(2023, 12, 1, 0, 0, 0).unwrap());
    let opts = ResolveOptions { published_by, ..ResolveOptions::default() };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().unwrap();
    assert_eq!(result.name_ver.as_ref().expect("name_ver").suffix.to_string(), "1.0.0");
    assert!(result.latest.is_none(), "immature dist-tags.latest suppresses the hint");
}

#[tokio::test]
async fn jsr_specifier_suppresses_latest_when_published_by_holds_back_raw_latest() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/@jsr%2Ffoo__bar")
        .with_status(200)
        .with_body(JSR_PACKAGE_BODY)
        .create_async()
        .await;
    let jsr_registry = format!("{}/", server.url());
    let mut registries = HashMap::new();
    registries.insert("default".to_string(), "https://registry.npmjs.org/".to_string());
    registries.insert("@jsr".to_string(), jsr_registry);
    let (resolver, _tempdir) = build_resolver_with_registries(registries);

    let wanted = WantedDependency {
        alias: Some("@foo/bar".to_string()),
        bare_specifier: Some("jsr:@foo/bar@^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let opts = ResolveOptions {
        published_by: Some(chrono::Utc.with_ymd_and_hms(2024, 6, 1, 0, 0, 0).unwrap()),
        ..ResolveOptions::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().unwrap();
    assert_eq!(result.name_ver.as_ref().expect("name_ver").suffix.to_string(), "1.0.0");
    assert!(result.latest.is_none(), "immature dist-tags.latest suppresses the hint");
}

/// A packument whose `dist` carries only the legacy `shasum`, the shape
/// behind <https://github.com/pnpm/pnpm/issues/13547>.
fn shasum_only_package_body(shasum: &str) -> String {
    json!({
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "modified": "2025-01-15T12:00:00.000Z",
        "versions": {
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "dist": {
                    "shasum": shasum,
                    "tarball": "https://registry/acme-1.0.0.tgz",
                },
            },
        },
    })
    .to_string()
}

fn revision_package_body(tarball: &str, revision: &serde_json::Value) -> String {
    json!({
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "modified": "2025-01-15T12:00:00.000Z",
        "versions": {
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "dist": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "tarball": tarball,
                    "revision": revision,
                    "revisions": [{
                        "revision": revision,
                        "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                        "tarball": tarball,
                        "manifest": {},
                    }],
                },
            },
        },
    })
    .to_string()
}

fn revision_history_package_body(registry: &str) -> String {
    let digest_a =
        "H0D8ktokFpR1CXnubPWC8tXX0o4YM13gWrxU0FYOD1MChgxlK_CNVgJSql50IQVG82n7u86MEs_HlXsmUv6adQ";
    let digest_b =
        "Umd2iCLuYk1I_OFexcp5y9YCy39MIVelFlVpkfIu-Me173sY0f9BxZNw77CFhlHUSpNsEbexRMSP4E3zxqPo2g";
    let digest_c =
        "rMKNsr63tCuqHLAkPUAcy04_zkTXsCh5pSeZqt_1QVItiCJZiy-mZPnVFWwAySSAXXXDhovVbCrLgdN-mONa3A";
    let integrity_a = format!("sha512-{}==", digest_a.replace('_', "/").replace('-', "+"));
    let integrity_b = format!("sha512-{}==", digest_b.replace('_', "/").replace('-', "+"));
    let integrity_c = format!("sha512-{}==", digest_c.replace('_', "/").replace('-', "+"));
    json!({
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "deprecated": "current warning",
                "dependencies": { "current-only": "1.0.0" },
                "optionalDependencies": { "removed": "1.0.0" },
                "dist": {
                    "integrity": integrity_c,
                    "tarball": format!("{registry}-/tarballs/sha512/{digest_c}"),
                    "revision": 2,
                    "revisions": [
                        {
                            "revision": 0,
                            "integrity": integrity_a,
                            "tarball": format!("{registry}-/tarballs/sha512/{digest_a}"),
                            "manifest": { "dependencies": { "original": "1.0.0" } },
                        },
                        {
                            "revision": 1,
                            "integrity": integrity_b,
                            "tarball": format!("{registry}-/tarballs/sha512/{digest_b}"),
                            "manifest": {
                                "name": "not-acme",
                                "version": "9.0.0",
                                "deprecated": "historical warning",
                                "dist": {},
                                "dependencies": { "fixed": "1.0.0" },
                            },
                        },
                        {
                            "revision": 2,
                            "integrity": integrity_c,
                            "tarball": format!("{registry}-/tarballs/sha512/{digest_c}"),
                            "manifest": { "dependencies": { "selected-current": "1.0.0" } },
                        },
                    ],
                },
            },
        },
    })
    .to_string()
}

#[tokio::test]
async fn explicit_revision_selects_its_artifact_and_manifest() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(revision_history_package_body(&registry))
        .create_async()
        .await;
    let (resolver, _tempdir) = build_resolver(&registry);
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("1.0.0+r1".to_string()),
        ..WantedDependency::default()
    };
    let opts = ResolveOptions { calc_specifier: true, ..ResolveOptions::default() };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().unwrap();
    let LockfileResolution::Tarball(resolution) = &result.resolution else {
        panic!("expected tarball resolution");
    };
    assert_eq!(resolution.revision.map(TarballRevision::get), Some(1));
    assert!(resolution.tarball.ends_with("sha512/Umd2iCLuYk1I_OFexcp5y9YCy39MIVelFlVpkfIu-Me173sY0f9BxZNw77CFhlHUSpNsEbexRMSP4E3zxqPo2g"));
    assert_eq!(result.normalized_bare_specifier.as_deref(), Some("1.0.0+r1"));
    let manifest = result.manifest.as_ref().expect("manifest");
    assert_eq!(manifest["name"], "acme");
    assert_eq!(manifest["version"], "1.0.0");
    assert_eq!(manifest["deprecated"], "current warning");
    assert_eq!(manifest["dependencies"], json!({ "fixed": "1.0.0" }));
    assert!(manifest.get("optionalDependencies").is_none());
}

#[tokio::test]
async fn explicit_current_revision_accepts_its_matching_history_record() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(revision_history_package_body(&registry))
        .create_async()
        .await;
    let (resolver, _tempdir) = build_resolver(&registry);
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("1.0.0+r2".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();
    let LockfileResolution::Tarball(resolution) = &result.resolution else {
        panic!("expected tarball resolution");
    };
    assert_eq!(resolution.revision.map(TarballRevision::get), Some(2));
    assert_eq!(
        result.manifest.as_ref().expect("manifest")["dependencies"],
        json!({
            "selected-current": "1.0.0",
        }),
    );
}

#[tokio::test]
async fn explicit_original_revision_omits_the_lockfile_revision() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(revision_history_package_body(&registry))
        .create_async()
        .await;
    let (resolver, _tempdir) = build_resolver(&registry);
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("1.0.0+r0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();
    let LockfileResolution::Tarball(resolution) = &result.resolution else {
        panic!("expected tarball resolution");
    };
    assert_eq!(resolution.revision, None);
    assert_eq!(
        result.manifest.as_ref().expect("manifest")["dependencies"],
        json!({
            "original": "1.0.0",
        }),
    );
}

#[tokio::test]
async fn unknown_and_invalid_explicit_revisions_are_hard_errors() {
    for (specifier, expected_kind) in [("1.0.0+r9", "missing"), ("1.0.0+r01", "invalid")] {
        let mut server = mockito::Server::new_async().await;
        let registry = format!("{}/", server.url());
        server
            .mock("GET", "/acme")
            .with_status(200)
            .with_body(revision_history_package_body(&registry))
            .create_async()
            .await;
        let (resolver, _tempdir) = build_resolver(&registry);
        let wanted = WantedDependency {
            alias: Some("acme".to_string()),
            bare_specifier: Some(specifier.to_string()),
            ..WantedDependency::default()
        };
        let error = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap_err();
        match expected_kind {
            "missing" => assert!(error.downcast_ref::<NoMatchingRevisionError>().is_some()),
            "invalid" => assert!(error.downcast_ref::<InvalidRevisionSpecifierError>().is_some()),
            _ => unreachable!(),
        }
    }
}

#[tokio::test]
async fn revision_metadata_is_validated_and_preserved() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let tarball = format!("{}-/tarballs/sha512/{}", registry, "A".repeat(86));
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(revision_package_body(&tarball, &json!(2)))
        .create_async()
        .await;
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted =
        WantedDependency { alias: Some("acme".to_string()), ..WantedDependency::default() };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();

    mock.assert_async().await;
    let LockfileResolution::Tarball(resolution) = result.resolution else {
        panic!("expected a tarball resolution");
    };
    assert_eq!(resolution.tarball, tarball);
    assert_eq!(resolution.revision.map(TarballRevision::get), Some(2));
}

#[tokio::test]
async fn current_revision_requires_a_matching_history_entry() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let tarball = format!("{}-/tarballs/sha512/{}", registry, "A".repeat(86));
    let mut body: serde_json::Value =
        serde_json::from_str(&revision_package_body(&tarball, &json!(1))).unwrap();
    body["versions"]["1.0.0"]["dist"]["revisions"] = json!([]);
    server.mock("GET", "/acme").with_status(200).with_body(body.to_string()).create_async().await;
    let (resolver, _tempdir) = build_resolver(&registry);
    let wanted =
        WantedDependency { alias: Some("acme".to_string()), ..WantedDependency::default() };

    let error = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap_err();

    assert!(error.downcast_ref::<MalformedRevisionHistoryError>().is_some());
}

#[tokio::test]
async fn malformed_revision_metadata_has_the_malformed_metadata_error() {
    for revision in
        [json!(0), json!(-1), json!(1.5), json!(9_007_199_254_740_992_u64), json!("1"), json!("01")]
    {
        let mut server = mockito::Server::new_async().await;
        let registry = format!("{}/", server.url());
        let tarball = format!("{}-/tarballs/sha512/{}", registry, "A".repeat(86));
        server
            .mock("GET", "/acme")
            .with_status(200)
            .with_body(revision_package_body(&tarball, &revision))
            .create_async()
            .await;
        let (resolver, _tempdir) = build_resolver(&registry);

        let wanted =
            WantedDependency { alias: Some("acme".to_string()), ..WantedDependency::default() };
        let error = match resolver.resolve(&wanted, &ResolveOptions::default()).await {
            Ok(result) => panic!("revision {revision} must fail the resolve; got {result:?}"),
            Err(error) => error,
        };

        let error =
            error.downcast_ref::<MalformedRevisionHistoryError>().expect("revision history error");
        assert_eq!(error.name, "acme");
        assert_eq!(error.version, "1.0.0");
    }
}

#[tokio::test]
async fn revision_metadata_rejects_a_tarball_from_another_registry() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let tarball = format!("https://attacker.example/-/tarballs/sha512/{}", "A".repeat(86));
    server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(revision_package_body(&tarball, &json!(1)))
        .create_async()
        .await;
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted =
        WantedDependency { alias: Some("acme".to_string()), ..WantedDependency::default() };
    let error = resolver
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .expect_err("a revision URL from another registry must fail the resolve");

    assert!(error.downcast_ref::<MalformedRevisionHistoryError>().is_some());
}

#[tokio::test]
async fn shasum_only_metadata_resolves_to_a_sha1_integrity() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(shasum_only_package_body("e21bf1d18b7ce29d1cd45f6d8e0e8bcd0a4ca8ba"))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted =
        WantedDependency { alias: Some("acme".to_string()), ..WantedDependency::default() };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();

    let LockfileResolution::Tarball(tarball) = &result.resolution else {
        panic!("expected a tarball resolution, got {:?}", result.resolution);
    };
    assert_eq!(
        tarball.integrity.as_ref().map(ToString::to_string).as_deref(),
        Some("sha1-4hvx0Yt84p0c1F9tjg6LzQpMqLo="),
    );
}

#[tokio::test]
async fn unparsable_shasum_fails_the_resolve() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(shasum_only_package_body("not-a-hex-digest"))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted =
        WantedDependency { alias: Some("acme".to_string()), ..WantedDependency::default() };
    let error = resolver
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .expect_err("an unusable shasum must fail the resolve");

    let error = error.downcast_ref::<InvalidTarballIntegrityError>().expect("integrity error");
    assert_eq!(error.shasum, "not-a-hex-digest");
    assert_eq!(error.tarball, "https://registry/acme-1.0.0.tgz");
}

#[tokio::test]
async fn invalid_shasum_error_redacts_registry_metadata() {
    let mut server = mockito::Server::new_async().await;
    let body = json!({
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "modified": "2025-01-15T12:00:00.000Z",
        "versions": {
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "dist": {
                    "shasum": "not\u{7}-a-hex-digest",
                    "tarball": "https://user:hunter2@registry/acme-1.0.0.tgz",
                },
            },
        },
    })
    .to_string();
    let _mock = server.mock("GET", "/acme").with_status(200).with_body(body).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted =
        WantedDependency { alias: Some("acme".to_string()), ..WantedDependency::default() };
    let error = resolver
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .expect_err("an unusable shasum must fail the resolve")
        .to_string();

    assert!(!error.contains("hunter2"), "inline credentials must not reach the message: {error}");
    assert!(
        !error.chars().any(char::is_control),
        "control characters must not reach the message: {error:?}",
    );
}
