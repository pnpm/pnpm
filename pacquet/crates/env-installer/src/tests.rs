use crate::{
    ConfigDepError, ConfigDepsInstallOptions, install_config_deps, is_package_manager_resolved,
    prune_env_lockfile, resolve_and_install_config_deps, resolve_package_manager_integrities,
};
use pacquet_lockfile::{
    EnvLockfile, LockfileResolution, PackageKey, PackageMetadata, RegistryResolution,
    SnapshotDepRef, SnapshotEntry, SpecifierAndResolution,
};
use pacquet_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pacquet_reporter::{InstallingConfigDepsStatus, LogEvent, Reporter, SilentReporter};
use pacquet_resolving_npm_resolver::{
    InMemoryPackageMetaCache, NpmResolver, shared_packument_fetch_locker,
    shared_picked_manifest_cache,
};
use pacquet_resolving_resolver_base::{
    LatestInfo, LatestQuery, PkgResolutionId, ResolveFuture, ResolveLatestFuture, ResolveOptions,
    ResolveResult, Resolver, WantedDependency,
};
use pacquet_store_dir::StoreDir;
use pacquet_testing_utils::registry::TestRegistry;
use pacquet_workspace_state::ConfigDependency;
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::{Arc, Mutex},
};
use tempfile::TempDir;

/// Resolve `name@version` against the mock registry and return its
/// integrity string, so migration tests can build the old inline
/// `<version>+<integrity>` format without hard-coding a checksum.
async fn integrity_of(
    resolver: &NpmResolver<InMemoryPackageMetaCache>,
    name: &str,
    version: &str,
) -> String {
    let wanted = WantedDependency {
        alias: Some(name.to_string()),
        bare_specifier: Some(version.to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();
    match result.resolution {
        LockfileResolution::Tarball(tarball) => tarball.integrity.unwrap().to_string(),
        LockfileResolution::Registry(registry) => registry.integrity.to_string(),
        other => panic!("unexpected resolution: {other:?}"),
    }
}

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
        filter_metadata: false,
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
        package_import_method: pacquet_config::PackageImportMethod::default(),
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

#[derive(Default)]
struct FixtureResolver {
    packages: HashMap<(String, String), serde_json::Value>,
}

impl FixtureResolver {
    fn new() -> Self {
        Self::default()
    }

    fn package(mut self, manifest: serde_json::Value) -> Self {
        let name = manifest["name"].as_str().expect("fixture package name").to_string();
        let version = manifest["version"].as_str().expect("fixture package version").to_string();
        self.packages.insert((name, version), manifest);
        self
    }
}

impl Resolver for FixtureResolver {
    fn resolve<'a>(
        &'a self,
        wanted_dependency: &'a WantedDependency,
        _opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        Box::pin(async move {
            let Some(alias) = wanted_dependency.alias.as_deref() else {
                return Ok(None);
            };
            let Some(specifier) = wanted_dependency.bare_specifier.as_deref() else {
                return Ok(None);
            };
            let Some(manifest) =
                self.packages.get(&(alias.to_string(), specifier.to_string())).cloned()
            else {
                return Ok(None);
            };
            let name = manifest["name"].as_str().expect("fixture package name").to_string();
            let version =
                manifest["version"].as_str().expect("fixture package version").to_string();
            let id = format!("{name}@{version}");
            Ok(Some(ResolveResult {
                id: PkgResolutionId::from(id.as_str()),
                name_ver: Some(id.parse().expect("fixture name/version parses")),
                latest: Some(version),
                published_at: None,
                manifest: Some(Arc::new(manifest)),
                resolution: LockfileResolution::Registry(RegistryResolution {
                    integrity: ssri::Integrity::from(id.as_bytes()),
                }),
                resolved_via: "npm-registry".to_string(),
                normalized_bare_specifier: Some(specifier.to_string()),
                alias: Some(alias.to_string()),
                policy_violation: None,
            }))
        })
    }

    fn resolve_latest<'a>(
        &'a self,
        _query: &'a LatestQuery,
        _opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(async { Ok(Some(LatestInfo::default())) })
    }
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

    let only_linux = "@pnpm.e2e/only-linux-x64-glibc@1.0.0".parse().unwrap();
    let metadata = env.packages.get(&only_linux).expect("platform subdep recorded in packages");
    assert_eq!(metadata.os.as_deref(), Some(["linux".to_string()].as_slice()));
    assert_eq!(metadata.cpu.as_deref(), Some(["x64".to_string()].as_slice()));
    assert_eq!(metadata.libc.as_deref(), Some(["glibc".to_string()].as_slice()));
}

