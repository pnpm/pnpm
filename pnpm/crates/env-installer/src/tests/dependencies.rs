use super::{
    BTreeMap, ConfigDepError, EnvLockfile, FixtureResolver, PackageKey, SilentReporter,
    SpecifierAndResolution, TempDir, build_resolver, clean_spec, harness,
    is_package_manager_resolved, options, pnpm_engine_packages, resolve_and_install_config_deps,
    resolve_package_manager_integrities,
};

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
            "version": "11.0.0",
            "bin": "bin/pnpm.cjs",
            "engines": { "node": ">=22.0.0" },
        }))
        .package(serde_json::json!({
            "name": "@pnpm/exe",
            "version": "11.0.0",
            "bin": { "pnpm": "bin/pnpm.cjs" },
            "dependencies": { "detect-libc": "2.0.0" },
            "optionalDependencies": { "@pnpm/linuxstatic-x64": "11.0.0" },
        }))
        .package(serde_json::json!({
            "name": "detect-libc",
            "version": "2.0.0",
            "engines": { "node": ">=8" },
        }))
        .package(serde_json::json!({
            "name": "@pnpm/linuxstatic-x64",
            "version": "11.0.0",
            "cpu": ["x64"],
            "os": ["linux"],
            "libc": "musl",
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
    let pm_deps = env.importers[EnvLockfile::ROOT_IMPORTER_KEY]
        .package_manager_dependencies
        .as_ref()
        .expect("package manager deps recorded");
    assert_eq!(pm_deps["pnpm"].specifier, "^11.0.0");
    assert_eq!(pm_deps["pnpm"].version, "11.0.0");
    assert_eq!(pm_deps["@pnpm/exe"].specifier, "^11.0.0");
    assert_eq!(pm_deps["@pnpm/exe"].version, "11.0.0");

    let pnpm_key: PackageKey = "pnpm@11.0.0".parse().unwrap();
    let exe_key: PackageKey = "@pnpm/exe@11.0.0".parse().unwrap();
    let libc_key: PackageKey = "detect-libc@2.0.0".parse().unwrap();
    let platform_key: PackageKey = "@pnpm/linuxstatic-x64@11.0.0".parse().unwrap();

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
        "11.0.0",
    );
    assert!(env.snapshots[&platform_key].optional);
    assert!(!env.snapshots[&libc_key].optional);
    assert!(is_package_manager_resolved(&env, "^11.0.0", "11.0.0"));
    assert!(!is_package_manager_resolved(&env, "~11.0.0", "11.0.0"));

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
    assert!(!is_package_manager_resolved(&env_with_extra_pm_dep, "^11.0.0", "11.0.0",));
}

#[tokio::test]
async fn resolves_package_manager_dependencies_without_exe_from_v12() {
    let harness = harness();
    let root = TempDir::new().unwrap();
    let resolver = FixtureResolver::new()
        .package(serde_json::json!({
            "name": "pnpm",
            "version": "12.0.0",
            "bin": { "pnpm": "pnpm" },
            "optionalDependencies": { "@pnpm/exe.linux-x64": "12.0.0" },
        }))
        .package(serde_json::json!({
            "name": "@pnpm/exe.linux-x64",
            "version": "12.0.0",
            "cpu": ["x64"],
            "os": ["linux"],
        }));

    resolve_package_manager_integrities(
        pnpm_engine_packages("12.0.0"),
        "^12.0.0",
        "12.0.0",
        &resolver,
        &options(&harness, root.path(), false),
        false,
    )
    .await
    .unwrap();

    let env = EnvLockfile::read(root.path()).unwrap().expect("env lockfile written");
    let pm_deps = env.importers[EnvLockfile::ROOT_IMPORTER_KEY]
        .package_manager_dependencies
        .as_ref()
        .expect("package manager deps recorded");
    assert_eq!(pm_deps.len(), 1);
    assert_eq!(pm_deps["pnpm"].specifier, "^12.0.0");
    assert_eq!(pm_deps["pnpm"].version, "12.0.0");
    assert!(!pm_deps.contains_key("@pnpm/exe"));

    let pnpm_key: PackageKey = "pnpm@12.0.0".parse().unwrap();
    let platform_key: PackageKey = "@pnpm/exe.linux-x64@12.0.0".parse().unwrap();
    assert!(env.packages.contains_key(&pnpm_key));
    assert!(env.packages.contains_key(&platform_key));
    let platform_name = "@pnpm/exe.linux-x64".parse().unwrap();
    assert_eq!(
        env.snapshots[&pnpm_key].optional_dependencies.as_ref().unwrap()[&platform_name]
            .to_string(),
        "12.0.0",
    );
    assert!(is_package_manager_resolved(&env, "^12.0.0", "12.0.0"));
}

#[tokio::test]
async fn resolves_package_manager_dependencies_without_exe_before_it_was_published() {
    let harness = harness();
    let root = TempDir::new().unwrap();
    let resolver = FixtureResolver::new().package(serde_json::json!({
        "name": "pnpm",
        "version": "6.16.0",
        "bin": { "pnpm": "bin/pnpm.cjs", "pnpx": "bin/pnpx.cjs" },
    }));

    resolve_package_manager_integrities(
        pnpm_engine_packages("6.16.0"),
        "^6.0.0",
        "6.16.0",
        &resolver,
        &options(&harness, root.path(), false),
        false,
    )
    .await
    .unwrap();

    let env = EnvLockfile::read(root.path()).unwrap().expect("env lockfile written");
    let pm_deps = env.importers[EnvLockfile::ROOT_IMPORTER_KEY]
        .package_manager_dependencies
        .as_ref()
        .expect("package manager deps recorded");
    assert_eq!(pm_deps.len(), 1);
    assert_eq!(pm_deps["pnpm"].specifier, "^6.0.0");
    assert_eq!(pm_deps["pnpm"].version, "6.16.0");
    assert!(!pm_deps.contains_key("@pnpm/exe"));

    let pnpm_key: PackageKey = "pnpm@6.16.0".parse().unwrap();
    assert!(env.packages.contains_key(&pnpm_key));
    assert!(env.snapshots.contains_key(&pnpm_key));
    assert!(is_package_manager_resolved(&env, "^6.0.0", "6.16.0"));
    assert!(!is_package_manager_resolved(&env, "^6.0.0", "6.17.1"));
}

#[tokio::test]
async fn resolves_package_manager_dependencies_with_exe_at_first_published_version() {
    let harness = harness();
    let root = TempDir::new().unwrap();
    let resolver = FixtureResolver::new()
        .package(serde_json::json!({
            "name": "pnpm",
            "version": "6.17.1",
            "bin": { "pnpm": "bin/pnpm.cjs", "pnpx": "bin/pnpx.cjs" },
        }))
        .package(serde_json::json!({
            "name": "@pnpm/exe",
            "version": "6.17.1",
            "bin": { "pnpm": "pnpm" },
            "optionalDependencies": { "@pnpm/macos-arm64": "6.17.1" },
        }))
        .package(serde_json::json!({
            "name": "@pnpm/macos-arm64",
            "version": "6.17.1",
            "cpu": ["arm64"],
            "os": ["darwin"],
        }));

    resolve_package_manager_integrities(
        pnpm_engine_packages("6.17.1"),
        "^6.0.0",
        "6.17.1",
        &resolver,
        &options(&harness, root.path(), false),
        false,
    )
    .await
    .unwrap();

    let env = EnvLockfile::read(root.path()).unwrap().expect("env lockfile written");
    let pm_deps = env.importers[EnvLockfile::ROOT_IMPORTER_KEY]
        .package_manager_dependencies
        .as_ref()
        .expect("package manager deps recorded");
    assert_eq!(pm_deps.len(), 2);
    assert_eq!(pm_deps["pnpm"].version, "6.17.1");
    assert_eq!(pm_deps["@pnpm/exe"].version, "6.17.1");

    let exe_key: PackageKey = "@pnpm/exe@6.17.1".parse().unwrap();
    let platform_key: PackageKey = "@pnpm/macos-arm64@6.17.1".parse().unwrap();
    assert!(env.packages.contains_key(&exe_key));
    assert!(env.packages.contains_key(&platform_key));
    assert!(is_package_manager_resolved(&env, "^6.0.0", "6.17.1"));
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
