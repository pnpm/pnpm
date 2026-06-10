use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::TimeZone;
use pacquet_config::TrustPolicy;
use pacquet_lockfile::LockfileResolution;
use pacquet_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pacquet_resolving_resolver_base::{
    LatestQuery, ResolveOptions, Resolver, UpdateBehavior, WantedDependency, WorkspacePackage,
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
        retry_opts: RetryOpts::default(),
    };
    (resolver, cache_dir)
}

/// Packument body for `@jsr/foo__bar` — the npm-shaped name JSR
/// serves `@foo/bar` under
/// ([source](https://github.com/pnpm/pnpm/blob/1627943d2a/resolving/jsr-specifier-parser/src/index.ts#L53-L64)).
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
async fn workspace_path_form_falls_through_to_local_resolver() {
    let server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    // `workspace:./foo` and `workspace:../foo` are owned by the local
    // resolver in the chain — `try_resolve_from_workspace` defers on
    // them so the dispatcher falls through.
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
    // Mirrors pnpm's
    // [`Cannot resolve package from workspace because opts.workspacePackages is not defined`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/npm-resolver/src/index.ts#L828-L830)
    // throw when the resolver receives a `workspace:` spec but the
    // install caller never populated `workspace_packages`.
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

    // `^1.0.0` picks 1.1.0 (the max), which has no trust evidence while
    // the earlier 1.0.0 shipped a trusted publisher — a downgrade. The
    // resolver-time gate must reject it as a hard error.
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

    // Same downgrade history, but without `trustPolicy='no-downgrade'`
    // the gate never runs and 1.1.0 resolves cleanly.
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
        // The user-facing dependency alias is the JSR-style scoped
        // name. The resolver folds it into `@jsr/foo__bar` for the
        // metadata fetch but restores `@foo/bar` on the result.
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
    // Asserting the upstream-defined error message ties the test to
    // the public `ERR_PNPM_MISSING_JSR_PACKAGE_SCOPE` contract; the
    // resolver seam returns the parser error as a boxed `dyn Error`
    // so we can't downcast to the variant directly.
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
///
/// The fixture for each registry serves a payload that differs by
/// `dependencies`, so the cache leak shows up as the second
/// resolver's `manifest.dependencies` being the *first* registry's
/// when the bug is present. With the registry-scoped key in place
/// each resolver gets its own manifest.
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

/// Ports pnpm's [`index.ts#L1442-L1491`](https://github.com/pnpm/pnpm/blob/5353fcbf01/resolving/npm-resolver/test/index.ts#L1442-L1491);
/// this is the case behind [#11929] (babylon's `@dev/build-tools`
/// isn't on npm, so bare-semver must resolve via the workspace).
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

/// Ports pnpm's [`index.ts#L1129-L1166`](https://github.com/pnpm/pnpm/blob/5353fcbf01/resolving/npm-resolver/test/index.ts#L1129-L1166).
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

/// Ports pnpm's [`index.ts#L1208-L1245`](https://github.com/pnpm/pnpm/blob/5353fcbf01/resolving/npm-resolver/test/index.ts#L1208-L1245).
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

/// Ports pnpm's [`index.ts#L1315-L1357`](https://github.com/pnpm/pnpm/blob/5353fcbf01/resolving/npm-resolver/test/index.ts#L1315-L1357).
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

/// Ports pnpm's [`index.ts#L1358-L1398`](https://github.com/pnpm/pnpm/blob/5353fcbf01/resolving/npm-resolver/test/index.ts#L1358-L1398).
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

/// Ports pnpm's [`index.ts#L1399-L1441`](https://github.com/pnpm/pnpm/blob/5353fcbf01/resolving/npm-resolver/test/index.ts#L1399-L1441).
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

/// Ports pnpm's [`index.ts#L1167-L1206`](https://github.com/pnpm/pnpm/blob/5353fcbf01/resolving/npm-resolver/test/index.ts#L1167-L1206).
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

/// Ports pnpm's [`index.ts#L1494-L1544`](https://github.com/pnpm/pnpm/blob/5353fcbf01/resolving/npm-resolver/test/index.ts#L1494-L1544).
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

/// Ports pnpm's [`index.ts#L1546-L1582`](https://github.com/pnpm/pnpm/blob/5353fcbf01/resolving/npm-resolver/test/index.ts#L1546-L1582);
/// exercises the `includePrerelease` arm of `resolve_workspace_range`.
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

/// Ports pnpm's [`index.ts#L1584-L1634`](https://github.com/pnpm/pnpm/blob/5353fcbf01/resolving/npm-resolver/test/index.ts#L1584-L1634).
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

/// Ports pnpm's [`index.ts#L1636-L1672`](https://github.com/pnpm/pnpm/blob/5353fcbf01/resolving/npm-resolver/test/index.ts#L1636-L1672);
/// covers the `Ok(None)` fallback arm (200 + no matching version),
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

/// Ports pnpm's [`index.ts#L2092-L2121`](https://github.com/pnpm/pnpm/blob/5353fcbf01/resolving/npm-resolver/test/index.ts#L2092-L2121).
#[tokio::test]
async fn registry_error_propagates_when_workspace_has_no_matching_version() {
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
        .expect_err("workspace can't satisfy 2.0.0; original 404 error must surface");
    assert!(err.to_string().contains("404"), "expected the 404 to propagate, got: {err}");
}

/// Ports pnpm's [`index.ts#L2154-L2183`](https://github.com/pnpm/pnpm/blob/5353fcbf01/resolving/npm-resolver/test/index.ts#L2154-L2183).
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