#[tokio::test]
async fn resolves_package_manager_dependencies_graph() {
    let harness = harness();
    let root = TempDir::new().unwrap();
    let resolver = FixtureResolver::new()
        .package(serde_json::json!({
            "name": "pnpm",
            "version": "100.0.0",
            "bin": "bin/pnpm.cjs",
            "engines": { "node": ">=22.0.0" },
        }))
        .package(serde_json::json!({
            "name": "@pnpm/exe",
            "version": "100.0.0",
            "bin": { "pnpm": "bin/pnpm.cjs" },
            "dependencies": { "detect-libc": "2.0.0" },
            "optionalDependencies": { "@pnpm/linuxstatic-x64": "100.0.0" },
        }))
        .package(serde_json::json!({
            "name": "detect-libc",
            "version": "2.0.0",
            "engines": { "node": ">=8" },
        }))
        .package(serde_json::json!({
            "name": "@pnpm/linuxstatic-x64",
            "version": "100.0.0",
            "cpu": ["x64"],
            "os": ["linux"],
            "libc": "musl",
        }));

    resolve_package_manager_integrities(
        "^100.0.0",
        "100.0.0",
        &resolver,
        &options(&harness, root.path(), false),
    )
    .await
    .unwrap();

    let env = EnvLockfile::read(root.path()).unwrap().expect("env lockfile written");
    let pm_deps = env.importers[EnvLockfile::ROOT_IMPORTER_KEY]
        .package_manager_dependencies
        .as_ref()
        .expect("package manager deps recorded");
    assert_eq!(pm_deps["pnpm"].specifier, "^100.0.0");
    assert_eq!(pm_deps["pnpm"].version, "100.0.0");
    assert_eq!(pm_deps["@pnpm/exe"].specifier, "^100.0.0");
    assert_eq!(pm_deps["@pnpm/exe"].version, "100.0.0");

    let pnpm_key: PackageKey = "pnpm@100.0.0".parse().unwrap();
    let exe_key: PackageKey = "@pnpm/exe@100.0.0".parse().unwrap();
    let libc_key: PackageKey = "detect-libc@2.0.0".parse().unwrap();
    let platform_key: PackageKey = "@pnpm/linuxstatic-x64@100.0.0".parse().unwrap();

    assert_eq!(env.packages[&pnpm_key].has_bin, Some(true));
    assert_eq!(env.packages[&pnpm_key].engines.as_ref().unwrap()["node"], ">=22.0.0");
    assert_eq!(env.packages[&exe_key].has_bin, Some(true));
    assert_eq!(env.packages[&platform_key].libc.as_deref(), Some(["musl".to_string()].as_slice()));

    let exe_snapshot = &env.snapshots[&exe_key];
    let detect_libc_name = "detect-libc".parse().unwrap();
    let platform_name = "@pnpm/linuxstatic-x64".parse().unwrap();
    let detect_libc_ref =
        exe_snapshot.dependencies.as_ref().unwrap()[&detect_libc_name].to_string();
    assert_eq!(detect_libc_ref, "2.0.0");
    assert_eq!(
        exe_snapshot.optional_dependencies.as_ref().unwrap()[&platform_name].to_string(),
        "100.0.0",
    );
    assert!(env.snapshots[&platform_key].optional);
    assert!(!env.snapshots[&libc_key].optional);
    assert!(is_package_manager_resolved(&env, "^100.0.0", "100.0.0"));
    assert!(!is_package_manager_resolved(&env, "~100.0.0", "100.0.0"));

    let mut env_with_extra_pm_dep = env.clone();
    env_with_extra_pm_dep
        .importers
        .get_mut(EnvLockfile::ROOT_IMPORTER_KEY)
        .unwrap()
        .package_manager_dependencies
        .as_mut()
        .unwrap()
        .insert(
            "yarn".to_string(),
            SpecifierAndResolution { specifier: "1.0.0".to_string(), version: "1.0.0".to_string() },
        );
    assert!(!is_package_manager_resolved(&env_with_extra_pm_dep, "^100.0.0", "100.0.0",));
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

/// Recursively search `dir` for an entry named `name`, without following
/// symlinks (so it can't loop through the dir links a successful install leaves).
fn contains_entry_named(dir: &Path, name: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        if entry.file_name() == name {
            return true;
        }
        if entry.file_type().is_ok_and(|file_type| file_type.is_dir())
            && contains_entry_named(&entry.path(), name)
        {
            return true;
        }
    }
    false
}

