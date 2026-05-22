use std::{collections::HashMap, sync::Arc};

use chrono::TimeZone;
use pacquet_lockfile::LockfileResolution;
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_resolving_resolver_base::{
    LatestQuery, ResolveOptions, Resolver, UpdateBehavior, WantedDependency,
};
use pretty_assertions::assert_eq;
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

/// Two NpmResolvers pointing at different registries, sharing the
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
