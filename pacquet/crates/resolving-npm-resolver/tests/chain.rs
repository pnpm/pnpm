//! Chain-integration tests for [`NamedRegistryResolver`].
//!
//! Mirrors upstream's
//! [`resolving/default-resolver/test/namedRegistry.ts`](https://github.com/pnpm/pnpm/blob/b61e268d57/resolving/default-resolver/test/namedRegistry.ts):
//! once the named-registry resolver lands in the chain, configured
//! aliases must lose to the explicit local-scheme protocols
//! (`link:` / `workspace:` / `file:`) so a user who happens to
//! configure a `link` / `workspace:` / `file` named-registry alias
//! cannot accidentally hijack the built-in shapes.

use std::{
    collections::{HashMap, HashSet},
    fs,
    sync::Arc,
};

use pacquet_lockfile::LockfileResolution;
use pacquet_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pacquet_resolving_default_resolver::DefaultResolver;
use pacquet_resolving_local_resolver::{
    LocalPathResolver, LocalResolverContext, LocalSchemeResolver,
};
use pacquet_resolving_npm_resolver::{
    InMemoryPackageMetaCache, NamedRegistryResolver, merge_named_registries,
    shared_packument_fetch_locker, shared_picked_manifest_cache,
};
use pacquet_resolving_resolver_base::{ResolveOptions, WantedDependency};
use tempfile::TempDir;

fn named_registry_resolver(
    user: &HashMap<String, String>,
) -> NamedRegistryResolver<InMemoryPackageMetaCache> {
    let merged = merge_named_registries(user).expect("URLs are valid");
    let registry_names: HashSet<String> = merged.keys().cloned().collect();
    NamedRegistryResolver {
        named_registries: merged,
        registry_names,
        http_client: Arc::new(ThrottledClient::default()),
        auth_headers: Arc::new(AuthHeaders::default()),
        meta_cache: Arc::new(InMemoryPackageMetaCache::default()),
        fetch_locker: shared_packument_fetch_locker(),
        picked_manifest_cache: shared_picked_manifest_cache(),
        // No cache_dir means no on-disk mirror — every fetch goes
        // through the network. The link / workspace / file tests never
        // hit named-registry, so this is fine without mocks.
        cache_dir: None,
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: false,
        retry_opts: RetryOpts::default(),
    }
}

fn setup_project_with_pkg() -> TempDir {
    let tmp = TempDir::new().expect("tempdir");
    let pkg_dir = tmp.path().join("pkg");
    fs::create_dir_all(&pkg_dir).expect("create pkg dir");
    fs::write(pkg_dir.join("package.json"), r#"{"name":"pkg","version":"1.0.0"}"#)
        .expect("write pkg manifest");
    fs::write(tmp.path().join("package.json"), r#"{"name":"parent","version":"0.0.0"}"#)
        .expect("write parent manifest");
    tmp
}

/// Explicit `link:./pkg` wins over a `link` named-registry alias.
/// Mirrors the test.each row in
/// [`namedRegistry.ts`](https://github.com/pnpm/pnpm/blob/b61e268d57/resolving/default-resolver/test/namedRegistry.ts#L81-L108).
#[tokio::test]
async fn link_scheme_wins_over_named_registry_alias() {
    let project = setup_project_with_pkg();

    let mut user = HashMap::new();
    user.insert("link".to_string(), "https://npm.work.example.com/".to_string());

    let local_ctx = LocalResolverContext::default();
    let resolver = DefaultResolver::new(vec![
        Box::new(LocalSchemeResolver::new(local_ctx)),
        Box::new(named_registry_resolver(&user)),
        Box::new(LocalPathResolver::new(local_ctx)),
    ]);

    let opts = ResolveOptions {
        project_dir: project.path().to_path_buf(),
        lockfile_dir: project.path().to_path_buf(),
        ..ResolveOptions::default()
    };
    let wanted = WantedDependency {
        alias: Some("pkg".to_string()),
        bare_specifier: Some("link:./pkg".to_string()),
        ..WantedDependency::default()
    };

    let result = resolver.resolve(&wanted, &opts).await.expect("resolve");
    assert_eq!(result.resolved_via, "local-filesystem");
    assert!(matches!(result.resolution, LockfileResolution::Directory(_)));
}

/// Explicit `workspace:./pkg` wins over a `workspace` named-registry
/// alias.
#[tokio::test]
async fn workspace_scheme_wins_over_named_registry_alias() {
    let project = setup_project_with_pkg();

    let mut user = HashMap::new();
    user.insert("workspace".to_string(), "https://npm.work.example.com/".to_string());

    let local_ctx = LocalResolverContext::default();
    let resolver = DefaultResolver::new(vec![
        Box::new(LocalSchemeResolver::new(local_ctx)),
        Box::new(named_registry_resolver(&user)),
        Box::new(LocalPathResolver::new(local_ctx)),
    ]);

    let opts = ResolveOptions {
        project_dir: project.path().to_path_buf(),
        lockfile_dir: project.path().to_path_buf(),
        ..ResolveOptions::default()
    };
    let wanted = WantedDependency {
        alias: Some("pkg".to_string()),
        bare_specifier: Some("workspace:./pkg".to_string()),
        ..WantedDependency::default()
    };

    let result = resolver.resolve(&wanted, &opts).await.expect("resolve");
    assert_eq!(result.resolved_via, "local-filesystem");
    assert!(matches!(result.resolution, LockfileResolution::Directory(_)));
}

/// Explicit `file:./pkg` wins over a `file` named-registry alias.
#[tokio::test]
async fn file_scheme_wins_over_named_registry_alias() {
    let project = setup_project_with_pkg();

    let mut user = HashMap::new();
    user.insert("file".to_string(), "https://npm.work.example.com/".to_string());

    let local_ctx = LocalResolverContext::default();
    let resolver = DefaultResolver::new(vec![
        Box::new(LocalSchemeResolver::new(local_ctx)),
        Box::new(named_registry_resolver(&user)),
        Box::new(LocalPathResolver::new(local_ctx)),
    ]);

    let opts = ResolveOptions {
        project_dir: project.path().to_path_buf(),
        lockfile_dir: project.path().to_path_buf(),
        ..ResolveOptions::default()
    };
    let wanted = WantedDependency {
        alias: Some("pkg".to_string()),
        bare_specifier: Some("file:./pkg".to_string()),
        ..WantedDependency::default()
    };

    let result = resolver.resolve(&wanted, &opts).await.expect("resolve");
    assert_eq!(result.resolved_via, "local-filesystem");
    assert!(matches!(result.resolution, LockfileResolution::Directory(_)));
}