#[tokio::test]
async fn rejects_config_dep_with_path_traversal_name() {
    let harness = harness();
    let (resolver, _cache) = build_resolver(&harness.registry_url);
    let root = TempDir::new().unwrap();

    // Resolve a legit config dep, then re-key its entry under a traversal-shaped
    // name to mimic a malicious committed lockfile.
    let mut config_deps = BTreeMap::new();
    config_deps.insert("@pnpm.e2e/foo".to_string(), clean_spec("100.0.0"));
    resolve_and_install_config_deps::<SilentReporter>(
        &config_deps,
        &resolver,
        &options(&harness, root.path(), false),
    )
    .await
    .unwrap();

    let mut env = EnvLockfile::read(root.path()).unwrap().expect("env lockfile written");
    let spec = env.root_importer_mut().config_dependencies.remove("@pnpm.e2e/foo").unwrap();
    let malicious_name = "../../PWNED_CFGDEP".to_string();
    env.root_importer_mut().config_dependencies.insert(malicious_name.clone(), spec.clone());
    let legit_key: PackageKey = "@pnpm.e2e/foo@100.0.0".parse().unwrap();
    let pkg = env.packages[&legit_key].clone();
    let malicious_key: PackageKey = format!("{malicious_name}@{}", spec.version).parse().unwrap();
    env.packages.insert(malicious_key, pkg);

    let error = install_config_deps::<SilentReporter>(&env, &options(&harness, root.path(), false))
        .await
        .expect_err("a traversal-shaped config dep name must be rejected");
    assert!(
        matches!(error, ConfigDepError::InvalidDependencyName { .. }),
        "unexpected error: {error:?}",
    );

    assert!(!contains_entry_named(root.path(), "PWNED_CFGDEP"));
    assert!(!contains_entry_named(&harness.store_dir.links(), "PWNED_CFGDEP"));
}

#[tokio::test]
async fn rejects_optional_subdep_with_path_traversal_name() {
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

    let mut env = EnvLockfile::read(root.path()).unwrap().expect("env lockfile written");
    let parent_key: PackageKey = "@pnpm.e2e/foo@100.0.0".parse().unwrap();
    let pkg = env.packages[&parent_key].clone();
    let malicious_name = "../../PWNED_SUBDEP".to_string();
    let malicious_key: PackageKey = format!("{malicious_name}@100.0.0").parse().unwrap();
    env.packages.insert(malicious_key, pkg);
    let subdep_name: pacquet_lockfile::PkgName = malicious_name.parse().unwrap();
    let subdep_ref: SnapshotDepRef = "100.0.0".parse().unwrap();
    env.snapshots.entry(parent_key).or_default().optional_dependencies =
        Some(std::iter::once((subdep_name, subdep_ref)).collect());

    let error = install_config_deps::<SilentReporter>(&env, &options(&harness, root.path(), false))
        .await
        .expect_err("a traversal-shaped optional subdep name must be rejected");
    assert!(
        matches!(error, ConfigDepError::InvalidDependencyName { .. }),
        "unexpected error: {error:?}",
    );

    assert!(!contains_entry_named(root.path(), "PWNED_SUBDEP"));
    assert!(!contains_entry_named(&harness.store_dir.links(), "PWNED_SUBDEP"));
}

/// `__proto__` is an invalid npm name (leading `_`); Rust's string-keyed maps
/// reject it with none of the null-prototype handling the JS side needs.
#[tokio::test]
async fn rejects_config_dep_named_dunder_proto() {
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

    let mut env = EnvLockfile::read(root.path()).unwrap().expect("env lockfile written");
    let spec = env.root_importer_mut().config_dependencies.remove("@pnpm.e2e/foo").unwrap();
    let malicious_name = "__proto__".to_string();
    env.root_importer_mut().config_dependencies.insert(malicious_name.clone(), spec.clone());
    let legit_key: PackageKey = "@pnpm.e2e/foo@100.0.0".parse().unwrap();
    let pkg = env.packages[&legit_key].clone();
    let malicious_key: PackageKey = format!("{malicious_name}@{}", spec.version).parse().unwrap();
    env.packages.insert(malicious_key, pkg);

    let error = install_config_deps::<SilentReporter>(&env, &options(&harness, root.path(), false))
        .await
        .expect_err("a config dep named __proto__ must be rejected");
    assert!(
        matches!(error, ConfigDepError::InvalidDependencyName { .. }),
        "unexpected error: {error:?}",
    );
}

