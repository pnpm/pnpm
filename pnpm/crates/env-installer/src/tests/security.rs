use super::{
    BTreeMap, ConfigDepError, EnvLockfile, PackageKey, SilentReporter, SnapshotDepRef, TempDir,
    build_resolver, clean_spec, contains_entry_named, harness, install_config_deps, options,
    resolve_and_install_config_deps,
};

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
    let subdep_name: pnpm_lockfile::PkgName = malicious_name.parse().unwrap();
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
    let subdep_name_parsed: pnpm_lockfile::PkgName = subdep_name.parse().unwrap();
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
