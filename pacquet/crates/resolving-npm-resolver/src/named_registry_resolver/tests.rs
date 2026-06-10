use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use pacquet_lockfile::LockfileResolution;
use pacquet_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pacquet_resolving_resolver_base::{ResolveOptions, Resolver, WantedDependency};
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use crate::{
    NamedRegistryResolver, merge_named_registries,
    pick_package::{
        InMemoryPackageMetaCache, shared_packument_fetch_locker, shared_picked_manifest_cache,
    },
};

/// Packument for `@acme/private` served under a named registry —
/// matches the fixture upstream uses in
/// `resolveNamedRegistry.test.ts`.
const ACME_PRIVATE_BODY: &str = r#"{
    "name": "@acme/private",
    "dist-tags": { "latest": "2.1.0" },
    "modified": "2025-01-15T12:00:00.000Z",
    "time": {
        "1.0.0": "2024-01-10T08:30:00.000Z",
        "2.0.0": "2024-06-01T08:30:00.000Z",
        "2.1.0": "2024-12-10T08:30:00.000Z"
    },
    "versions": {
        "1.0.0": {
            "name": "@acme/private",
            "version": "1.0.0",
            "dist": {
                "integrity": "sha512-EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE==",
                "shasum": "4444444444444444444444444444444444444444",
                "tarball": "https://npm.work.example/@acme/private/-/private-1.0.0.tgz"
            }
        },
        "2.0.0": {
            "name": "@acme/private",
            "version": "2.0.0",
            "dist": {
                "integrity": "sha512-FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF==",
                "shasum": "5555555555555555555555555555555555555555",
                "tarball": "https://npm.work.example/@acme/private/-/private-2.0.0.tgz"
            }
        },
        "2.1.0": {
            "name": "@acme/private",
            "version": "2.1.0",
            "dist": {
                "integrity": "sha512-GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG==",
                "shasum": "6666666666666666666666666666666666666666",
                "tarball": "https://npm.work.example/@acme/private/-/private-2.1.0.tgz"
            }
        }
    }
}"#;