#[tokio::test]
async fn rejects_invalid_manifest_config_dep_name_before_writing_lockfile() {
    let harness = harness();
    let (resolver, _cache) = build_resolver(&harness.registry_url);
    let root = TempDir::new().unwrap();

    let mut config_deps = BTreeMap::new();
    config_deps.insert(
        "../../PWNED".to_string(),
        ConfigDependency::VersionWithIntegrity("100.0.0+sha512-deadbeef".to_string()),
    );

    let error = resolve_and_install_config_deps::<SilentReporter>(
        &config_deps,
        &resolver,
        &options(&harness, root.path(), false),
    )
    .await
    .expect_err("an invalid manifest config dep name must be rejected");
    assert!(
        matches!(error, ConfigDepError::InvalidDependencyName { .. }),
        "unexpected error: {error:?}",
    );

    assert!(!root.path().join("pnpm-lock.yaml").exists());
}

#[tokio::test]
async fn rejects_invalid_manifest_config_dep_version_before_writing_lockfile() {
    let harness = harness();
    let (resolver, _cache) = build_resolver(&harness.registry_url);
    let root = TempDir::new().unwrap();

    let integrity = integrity_of(&resolver, "@pnpm.e2e/foo", "100.0.0").await;
    let mut config_deps = BTreeMap::new();
    config_deps.insert(
        "@pnpm.e2e/foo".to_string(),
        ConfigDependency::VersionWithIntegrity(format!("../../../PWNED+{integrity}")),
    );

    let error = resolve_and_install_config_deps::<SilentReporter>(
        &config_deps,
        &resolver,
        &options(&harness, root.path(), false),
    )
    .await
    .expect_err("an invalid manifest config dep version must be rejected");
    assert!(
        matches!(error, ConfigDepError::InvalidConfigDepVersion { .. }),
        "unexpected error: {error:?}",
    );

    assert!(!root.path().join("pnpm-lock.yaml").exists());
}

#[tokio::test]
async fn rejects_config_dep_with_path_traversal_version() {
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

    let mut env = EnvLockfile::read(root.path()).unwrap().expect("env lockfile written");
    let malicious_version = "../../../PWNED";
    env.root_importer_mut().config_dependencies.get_mut("@pnpm.e2e/foo").unwrap().version =
        malicious_version.to_string();
    let legit_key: PackageKey = "@pnpm.e2e/foo@100.0.0".parse().unwrap();
    let pkg = env.packages[&legit_key].clone();
    let malicious_key: PackageKey = format!("@pnpm.e2e/foo@{malicious_version}").parse().unwrap();
    env.packages.insert(malicious_key, pkg);

    let error = install_config_deps::<SilentReporter>(&env, &options(&harness, root.path(), false))
        .await
        .expect_err("a traversal-shaped config dep version must be rejected");
    assert!(
        matches!(error, ConfigDepError::InvalidConfigDepVersion { .. }),
        "unexpected error: {error:?}",
    );
    // Pin the message format (guards against a doubled/dropped quote).
    let message = error.to_string();
    assert_eq!(
        message,
        r#"The config dependency "@pnpm.e2e/foo" has an invalid version "../../../PWNED""#,
    );

    assert!(!contains_entry_named(root.path(), "PWNED"));
    assert!(!contains_entry_named(&harness.store_dir.links(), "PWNED"));
}

