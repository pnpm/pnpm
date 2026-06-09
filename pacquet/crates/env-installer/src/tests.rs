use crate::{ConfigDepError, ConfigDepsInstallOptions, resolve_and_install_config_deps};
use pacquet_lockfile::EnvLockfile;
use pacquet_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pacquet_reporter::SilentReporter;
use pacquet_resolving_npm_resolver::{
    InMemoryPackageMetaCache, NpmResolver, shared_packument_fetch_locker,
    shared_picked_manifest_cache,
};
use pacquet_store_dir::StoreDir;
use pacquet_testing_utils::registry::TestRegistry;
use pacquet_workspace_state::ConfigDependency;
use std::{collections::BTreeMap, path::Path, sync::Arc};
use tempfile::TempDir;

/// Build an npm resolver pointing at the in-process mock registry.
fn build_resolver(registry: &str) -> (NpmResolver<InMemoryPackageMetaCache>, TempDir) {
    let cache_dir = TempDir::new().unwrap();
    let mut registries = std::collections::HashMap::new();
    registries.insert("default".to_string(), registry.to_string());
    let resolver = NpmResolver {
        registries,
        named_registries: std::collections::HashMap::new(),
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

/// Per-test handles kept alive for the duration of an install call.
struct Harness {
    registry_url: String,
    registries: std::collections::HashMap<String, String>,
    http_client: ThrottledClient,
    auth_headers: AuthHeaders,
    store_dir: &'static StoreDir,
}

fn harness() -> Harness {
    let registry_url = TestRegistry::start().url();
    let mut registries = std::collections::HashMap::new();
    registries.insert("default".to_string(), registry_url.clone());
    let store_dir: &'static StoreDir =
        Box::leak(Box::new(StoreDir::new(TempDir::new().unwrap().keep())));
    Harness {
        registry_url,
        registries,
        http_client: ThrottledClient::default(),
        auth_headers: AuthHeaders::default(),
        store_dir,
    }
}

fn options<'a>(
    harness: &'a Harness,
    root_dir: &'a Path,
    frozen: bool,
) -> ConfigDepsInstallOptions<'a> {
    ConfigDepsInstallOptions {
        root_dir,
        store_dir: harness.store_dir,
        http_client: &harness.http_client,
        auth_headers: &harness.auth_headers,
        registries: &harness.registries,
        verify_store_integrity: true,
        offline: false,
        package_import_method: Default::default(),
        retry_opts: RetryOpts::default(),
        frozen_lockfile: frozen,
        supported_architectures: None,
        current_node_version: "20.0.0",
        current_os: "linux",
        current_cpu: "x64",
        current_libc: "glibc",
    }
}

fn clean_spec(version: &str) -> ConfigDependency {
    ConfigDependency::VersionWithIntegrity(version.to_string())
}

#[tokio::test]
async fn resolves_and_installs_config_dep_when_no_env_lockfile_exists() {
    let harness = harness();
    let (resolver, _cache) = build_resolver(&harness.registry_url);
    let root = TempDir::new().unwrap();

    let mut config_deps = BTreeMap::new();
    config_deps.insert("@pnpm.e2e/foo".to_string(), clean_spec("100.0.0"));

    resolve_and_install_config_deps::<SilentReporter>(
        &config_deps,
        &resolver,
        &options(&harness, root.path(), false),
    )
    .await
    .unwrap();

    let installed = root.path().join("node_modules/.pnpm-config/@pnpm.e2e/foo/package.json");
    assert!(installed.exists(), "config dep must be linked into .pnpm-config");

    let env = EnvLockfile::read(root.path()).unwrap().expect("env lockfile written");
    let importer = &env.importers[EnvLockfile::ROOT_IMPORTER_KEY];
    let entry = &importer.config_dependencies["@pnpm.e2e/foo"];
    assert_eq!(entry.specifier, "100.0.0");
    assert_eq!(entry.version, "100.0.0");
    let key = "@pnpm.e2e/foo@100.0.0".parse().unwrap();
    assert!(env.packages.contains_key(&key), "package entry recorded");
    assert!(
        env.snapshots[&key].optional_dependencies.is_none(),
        "a config dep with no optionalDependencies keeps an empty snapshot",
    );
}

