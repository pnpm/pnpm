use super::{
    BTreeMap, ConfigDepError, ConfigDependency, EnvLockfile, FixtureResolver, LockfileResolution,
    PackageKey, SilentReporter, SpecifierAndResolution, TempDir, build_resolver, clean_spec,
    harness, integrity_of, options, pnpm_engine_packages, resolve_and_install_config_deps,
    resolve_package_manager_integrities,
};

/// The entries a forced resync repairs already record the pinned version,
/// so `--frozen-lockfile` re-resolves them in memory: the caller gets usable
/// entries and the lockfile on disk keeps the bytes it had.
#[tokio::test]
async fn force_resync_under_frozen_lockfile_resolves_without_writing() {
    let harness = harness();
    let root = TempDir::new().unwrap();
    let recorded = FixtureResolver::new().package(serde_json::json!({
        "name": "pnpm",
        "version": "12.0.0",
        "bin": "bin/pnpm.cjs",
    }));
    resolve_package_manager_integrities(
        pnpm_engine_packages("12.0.0"),
        "^12.0.0",
        "12.0.0",
        &recorded,
        &options(&harness, root.path(), false),
        false,
    )
    .await
    .unwrap();
    let lockfile_path = root.path().join("pnpm-lock.yaml");
    let before = std::fs::read_to_string(&lockfile_path).unwrap();

    // A resync that would add an entry, under a frozen lockfile.
    let repaired = FixtureResolver::new()
        .package(serde_json::json!({
            "name": "pnpm",
            "version": "12.0.0",
            "bin": "bin/pnpm.cjs",
            "optionalDependencies": { "@pnpm/exe.linux-x64": "12.0.0" },
        }))
        .package(serde_json::json!({
            "name": "@pnpm/exe.linux-x64",
            "version": "12.0.0",
        }));
    let env = resolve_package_manager_integrities(
        pnpm_engine_packages("12.0.0"),
        "^12.0.0",
        "12.0.0",
        &repaired,
        &options(&harness, root.path(), true),
        true,
    )
    .await
    .expect("a forced resync is a repair, not a lockfile update");

    let platform_key: PackageKey = "@pnpm/exe.linux-x64@12.0.0".parse().unwrap();
    assert!(env.packages.contains_key(&platform_key), "the caller gets the repaired closure");
    for (key, metadata) in &env.packages {
        assert!(
            matches!(&metadata.resolution, LockfileResolution::Registry(resolution)
                if !resolution.integrity.to_string().is_empty()),
            "the bootstrap only reads integrity-only registry resolutions, {key} has {:?}",
            metadata.resolution,
        );
    }
    assert_eq!(std::fs::read_to_string(&lockfile_path).unwrap(), before);
}

/// A pnpm below 11.20.0 pins `@pnpm/exe` beside `pnpm` for a v12 version.
/// The entry pins the wanted version and cannot change which pnpm runs, so a
/// frozen lockfile accepts the block a teammate's older pnpm left behind,
/// and a writable install rewrites it to the packages this pnpm installs
/// from.
#[tokio::test]
async fn frozen_lockfile_accepts_an_engine_package_it_does_not_install_from() {
    let harness = harness();
    let root = TempDir::new().unwrap();
    let fixtures = || {
        FixtureResolver::new()
            .package(serde_json::json!({
                "name": "pnpm",
                "version": "12.0.0",
                "bin": "bin/pnpm.cjs",
            }))
            .package(serde_json::json!({
                "name": "@pnpm/exe",
                "version": "12.0.0",
                "bin": "pnpm",
            }))
    };
    // The set an older pnpm records for a v12 pin.
    resolve_package_manager_integrities(
        &["pnpm", "@pnpm/exe"],
        "^12.0.0",
        "12.0.0",
        &fixtures(),
        &options(&harness, root.path(), false),
        false,
    )
    .await
    .unwrap();
    let lockfile_path = root.path().join("pnpm-lock.yaml");
    let before = std::fs::read_to_string(&lockfile_path).unwrap();

    resolve_package_manager_integrities(
        pnpm_engine_packages("12.0.0"),
        "^12.0.0",
        "12.0.0",
        &fixtures(),
        &options(&harness, root.path(), true),
        false,
    )
    .await
    .expect("the block pins the wanted version, so there is nothing to update");
    assert_eq!(std::fs::read_to_string(&lockfile_path).unwrap(), before);

    let env = resolve_package_manager_integrities(
        pnpm_engine_packages("12.0.0"),
        "^12.0.0",
        "12.0.0",
        &fixtures(),
        &options(&harness, root.path(), false),
        false,
    )
    .await
    .unwrap();

    let recorded: Vec<&str> = env.importers[EnvLockfile::ROOT_IMPORTER_KEY]
        .package_manager_dependencies
        .as_ref()
        .expect("recorded package manager dependencies")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(recorded, ["pnpm"], "a writable install records what it installs from");
}