#[tokio::test]
async fn rejects_optional_subdep_with_path_traversal_version() {
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

    let mut env = EnvLockfile::read(root.path()).unwrap().expect("env lockfile written");
    let parent_key: PackageKey = "@pnpm.e2e/foo@100.0.0".parse().unwrap();
    let pkg = env.packages[&parent_key].clone();
    let malicious_version = "../../../PWNED";
    let subdep_name = "@pnpm.e2e/bar";
    let malicious_key: PackageKey = format!("{subdep_name}@{malicious_version}").parse().unwrap();
    env.packages.insert(malicious_key, pkg);
    let subdep_name_parsed: pacquet_lockfile::PkgName = subdep_name.parse().unwrap();
    let subdep_ref: SnapshotDepRef = malicious_version.parse().unwrap();
    env.snapshots.entry(parent_key).or_default().optional_dependencies =
        Some(std::iter::once((subdep_name_parsed, subdep_ref)).collect());

    let error = install_config_deps::<SilentReporter>(&env, &options(&harness, root.path(), false))
        .await
        .expect_err("a traversal-shaped optional subdep version must be rejected");
    assert!(
        matches!(error, ConfigDepError::InvalidConfigDepVersion { .. }),
        "unexpected error: {error:?}",
    );

    assert!(!contains_entry_named(root.path(), "PWNED"));
    assert!(!contains_entry_named(&harness.store_dir.links(), "PWNED"));
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

#[tokio::test]
async fn migrates_old_inline_integrity_format() {
    let harness = harness();
    let (resolver, _cache) = build_resolver(&harness.registry_url);
    let root = TempDir::new().unwrap();

    // The old format embeds the integrity inline as `<version>+<integrity>`,
    // which the migration path records into the lockfile without re-resolving.
    let integrity = integrity_of(&resolver, "@pnpm.e2e/foo", "100.0.0").await;
    let mut config_deps = BTreeMap::new();
    config_deps.insert(
        "@pnpm.e2e/foo".to_string(),
        ConfigDependency::VersionWithIntegrity(format!("100.0.0+{integrity}")),
    );

    resolve_and_install_config_deps::<SilentReporter>(
        &config_deps,
        &resolver,
        &options(&harness, root.path(), false),
    )
    .await
    .unwrap();

    assert!(
        root.path().join("node_modules/.pnpm-config/@pnpm.e2e/foo/package.json").exists(),
        "migrated config dep is installed",
    );
    let env = EnvLockfile::read(root.path()).unwrap().expect("env lockfile written");
    let entry = &env.importers[EnvLockfile::ROOT_IMPORTER_KEY].config_dependencies["@pnpm.e2e/foo"];
    assert_eq!(entry.specifier, "100.0.0");
    assert_eq!(entry.version, "100.0.0");
    assert!(env.packages.contains_key(&"@pnpm.e2e/foo@100.0.0".parse().unwrap()));
}

#[tokio::test]
async fn frozen_lockfile_succeeds_when_up_to_date() {
    let harness = harness();
    let (resolver, _cache) = build_resolver(&harness.registry_url);
    let root = TempDir::new().unwrap();

    let mut config_deps = BTreeMap::new();
    config_deps.insert("@pnpm.e2e/foo".to_string(), clean_spec("100.0.0"));

    // First install (not frozen) populates the env lockfile.
    resolve_and_install_config_deps::<SilentReporter>(
        &config_deps,
        &resolver,
        &options(&harness, root.path(), false),
    )
    .await
    .unwrap();

    resolve_and_install_config_deps::<SilentReporter>(
        &config_deps,
        &resolver,
        &options(&harness, root.path(), true),
    )
    .await
    .expect("frozen install with an up-to-date env lockfile succeeds");
}

#[tokio::test]
async fn frozen_lockfile_rejects_old_format_migration() {
    let harness = harness();
    let (resolver, _cache) = build_resolver(&harness.registry_url);
    let root = TempDir::new().unwrap();

    // Migrating an old-format entry mutates the lockfile via the
    // `lockfile_changed` branch, distinct from the clean-specifier
    // resolve branch that `frozen_lockfile_rejects_new_config_dep` covers.
    let integrity = integrity_of(&resolver, "@pnpm.e2e/foo", "100.0.0").await;
    let mut config_deps = BTreeMap::new();
    config_deps.insert(
        "@pnpm.e2e/foo".to_string(),
        ConfigDependency::VersionWithIntegrity(format!("100.0.0+{integrity}")),
    );

    let error = resolve_and_install_config_deps::<SilentReporter>(
        &config_deps,
        &resolver,
        &options(&harness, root.path(), true),
    )
    .await
    .expect_err("migrating an old-format config dep under --frozen-lockfile must fail");
    assert!(
        matches!(error, ConfigDepError::FrozenLockfileOutdated { .. }),
        "unexpected error: {error:?}",
    );
}

/// Recording reporter capturing `pnpm:installing-config-deps` statuses.
struct RecordingReporter;
static CONFIG_DEP_EVENTS: Mutex<Vec<InstallingConfigDepsStatus>> = Mutex::new(Vec::new());

impl Reporter for RecordingReporter {
    fn emit(event: &LogEvent) {
        if let LogEvent::InstallingConfigDeps(log) = event {
            CONFIG_DEP_EVENTS.lock().unwrap().push(log.status);
        }
    }
}

#[tokio::test]
async fn emits_installing_config_deps_events_only_when_work_is_needed() {
    let harness = harness();
    let (resolver, _cache) = build_resolver(&harness.registry_url);
    let root = TempDir::new().unwrap();

    let mut config_deps = BTreeMap::new();
    config_deps.insert("@pnpm.e2e/foo".to_string(), clean_spec("100.0.0"));

    CONFIG_DEP_EVENTS.lock().unwrap().clear();
    resolve_and_install_config_deps::<RecordingReporter>(
        &config_deps,
        &resolver,
        &options(&harness, root.path(), false),
    )
    .await
    .unwrap();
    // The channel is order-sensitive for pnpm compatibility.
    let first = std::mem::take(&mut *CONFIG_DEP_EVENTS.lock().unwrap());
    assert_eq!(
        first,
        vec![InstallingConfigDepsStatus::Started, InstallingConfigDepsStatus::Done],
        "first install emits exactly started then done",
    );

    resolve_and_install_config_deps::<RecordingReporter>(
        &config_deps,
        &resolver,
        &options(&harness, root.path(), false),
    )
    .await
    .unwrap();
    let second = std::mem::take(&mut *CONFIG_DEP_EVENTS.lock().unwrap());
    assert!(second.is_empty(), "a no-op install emits nothing: {second:?}");
}

#[test]
fn prune_drops_orphan_packages_and_snapshots() {
    fn registry_pkg() -> PackageMetadata {
        PackageMetadata {
            resolution: LockfileResolution::Registry(RegistryResolution {
                integrity: ssri::Integrity::from(b"x"),
            }),
            version: None,
            engines: None,
            cpu: None,
            os: None,
            libc: None,
            deprecated: None,
            has_bin: None,
            prepare: None,
            bundled_dependencies: None,
            peer_dependencies: None,
            peer_dependencies_meta: None,
        }
    }

    let mut env = EnvLockfile::create();
    // A config dep with one optional subdep — both reachable.
    let parent: PackageKey = "@pnpm.e2e/foo@100.0.0".parse().unwrap();
    let subdep: PackageKey = "@pnpm.e2e/bar@1.0.0".parse().unwrap();
    env.root_importer_mut().config_dependencies.insert(
        "@pnpm.e2e/foo".to_string(),
        SpecifierAndResolution { specifier: "100.0.0".to_string(), version: "100.0.0".to_string() },
    );
    env.packages.insert(parent.clone(), registry_pkg());
    env.packages.insert(subdep.clone(), registry_pkg());
    let mut optionals = std::collections::HashMap::new();
    optionals
        .insert("@pnpm.e2e/bar".parse().unwrap(), SnapshotDepRef::Plain("1.0.0".parse().unwrap()));
    env.snapshots.insert(
        parent.clone(),
        SnapshotEntry { optional_dependencies: Some(optionals), ..SnapshotEntry::default() },
    );
    env.snapshots
        .insert(subdep.clone(), SnapshotEntry { optional: true, ..SnapshotEntry::default() });

    // An orphan left over from a previous resolution: no importer (and no
    // reachable snapshot) references it.
    let orphan: PackageKey = "@pnpm.e2e/foo@99.0.0".parse().unwrap();
    env.packages.insert(orphan.clone(), registry_pkg());
    env.snapshots.insert(orphan.clone(), SnapshotEntry::default());

    prune_env_lockfile(&mut env);

    assert!(env.packages.contains_key(&parent), "reachable config dep kept");
    assert!(env.packages.contains_key(&subdep), "reachable optional subdep kept");
    assert!(!env.packages.contains_key(&orphan), "orphan package pruned");
    assert!(!env.snapshots.contains_key(&orphan), "orphan snapshot pruned");
}

#[tokio::test]
async fn removed_config_dep_is_pruned_from_lockfile_and_pnpm_config() {
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
    assert!(root.path().join("node_modules/.pnpm-config/@pnpm.e2e/foo/package.json").exists());

    // Re-resolve with the dep no longer declared.
    let empty = BTreeMap::new();
    resolve_and_install_config_deps::<SilentReporter>(
        &empty,
        &resolver,
        &options(&harness, root.path(), false),
    )
    .await
    .unwrap();

    let env = EnvLockfile::read(root.path()).unwrap().expect("env lockfile present");
    assert!(
        env.importers[EnvLockfile::ROOT_IMPORTER_KEY].config_dependencies.is_empty(),
        "removed config dep dropped from the env lockfile importer",
    );
    assert!(
        !env.packages.contains_key(&"@pnpm.e2e/foo@100.0.0".parse().unwrap()),
        "its package entry pruned",
    );
    assert!(
        !root.path().join("node_modules/.pnpm-config/@pnpm.e2e/foo").exists(),
        "its .pnpm-config link removed",
    );
}
