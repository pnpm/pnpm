use super::{
    EnvLockfile, FixtureResolver, LockfileResolution, PackageKey, PackageMetadata,
    RegistryResolution, SnapshotDepRef, SnapshotEntry, SpecifierAndResolution, TempDir, harness,
    is_package_manager_resolved, options, pnpm_engine_packages, prune_env_lockfile,
    resolve_package_manager_integrities,
};

/// `force_resync` discards recorded entries that look up to date, so
/// invalid resolutions written by an earlier pnpm are healed instead of
/// surviving the fast path.
#[tokio::test]
async fn force_resync_overwrites_recorded_package_manager_entries() {
    let harness = harness();
    let root = TempDir::new().unwrap();
    let fixtures = || {
        FixtureResolver::new().package(serde_json::json!({
            "name": "pnpm",
            "version": "12.0.0",
            "bin": "bin/pnpm.cjs",
        }))
    };

    resolve_package_manager_integrities(
        pnpm_engine_packages("12.0.0"),
        "^12.0.0",
        "12.0.0",
        &fixtures().with_non_derivable_tarball_urls(),
        &options(&harness, root.path(), false),
        false,
    )
    .await
    .unwrap();
    let env = EnvLockfile::read(root.path()).unwrap().expect("env lockfile written");
    assert!(is_package_manager_resolved(&env, "^12.0.0", "12.0.0"));

    // Without force, the recorded entries short-circuit resolution; with
    // force, they are re-resolved and rewritten.
    resolve_package_manager_integrities(
        pnpm_engine_packages("12.0.0"),
        "^12.0.0",
        "12.0.0",
        &fixtures(),
        &options(&harness, root.path(), false),
        true,
    )
    .await
    .unwrap();
    let env = EnvLockfile::read(root.path()).unwrap().expect("env lockfile written");
    let key: PackageKey = "pnpm@12.0.0".parse().unwrap();
    assert!(matches!(&env.packages[&key].resolution, LockfileResolution::Registry(_)));
}

#[test]
fn prune_drops_orphan_packages_and_snapshots() {
    fn registry_pkg() -> PackageMetadata {
        PackageMetadata {
            resolution: LockfileResolution::Registry(RegistryResolution {
                integrity: ssri::Integrity::from(b"x"),
                revision: None,
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