/// The tolerance is for a package pinned at the wanted version. One pinning
/// anything else is a block that disagrees with the manifest.
#[tokio::test]
async fn frozen_lockfile_rejects_an_engine_package_pinned_at_another_version() {
    let harness = harness();
    let root = TempDir::new().unwrap();
    let resolver = FixtureResolver::new()
        .package(serde_json::json!({
            "name": "pnpm",
            "version": "12.0.0",
            "bin": "bin/pnpm.cjs",
        }))
        .package(serde_json::json!({
            "name": "@pnpm/exe",
            "version": "12.0.0",
            "bin": "pnpm",
        }));
    resolve_package_manager_integrities(
        &["pnpm", "@pnpm/exe"],
        "^12.0.0",
        "12.0.0",
        &resolver,
        &options(&harness, root.path(), false),
        false,
    )
    .await
    .unwrap();
    let mut env_lockfile = EnvLockfile::read(root.path()).unwrap().expect("env lockfile written");
    env_lockfile
        .root_importer_mut()
        .package_manager_dependencies
        .as_mut()
        .expect("recorded package manager dependencies")
        .insert(
            "@pnpm/exe".to_string(),
            SpecifierAndResolution {
                specifier: "^12.0.0".to_string(),
                version: "11.23.0".to_string(),
            },
        );
    env_lockfile.write(root.path()).unwrap();
    let lockfile_path = root.path().join("pnpm-lock.yaml");
    let before = std::fs::read_to_string(&lockfile_path).unwrap();

    let error = resolve_package_manager_integrities(
        pnpm_engine_packages("12.0.0"),
        "^12.0.0",
        "12.0.0",
        &resolver,
        &options(&harness, root.path(), true),
        false,
    )
    .await
    .expect_err("an entry pinning another version is an outdated lockfile");

    assert!(matches!(error, ConfigDepError::FrozenLockfileOutdated { .. }), "{error:?}");
    assert_eq!(std::fs::read_to_string(&lockfile_path).unwrap(), before);
}

/// Entries that do not record the pinned version are the case
/// `--frozen-lockfile` exists for: recording them would take the lockfile
/// out of sync with the manifest, so the command fails instead, and the
/// lockfile it refused to update keeps the bytes it had.
#[tokio::test]
async fn frozen_lockfile_rejects_outdated_package_manager_entries() {
    let harness = harness();
    let root = TempDir::new().unwrap();
    let resolver = || {
        FixtureResolver::new().package(serde_json::json!({
            "name": "pnpm",
            "version": "12.0.0",
            "bin": "bin/pnpm.cjs",
        }))
    };
    let outdated = resolve_package_manager_integrities(
        pnpm_engine_packages("12.0.0"),
        "^12.0.0",
        "12.0.0",
        &resolver(),
        &options(&harness, root.path(), true),
        false,
    )
    .await
    .expect_err("a missing entry has to be recorded, which a frozen lockfile forbids");

    assert!(matches!(outdated, ConfigDepError::FrozenLockfileOutdated { .. }), "{outdated:?}");
    assert!(!root.path().join("pnpm-lock.yaml").exists());

    // The same refusal once a lockfile exists: an entry recorded for another
    // version is what a bumped pin leaves behind.
    resolve_package_manager_integrities(
        pnpm_engine_packages("12.0.0"),
        "^12.0.0",
        "12.0.0",
        &resolver(),
        &options(&harness, root.path(), false),
        false,
    )
    .await
    .unwrap();
    let lockfile_path = root.path().join("pnpm-lock.yaml");
    let before = std::fs::read_to_string(&lockfile_path).unwrap();

    let stale = resolve_package_manager_integrities(
        pnpm_engine_packages("13.0.0"),
        "^13.0.0",
        "13.0.0",
        &FixtureResolver::new().package(serde_json::json!({
            "name": "pnpm",
            "version": "13.0.0",
            "bin": "bin/pnpm.cjs",
        })),
        &options(&harness, root.path(), true),
        false,
    )
    .await
    .expect_err("the recorded entry pins another version");

    assert!(matches!(stale, ConfigDepError::FrozenLockfileOutdated { .. }), "{stale:?}");
    assert_eq!(std::fs::read_to_string(&lockfile_path).unwrap(), before);
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
