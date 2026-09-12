use super::{
    BTreeMap, ConfigDependency, EnvLockfile, FixtureResolver, LockfileResolution, SilentReporter,
    TempDir, build_resolver, harness, integrity_of, options, pnpm_engine_packages,
    resolve_and_install_config_deps, resolve_package_manager_integrities,
};

/// A registry that advertises tarballs on another host (a load-balanced
/// proxy or Artifactory-style mirror) must still yield integrity-only
/// package-manager entries, or the bootstrap validation rejects them.
/// See <https://github.com/pnpm/pnpm/issues/13619>.
#[tokio::test]
async fn records_integrity_only_resolutions_for_non_derivable_tarball_urls() {
    let harness = harness();
    let root = TempDir::new().unwrap();
    let resolver = FixtureResolver::new()
        .with_non_derivable_tarball_urls()
        .package(serde_json::json!({
            "name": "pnpm",
            "version": "11.0.0",
            "bin": "bin/pnpm.cjs",
        }))
        .package(serde_json::json!({
            "name": "@pnpm/exe",
            "version": "11.0.0",
            "bin": { "pnpm": "bin/pnpm.cjs" },
            "optionalDependencies": { "@pnpm/linuxstatic-x64": "11.0.0" },
        }))
        .package(serde_json::json!({
            "name": "@pnpm/linuxstatic-x64",
            "version": "11.0.0",
        }));

    resolve_package_manager_integrities(
        pnpm_engine_packages("11.0.0"),
        "^11.0.0",
        "11.0.0",
        &resolver,
        &options(&harness, root.path(), false),
        false,
    )
    .await
    .unwrap();

    let env = EnvLockfile::read(root.path()).unwrap().expect("env lockfile written");
    for (key, metadata) in &env.packages {
        assert!(
            matches!(&metadata.resolution, LockfileResolution::Registry(resolution) if !resolution.integrity.to_string().is_empty()),
            "expected an integrity-only resolution for {key}, got {:?}",
            metadata.resolution,
        );
    }
}

#[tokio::test]
async fn migrates_old_inline_integrity_format() {
    let harness = harness();
    let (resolver, _cache) = build_resolver(&harness.registry_url);
    let root = TempDir::new().unwrap();

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
