use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::TimeZone;
use pacquet_config::{TrustPolicy, version_policy::create_package_version_policy};
use pacquet_lockfile::LockfileResolution;
use pacquet_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pacquet_resolving_resolver_base::{
    LatestQuery, PackageVersionGuard, PackageVersionGuardDecision, PackageVersionGuardFuture,
    ResolveOptions, Resolver, UpdateBehavior, WantedDependency, WorkspacePackage,
    WorkspacePackages, WorkspacePackagesByVersion,
};
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

use crate::{
    npm_resolver::NpmResolver,
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
        "1.0.0": "2024-01-10T08:30:00.000Z",
        "1.1.0": "2024-12-10T08:30:00.000Z"
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
        named_registries: HashMap::new(),
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

/// When `published_by` filters out the raw `latest`, the resolver's
/// `latest` field carries the policy-aware tag (highest mature version)
/// so the install summary reporter doesn't advertise a version the
/// policy itself held back. Mirrors `publishedBy.test.ts` upstream.
#[tokio::test]
async fn latest_is_policy_aware_when_published_by_filters_raw_latest() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    // PACKAGE_BODY has 1.0.0 (2024-01-10) and 1.1.0 (2024-12-10),
    // dist-tags.latest = 1.1.0. Cutoff 2024-06-01 filters 1.1.0 out
    // as immature → policy-aware latest becomes 1.0.0.
    let published_by = Some(chrono::Utc.with_ymd_and_hms(2024, 6, 1, 0, 0, 0).unwrap());
    let opts = ResolveOptions { published_by, ..ResolveOptions::default() };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().unwrap();
    assert_eq!(result.name_ver.as_ref().expect("name_ver").suffix.to_string(), "1.0.0");
    assert_eq!(result.latest.as_deref(), Some("1.0.0"), "policy-aware latest, not raw 1.1.0");
    assert!(result.policy_violation.is_none(), "1.0.0 is mature, no violation");
}

/// Baseline: without `published_by`, the resolver returns the raw
/// `dist-tags.latest` (`1.1.0` in [`PACKAGE_BODY`]), so the reporter's
/// `(X is available)` hint can still fire for an actual upgrade.
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
    // Picks 1.1.0 (max in range), latest is the raw tag.
    assert_eq!(result.name_ver.as_ref().expect("name_ver").suffix.to_string(), "1.1.0");
    assert_eq!(result.latest.as_deref(), Some("1.1.0"));
}

/// When `published_by_exclude` matches the package as `AnyVersion`,
/// the maturity filter is skipped entirely: the picker returns the
/// raw latest (1.1.0) even though `published_by` would otherwise
/// filter it out. Important for the reporter — an excluded package
/// must not have its `(X is available)` hint suppressed by a policy
/// that doesn't apply to it.
#[tokio::test]
async fn latest_is_raw_registry_tag_when_published_by_exclude_matches_package() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    // PACKAGE_BODY has 1.0.0 (2024-01-10) and 1.1.0 (2024-12-10),
    // dist-tags.latest = 1.1.0. Cutoff 2024-06-01 would normally filter
    // 1.1.0 out → policy-aware latest 1.0.0. The exclude policy bypasses
    // the filter for `acme`, so latest stays at the raw 1.1.0.
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

/// Pins the contract that the JSR path (which shares `pickFromSimpleRegistry`
/// with the named-registry path) carries the policy-aware latest through to
/// the resolve result. [`JSR_PACKAGE_BODY`] has 1.0.0 (2024-01-10) and
/// 1.1.0 (2024-12-10), `dist-tags.latest` = 1.1.0. `published_by` 2024-06-01
/// filters 1.1.0 out → policy-aware latest = 1.0.0.
#[tokio::test]
async fn jsr_specifier_surfaces_policy_aware_latest_under_published_by() {
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
    assert_eq!(result.latest.as_deref(), Some("1.0.0"), "policy-aware latest, not raw 1.1.0");
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
            named_registries: HashMap::new(),
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
        workspace_packages: Some(packages),
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
