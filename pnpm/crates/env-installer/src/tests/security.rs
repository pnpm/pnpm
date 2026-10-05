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

#[tokio::test]
async fn rejects_config_lockfile_redirect_before_installing() {
    let harness = harness();
    let (resolver, _cache) = build_resolver(&harness.registry_url);
    let root = TempDir::new().unwrap();
    let config_deps = BTreeMap::from([("@pnpm.e2e/foo".to_string(), clean_spec("100.0.0"))]);
    resolve_and_install_config_deps::<SilentReporter>(
        &config_deps,
        &resolver,
        &options(&harness, root.path(), false),
    )
    .await
    .unwrap();
    let mut env = EnvLockfile::read(root.path()).unwrap().unwrap();
    let key: PackageKey = "@pnpm.e2e/foo@100.0.0".parse().unwrap();
    env.packages.get_mut(&key).unwrap().resolution =
        pnpm_lockfile::LockfileResolution::Tarball(pnpm_lockfile::TarballResolution {
            tarball: format!("{}unapproved.tgz", harness.registry_url),
            integrity: Some("sha512-ZGVm".parse().unwrap()),
            revision: None,
            git_hosted: None,
            path: None,
        });
    env.write(root.path()).unwrap();
    let error = resolve_and_install_config_deps::<SilentReporter>(
        &config_deps,
        &resolver,
        &options(&harness, root.path(), true),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(error, ConfigDepError::BadConfigDep { message } if message.contains("Configuration dependency")),
    );
}

#[tokio::test]
async fn rejects_config_lockfile_replacing_declared_integrity_pin() {
    let harness = harness();
    let (resolver, _cache) = build_resolver(&harness.registry_url);
    let root = TempDir::new().unwrap();
    let config_deps = BTreeMap::from([("@pnpm.e2e/foo".to_string(), clean_spec("100.0.0"))]);
    resolve_and_install_config_deps::<SilentReporter>(
        &config_deps,
        &resolver,
        &options(&harness, root.path(), false),
    )
    .await
    .unwrap();
    let mut env = EnvLockfile::read(root.path()).unwrap().unwrap();
    let key: PackageKey = "@pnpm.e2e/foo@100.0.0".parse().unwrap();
    env.packages.get_mut(&key).unwrap().resolution =
        pnpm_lockfile::LockfileResolution::Tarball(pnpm_lockfile::TarballResolution {
            tarball: "https://unapproved.example/config.tgz".to_string(),
            integrity: Some("sha512-ZGVm".parse().unwrap()),
            revision: None,
            git_hosted: None,
            path: None,
        });
    env.write(root.path()).unwrap();
    let pinned = BTreeMap::from([("@pnpm.e2e/foo".to_string(), clean_spec("100.0.0+sha512-YWJj"))]);
    let error = resolve_and_install_config_deps::<SilentReporter>(
        &pinned,
        &resolver,
        &options(&harness, root.path(), true),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(error, ConfigDepError::BadConfigDep { message } if message.contains("configured integrity")),
    );
}

#[tokio::test]
async fn rejects_replacing_integrity_pinned_config_version() {
    let harness = harness();
    let (resolver, _cache) = build_resolver(&harness.registry_url);
    let root = TempDir::new().unwrap();
    let config_deps = BTreeMap::from([("@pnpm.e2e/foo".to_string(), clean_spec("100.0.0"))]);
    resolve_and_install_config_deps::<SilentReporter>(
        &config_deps,
        &resolver,
        &options(&harness, root.path(), false),
    )
    .await
    .unwrap();
    let mut env = EnvLockfile::read(root.path()).unwrap().unwrap();
    env.root_importer_mut().config_dependencies
        .get_mut("@pnpm.e2e/foo")
        .unwrap()
        .version = "200.0.0".to_string();
    let old_key: PackageKey = "@pnpm.e2e/foo@100.0.0".parse().unwrap();
    let new_key: PackageKey = "@pnpm.e2e/foo@200.0.0".parse().unwrap();
    let metadata = env.packages.remove(&old_key).unwrap();
    env.packages.insert(new_key, metadata);
    env.write(root.path()).unwrap();
    let pinned = BTreeMap::from([("@pnpm.e2e/foo".to_string(), clean_spec("100.0.0+sha512-YWJj"))]);
    let error = resolve_and_install_config_deps::<SilentReporter>(
        &pinned,
        &resolver,
        &options(&harness, root.path(), true),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(error, ConfigDepError::BadConfigDep { message } if message.contains("configured integrity")),
    );
}