#[tokio::test]
async fn records_optional_subdeps_with_platform_fields() {
    let harness = harness();
    let (resolver, _cache) = build_resolver(&harness.registry_url);
    let root = TempDir::new().unwrap();

    let mut config_deps = BTreeMap::new();
    config_deps
        .insert("@pnpm.e2e/support-different-architectures".to_string(), clean_spec("1.0.0"));

    resolve_and_install_config_deps::<SilentReporter>(
        &config_deps,
        &resolver,
        &options(&harness, root.path(), false),
    )
    .await
    .unwrap();

    let env = EnvLockfile::read(root.path()).unwrap().expect("env lockfile written");
    let parent_key = "@pnpm.e2e/support-different-architectures@1.0.0".parse().unwrap();
    let optionals = env.snapshots[&parent_key]
        .optional_dependencies
        .as_ref()
        .expect("optional subdeps recorded");
    assert_eq!(optionals.len(), 8, "all eight platform variants are recorded");

    // Every recorded subdep keeps its platform fields in `packages:`.
    let only_linux = "@pnpm.e2e/only-linux-x64-glibc@1.0.0".parse().unwrap();
    let metadata = env.packages.get(&only_linux).expect("platform subdep recorded in packages");
    assert_eq!(metadata.os.as_deref(), Some(["linux".to_string()].as_slice()));
    assert_eq!(metadata.cpu.as_deref(), Some(["x64".to_string()].as_slice()));
}

#[tokio::test]
async fn rejects_optional_subdep_with_non_exact_version() {
    let harness = harness();
    let (resolver, _cache) = build_resolver(&harness.registry_url);
    let root = TempDir::new().unwrap();

    // @pnpm.e2e/foobar declares `@pnpm.e2e/bar: "^100.0.0"` as an
    // optionalDependency — a range, which config deps forbid.
    let mut config_deps = BTreeMap::new();
    config_deps.insert("@pnpm.e2e/foobar".to_string(), clean_spec("100.0.0"));

    let error = resolve_and_install_config_deps::<SilentReporter>(
        &config_deps,
        &resolver,
        &options(&harness, root.path(), false),
    )
    .await
    .expect_err("a non-exact optional subdep must be rejected");
    assert!(
        matches!(error, ConfigDepError::OptionalNotExact { .. }),
        "unexpected error: {error:?}",
    );
}

#[tokio::test]
async fn frozen_lockfile_rejects_new_config_dep() {
    let harness = harness();
    let (resolver, _cache) = build_resolver(&harness.registry_url);
    let root = TempDir::new().unwrap();

    let mut config_deps = BTreeMap::new();
    config_deps.insert("@pnpm.e2e/foo".to_string(), clean_spec("100.0.0"));

    let error = resolve_and_install_config_deps::<SilentReporter>(
        &config_deps,
        &resolver,
        &options(&harness, root.path(), true),
    )
    .await
    .expect_err("a new config dep under --frozen-lockfile must fail");
    assert!(
        matches!(error, ConfigDepError::FrozenLockfileOutdated { .. }),
        "unexpected error: {error:?}",
    );
}

#[tokio::test]
async fn re_resolves_when_config_dep_version_changes() {
    let harness = harness();
    let (resolver, _cache) = build_resolver(&harness.registry_url);
    let root = TempDir::new().unwrap();

    let mut first = BTreeMap::new();
    first.insert("@pnpm.e2e/foo".to_string(), clean_spec("100.0.0"));
    resolve_and_install_config_deps::<SilentReporter>(
        &first,
        &resolver,
        &options(&harness, root.path(), false),
    )
    .await
    .unwrap();

    let mut second = BTreeMap::new();
    second.insert("@pnpm.e2e/foo".to_string(), clean_spec("100.1.0"));
    resolve_and_install_config_deps::<SilentReporter>(
        &second,
        &resolver,
        &options(&harness, root.path(), false),
    )
    .await
    .unwrap();

    let env = EnvLockfile::read(root.path()).unwrap().expect("env lockfile written");
    let entry = &env.importers[EnvLockfile::ROOT_IMPORTER_KEY].config_dependencies["@pnpm.e2e/foo"];
    assert_eq!(entry.version, "100.1.0", "version bump is reflected");
    let old_key = "@pnpm.e2e/foo@100.0.0".parse().unwrap();
    assert!(!env.packages.contains_key(&old_key), "stale version pruned from lockfile");
}