#[expect(
    clippy::needless_pass_by_value,
    reason = "nested test helper called many times; owned arg keeps the call sites and assert ergonomics simple"
)]
fn build_resolver(
    user_named_registries: HashMap<String, String>,
) -> (NamedRegistryResolver<InMemoryPackageMetaCache>, TempDir) {
    let merged = merge_named_registries(&user_named_registries).expect("URLs are valid");
    let registry_names: HashSet<String> = merged.keys().cloned().collect();
    let cache_dir = TempDir::new().expect("tempdir");
    let resolver = NamedRegistryResolver {
        named_registries: merged,
        registry_names,
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

#[tokio::test]
async fn resolves_via_builtin_gh_alias() {
    // The `gh:` alias is built in; configure a mock server and tell
    // `merge_named_registries` to redirect `gh` at it. Mirrors
    // upstream's first test in `resolveNamedRegistry.test.ts` —
    // confirms the resolver picks up the built-in entry without the
    // user having to declare it.
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/@acme%2Fprivate")
        .with_status(200)
        .with_body(ACME_PRIVATE_BODY)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());

    let mut user = HashMap::new();
    user.insert("gh".to_string(), registry);
    let (resolver, _tempdir) = build_resolver(user);

    let wanted = WantedDependency {
        alias: Some("@acme/private".to_string()),
        bare_specifier: Some("gh:^2.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();
    assert_eq!(result.resolved_via, "named-registry");
    assert_eq!(result.id.as_str(), "@acme/private@2.1.0");
    assert_eq!(result.latest.as_deref(), Some("2.1.0"));
    assert_eq!(result.alias.as_deref(), Some("@acme/private"));
}

#[tokio::test]
async fn preserves_scoped_pkg_name_when_alias_differs() {
    // `my-private` is the local manifest alias; the registry serves
    // the package under `@acme/private`. The resolver records the
    // resolution under the registry name so the lockfile / install
    // tree match how the package is published. Mirrors upstream's
    // "preserves the scoped package name when the alias is a
    // different name" test.
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/@acme%2Fprivate")
        .with_status(200)
        .with_body(ACME_PRIVATE_BODY)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());

    let mut user = HashMap::new();
    user.insert("gh".to_string(), registry);
    let (resolver, _tempdir) = build_resolver(user);

    let wanted = WantedDependency {
        alias: Some("my-private".to_string()),
        bare_specifier: Some("gh:@acme/private@^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();
    assert_eq!(result.resolved_via, "named-registry");
    assert_eq!(result.id.as_str(), "@acme/private@1.0.0");
    assert_eq!(
        result.alias.as_deref(),
        Some("@acme/private"),
        "alias must follow the registry name, not the local `my-private`",
    );
}

#[tokio::test]
async fn user_config_overrides_builtin_gh_alias() {
    // GHES users point `gh` at their enterprise host. The user-
    // supplied entry overrides the built-in default. Mirrors
    // upstream's "allows user config to override the built-in gh
    // alias (GHES)" test.
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/@acme%2Fprivate")
        .with_status(200)
        .with_body(ACME_PRIVATE_BODY)
        .create_async()
        .await;
    let enterprise_registry = format!("{}/", server.url());

    let mut user = HashMap::new();
    user.insert("gh".to_string(), enterprise_registry);
    let (resolver, _tempdir) = build_resolver(user);

    let wanted = WantedDependency {
        alias: Some("@acme/private".to_string()),
        bare_specifier: Some("gh:^2.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();
    assert_eq!(result.resolved_via, "named-registry");
    assert_eq!(result.id.as_str(), "@acme/private@2.1.0");
}

#[tokio::test]
async fn resolves_user_defined_named_registry() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/@acme%2Fprivate")
        .with_status(200)
        .with_body(ACME_PRIVATE_BODY)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());

    let mut user = HashMap::new();
    user.insert("work".to_string(), registry.clone());
    let (resolver, _tempdir) = build_resolver(user);

    let wanted = WantedDependency {
        alias: Some("@acme/private".to_string()),
        bare_specifier: Some("work:^2.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();
    assert_eq!(result.resolved_via, "named-registry");
    assert_eq!(result.id.as_str(), "@acme/private@2.1.0");
    // The resolver records the dependency under the scoped package
    // name the registry serves, not the local alias.
    assert_eq!(result.alias.as_deref(), Some("@acme/private"));
    assert!(matches!(result.resolution, LockfileResolution::Tarball(_)));
}

#[tokio::test]
async fn declines_non_named_specifiers() {
    let (resolver, _tempdir) = build_resolver(HashMap::new());

    // Plain semver — npm's slot, not named-registry's. No mock means
    // the test fails if the resolver tries to hit the network.
    for bare in ["^1.0.0", "npm:@acme/private@1.0.0", "jsr:@acme/private"] {
        let wanted = WantedDependency {
            alias: Some("@acme/private".to_string()),
            bare_specifier: Some(bare.to_string()),
            ..WantedDependency::default()
        };
        let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap();
        assert!(result.is_none(), "expected None for {bare:?}");
    }
}

#[tokio::test]
async fn declines_github_git_shortcut() {
    // `github:` belongs to the git resolver; `gh:` is the
    // named-registry alias. The parser keys off the alias set, so
    // `github:` always falls through here.
    let (resolver, _tempdir) = build_resolver(HashMap::new());

    for bare in ["github:owner/repo", "github:owner/repo#main", "github:@acme/foo"] {
        let wanted = WantedDependency {
            alias: None,
            bare_specifier: Some(bare.to_string()),
            ..WantedDependency::default()
        };
        let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap();
        assert!(result.is_none(), "expected None for {bare:?}");
    }
}

#[tokio::test]
async fn declines_named_alias_for_bare_version_without_package_alias() {
    // Without any package alias, `gh:<version>` cannot map to a
    // package name. No mock means the test fails if the resolver
    // tries to hit the network.
    let (resolver, _tempdir) = build_resolver(HashMap::new());
    let wanted = WantedDependency {
        alias: None,
        bare_specifier: Some("gh:2.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn errors_on_invalid_scoped_package_name() {
    let (resolver, _tempdir) = build_resolver(HashMap::new());
    let wanted = WantedDependency {
        alias: None,
        bare_specifier: Some("gh:@acme".to_string()),
        ..WantedDependency::default()
    };
    let err = resolver
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .expect_err("scope without name must error");
    assert!(err.to_string().contains("'gh:'"), "got {err}");
}
