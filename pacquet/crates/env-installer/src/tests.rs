use crate::{
    ConfigDepError, ConfigDepsInstallOptions, prune_env_lockfile, resolve_and_install_config_deps,
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
use pacquet_resolving_resolver_base::{ResolveOptions, Resolver, WantedDependency};
use pacquet_store_dir::StoreDir;
use pacquet_testing_utils::registry::TestRegistry;
use pacquet_workspace_state::ConfigDependency;
use std::{
    collections::BTreeMap,
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
    // The migrated specifier collapses to the bare version (the integrity
    // moves into the lockfile's packages entry).
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

    // A second install under --frozen-lockfile needs no changes, so it
    // succeeds rather than raising FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE.
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

    // An old inline-integrity entry that isn't yet in the env lockfile
    // needs migrating — which mutates the lockfile, so --frozen-lockfile
    // must reject it (the `lockfile_changed` branch, distinct from the
    // clean-specifier resolve branch).
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
    // First install does work: exactly `started` then `done`, in that
    // order (the channel is order-sensitive for pnpm compatibility).
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
    // Everything is already in place — no events on the second install.
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

    // Re-resolve with the dep no longer declared: it must be dropped from
    // the env lockfile and unlinked from `.pnpm-config`.
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
