use super::{
    BTreeMap, ConfigDepError, ConfigDependency, EnvLockfile, InstallingConfigDepsStatus, LogEvent,
    Mutex, PackageKey, Reporter, SilentReporter, TempDir, build_resolver, clean_spec,
    contains_entry_named, harness, install_config_deps, integrity_of, options,
    resolve_and_install_config_deps,
};

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

/// An inline integrity pins the config dependency alone, so a range in its
/// optionalDependencies does not block the install the way it does for a
/// clean specifier.
#[tokio::test]
async fn keeps_optional_subdeps_of_a_pinned_config_dep_out_of_the_lockfile() {
    let harness = harness();
    let (resolver, _cache) = build_resolver(&harness.registry_url);
    let root = TempDir::new().unwrap();

    let integrity = integrity_of(&resolver, "@pnpm.e2e/foobar", "100.0.0").await;
    let mut config_deps = BTreeMap::new();
    config_deps.insert(
        "@pnpm.e2e/foobar".to_string(),
        ConfigDependency::VersionWithIntegrity(format!("100.0.0+{integrity}")),
    );

    resolve_and_install_config_deps::<SilentReporter>(
        &config_deps,
        &resolver,
        &options(&harness, root.path(), false),
    )
    .await
    .unwrap();

    let env = EnvLockfile::read(root.path()).unwrap().expect("env lockfile written");
    let key: PackageKey = "@pnpm.e2e/foobar@100.0.0".parse().unwrap();
    assert!(
        env.snapshots[&key].optional_dependencies.is_none(),
        "a pinned config dep records no optional subdeps",
    );
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
async fn emits_installing_config_deps_events_only_when_work_is_needed() {
    // Recording reporter capturing every `LogEvent`. The static lives in this
    // fn's scope, so other tests get independent storage and never race on it.
    static CONFIG_DEP_EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            CONFIG_DEP_EVENTS.lock().unwrap().push(event.clone());
        }
    }

    // The `pnpm:installing-config-deps` statuses in emit order.
    fn config_dep_statuses(events: &[LogEvent]) -> Vec<InstallingConfigDepsStatus> {
        events
            .iter()
            .filter_map(|event| match event {
                LogEvent::InstallingConfigDeps(log) => Some(log.status),
                _ => None,
            })
            .collect()
    }

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
        config_dep_statuses(&first),
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
    assert!(config_dep_statuses(&second).is_empty(), "a no-op install emits nothing: {second:?}");
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
