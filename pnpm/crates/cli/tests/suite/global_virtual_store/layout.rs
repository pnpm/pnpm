use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, Path, append_workspace_yaml_key,
    assert_no_post_import_linking, fs, gvs_root, harness_store_and_cache_yaml, pacquet,
    pkg_in_slot, pkg_version_dir, read_modules_manifest, set_gvs_workspace_yaml, sole_hash_dir,
    write_manifest,
};
use assert_cmd::assert::OutputAssertExt;

#[test]
fn pnp_without_symlinks_repairs_a_missing_global_virtual_store_package() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    set_gvs_workspace_yaml(&workspace, "nodeLinker: pnp\nsymlink: false\n");
    write_manifest(&workspace, &serde_json::json!({ "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }));

    pacquet(&workspace).with_arg("install").assert().success();

    let version_dir = pkg_version_dir(&store_dir, "@pnpm.e2e/pkg-with-1-dep", "100.0.0");
    let package_dir = pkg_in_slot(&sole_hash_dir(&version_dir), "@pnpm.e2e/pkg-with-1-dep");
    fs::remove_dir_all(&package_dir).expect("remove package from the GVS slot");

    pacquet(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();
    assert!(
        package_dir.join("package.json").is_file(),
        "a frozen PnP install must repair a missing GVS package when symlinks are disabled",
    );

    drop((root, mock_instance));
}

/// TS: `virtualStoreOnly populates standard virtual store without importer
/// symlinks` (`globalVirtualStore.ts:539`).
#[test]
fn virtual_store_only_populates_standard_virtual_store_without_importer_symlinks() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, &serde_json::json!({ "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }));
    append_workspace_yaml_key(&workspace, "virtualStoreOnly", true);

    pacquet(&workspace).with_arg("install").assert().success();

    assert!(
        workspace
            .join("node_modules/.pnpm/@pnpm.e2e+pkg-with-1-dep@100.0.0/node_modules/@pnpm.e2e/pkg-with-1-dep/package.json")
            .exists(),
        "the standard virtual store must still be populated",
    );
    assert!(
        !workspace.join("node_modules/@pnpm.e2e/pkg-with-1-dep").exists(),
        "importer-level symlinks must not be created",
    );

    drop((root, mock_instance));
}

/// TS: `virtualStoreOnly with enableModulesDir=false throws config error
/// (standard virtual store)` (`globalVirtualStore.ts:559`). Without a
/// global virtual store there is nowhere to put the packages, because the
/// standard one lives inside `node_modules`.
#[test]
fn virtual_store_only_with_no_modules_dir_is_a_config_conflict() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, &serde_json::json!({}));
    append_workspace_yaml_key(&workspace, "virtualStoreOnly", true);
    append_workspace_yaml_key(&workspace, "enableModulesDir", false);

    let output = pacquet(&workspace).with_arg("install").assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_CONFIG_CONFLICT_VIRTUAL_STORE_ONLY_WITH_NO_MODULES_DIR"),
        "the conflict must surface pnpm's error code; got: {stderr}",
    );

    drop((root, mock_instance));
}

/// TS: `virtualStoreOnly with enableModulesDir=false works when GVS is
/// enabled` (`globalVirtualStore.ts:571`). The global virtual store lives
/// outside `node_modules`, so the same combination becomes legal.
#[test]
fn virtual_store_only_with_no_modules_dir_works_when_gvs_is_enabled() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, &serde_json::json!({ "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }));

    eprintln!("First install with the modules dir enabled, to produce a lockfile...");
    set_gvs_workspace_yaml(&workspace, "");
    pacquet(&workspace).with_arg("install").assert().success();

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    fs::remove_dir_all(gvs_root(&store_dir)).expect("remove the GVS root");

    eprintln!("Now virtualStoreOnly + enableModulesDir=false + GVS — must not throw...");
    set_gvs_workspace_yaml(&workspace, "virtualStoreOnly: true\nenableModulesDir: false\n");
    pacquet(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

    let version_dir = pkg_version_dir(&store_dir, "@pnpm.e2e/pkg-with-1-dep", "100.0.0");
    let hash_dir = sole_hash_dir(&version_dir);
    assert!(
        pkg_in_slot(&hash_dir, "@pnpm.e2e/pkg-with-1-dep").join("package.json").exists(),
        "the GVS must be populated even with no modules dir",
    );

    drop((root, mock_instance));
}

/// TS: `virtualStoreOnly with GVS populates global virtual store without
/// importer links` (`globalVirtualStore.ts:605`).
#[test]
fn virtual_store_only_with_gvs_populates_the_store_without_importer_links() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, &serde_json::json!({ "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }));
    set_gvs_workspace_yaml(&workspace, "virtualStoreOnly: true\n");

    pacquet(&workspace).with_arg("install").assert().success();

    let version_dir = pkg_version_dir(&store_dir, "@pnpm.e2e/pkg-with-1-dep", "100.0.0");
    let hash_dir = sole_hash_dir(&version_dir);
    assert!(
        pkg_in_slot(&hash_dir, "@pnpm.e2e/pkg-with-1-dep").join("package.json").exists(),
        "the GVS must be populated",
    );
    assert!(
        pkg_in_slot(&hash_dir, "@pnpm.e2e/dep-of-pkg-with-1-dep").join("package.json").exists(),
        "the transitive dep must be materialized in the slot too",
    );

    assert_no_post_import_linking(&workspace);

    drop((root, mock_instance));
}

/// TS: `virtualStoreOnly with frozenLockfile populates virtual store
/// without importer symlinks` (`globalVirtualStore.ts:635`).
#[test]
fn virtual_store_only_with_frozen_lockfile_populates_the_gvs_without_importer_symlinks() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, &serde_json::json!({ "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }));

    eprintln!("First install to produce a lockfile...");
    set_gvs_workspace_yaml(&workspace, "");
    pacquet(&workspace).with_arg("install").assert().success();

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    fs::remove_dir_all(gvs_root(&store_dir)).expect("remove the GVS root");

    eprintln!("Frozen reinstall with virtualStoreOnly...");
    set_gvs_workspace_yaml(&workspace, "virtualStoreOnly: true\n");
    pacquet(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

    let version_dir = pkg_version_dir(&store_dir, "@pnpm.e2e/pkg-with-1-dep", "100.0.0");
    let hash_dir = sole_hash_dir(&version_dir);
    assert!(
        pkg_in_slot(&hash_dir, "@pnpm.e2e/pkg-with-1-dep").join("package.json").exists(),
        "the GVS must be populated",
    );
    assert!(
        pkg_in_slot(&hash_dir, "@pnpm.e2e/dep-of-pkg-with-1-dep").join("package.json").exists(),
        "the transitive dep must be materialized in the slot too",
    );

    assert_no_post_import_linking(&workspace);

    drop((root, mock_instance));
}

/// TS: `virtualStoreOnly with frozenLockfile populates standard virtual
/// store without importer symlinks` (`globalVirtualStore.ts:677`).
#[test]
fn virtual_store_only_with_frozen_lockfile_populates_the_standard_store() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, &serde_json::json!({ "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }));

    eprintln!("First install to produce a lockfile...");
    pacquet(&workspace).with_arg("install").assert().success();

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    eprintln!("Frozen reinstall with virtualStoreOnly...");
    append_workspace_yaml_key(&workspace, "virtualStoreOnly", true);
    pacquet(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

    assert!(
        workspace
            .join("node_modules/.pnpm/@pnpm.e2e+pkg-with-1-dep@100.0.0/node_modules/@pnpm.e2e/pkg-with-1-dep/package.json")
            .exists(),
        "the standard virtual store must be populated",
    );

    assert_no_post_import_linking(&workspace);

    drop((root, mock_instance));
}

/// TS: `virtualStoreOnly suppresses hoisting even with explicit
/// hoistPattern` (`globalVirtualStore.ts:708`). The flag wins over an
/// explicit opt-in on both hoist patterns.
#[test]
fn virtual_store_only_suppresses_hoisting_even_with_explicit_hoist_pattern() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, &serde_json::json!({ "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }));
    append_workspace_yaml_key(&workspace, "virtualStoreOnly", true);
    append_workspace_yaml_key(&workspace, "hoistPattern", "['*']");
    append_workspace_yaml_key(&workspace, "publicHoistPattern", "['*']");

    pacquet(&workspace).with_arg("install").assert().success();

    assert!(
        workspace
            .join("node_modules/.pnpm/@pnpm.e2e+pkg-with-1-dep@100.0.0/node_modules/@pnpm.e2e/pkg-with-1-dep/package.json")
            .exists(),
        "the virtual store must still be populated",
    );

    assert_no_post_import_linking(&workspace);

    drop((root, mock_instance));
}

/// A `virtualStoreOnly` install must leave the project in a state a
/// following ordinary install completes rather than purges — the empty
/// hoist patterns it records are deliberate, not drift.
///
/// Pacquet-only: upstream encodes this as the `!modules.virtualStoreOnly`
/// guards inside `validateModules.ts`, which has no test of its own.
#[test]
fn ordinary_install_after_virtual_store_only_completes_the_linking() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, &serde_json::json!({ "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }));
    append_workspace_yaml_key(&workspace, "virtualStoreOnly", true);
    append_workspace_yaml_key(&workspace, "hoistPattern", "['*']");

    eprintln!("virtualStoreOnly install...");
    pacquet(&workspace).with_arg("install").assert().success();
    assert_eq!(
        read_modules_manifest(&workspace).virtual_store_only,
        Some(true),
        ".modules.yaml must record that this install was virtualStoreOnly",
    );

    eprintln!("Ordinary install with the same hoistPattern must complete the linking...");
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(&yaml_path, yaml.replace("virtualStoreOnly: true\n", ""))
        .expect("write pnpm-workspace.yaml");
    pacquet(&workspace).with_arg("install").assert().success();

    assert!(
        workspace.join("node_modules/@pnpm.e2e/pkg-with-1-dep/package.json").exists(),
        "the follow-up install must create the importer symlinks",
    );
    assert_eq!(
        read_modules_manifest(&workspace).virtual_store_only,
        None,
        "the flag must be cleared once an ordinary install has completed the linking",
    );

    drop((root, mock_instance));
}

/// With a global virtual store, package directories live outside the
/// project, so Node's upward `node_modules` walk from their real paths
/// cannot reach the private hoist. Scripts must receive `NODE_PATH` plus
/// the ESM `NODE_PATH` loader flag in `NODE_OPTIONS`, so both CJS and ESM
/// phantom imports keep resolving. Mirrors the TS coverage in
/// `pnpm11/config/reader/test/index.ts` and
/// `pnpm11/exec/esm-node-path-loader/test/index.ts`.
#[test]
fn scripts_resolve_phantom_esm_imports_through_the_private_hoist() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    set_gvs_workspace_yaml(&workspace, "privateHoistPattern:\n  - '*'\n");
    let manifest = serde_json::json!({
        "name": "project-with-phantom-esm-import",
        "version": "1.0.0",
        "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" },
        "scripts": { "check": "node check.mjs" },
    });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");
    fs::write(
        workspace.join("check.mjs"),
        "import fs from 'node:fs'\n\
         fs.writeFileSync('node-path.txt', process.env.NODE_PATH ?? '<unset>')\n\
         fs.writeFileSync('node-options.txt', process.env.NODE_OPTIONS ?? '<unset>')\n\
         await import('@pnpm.e2e/dep-of-pkg-with-1-dep')\n\
         fs.writeFileSync('phantom.txt', 'resolved')\n",
    )
    .expect("write check.mjs");

    pacquet(&workspace).with_arg("install").assert().success();
    pacquet(&workspace).with_args(["run", "check"]).assert().success();

    let node_path =
        fs::read_to_string(workspace.join("node-path.txt")).expect("read node-path.txt");
    let path_delimiter = if cfg!(windows) { ';' } else { ':' };
    assert!(
        node_path
            .split(path_delimiter)
            .any(|entry| Path::new(entry).ends_with("node_modules/.pnpm/node_modules")),
        "NODE_PATH must carry the private hoist dir: {node_path}",
    );
    let node_options =
        fs::read_to_string(workspace.join("node-options.txt")).expect("read node-options.txt");
    assert!(
        node_options
            .contains(pnpm_config::esm_node_path_loader::esm_node_path_loader_import_flag()),
        "NODE_OPTIONS must carry the ESM NODE_PATH loader flag: {node_options}",
    );
    assert_eq!(
        fs::read_to_string(workspace.join("phantom.txt")).expect("read phantom.txt"),
        "resolved",
    );

    drop((root, mock_instance));
}

/// Driven through a real install: the setting only means anything once
/// something has been materialized somewhere.
#[test]
fn virtual_store_type_selects_where_packages_are_materialized() {
    for (yaml, expect_shared) in [
        ("virtualStoreType: project\n", false),
        ("virtualStoreType: global\n", true),
        ("enableGlobalVirtualStore: false\n", false),
        ("virtualStoreType: global\nenableGlobalVirtualStore: false\n", true),
        ("virtualStoreType: project\nenableGlobalVirtualStore: true\n", false),
    ] {
        let CommandTempCwd { root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

        let mut workspace_yaml = harness_store_and_cache_yaml(&workspace);
        workspace_yaml.push_str(yaml);
        fs::write(workspace.join("pnpm-workspace.yaml"), workspace_yaml)
            .expect("write pnpm-workspace.yaml");
        write_manifest(&workspace, &serde_json::json!({ "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }));

        pacquet(&workspace).with_arg("install").assert().success();

        let project_local_slot =
            workspace.join("node_modules/.pnpm/@pnpm.e2e+pkg-with-1-dep@100.0.0");
        // On macOS a project-local install still materializes the
        // canonical slots as the directory-clone cache
        // (`pnpm-deps-restorer/src/dir_clone_cache.rs`), so the links
        // root exists in every row there; where the *install* lives is
        // pinned by the project-local-slot assertion below.
        let expect_links_root = expect_shared || cfg!(target_os = "macos");
        assert_eq!(
            gvs_root(&store_dir).is_dir(),
            expect_links_root,
            "the shared store is populated only when asked for (or as the macOS \
             directory-clone cache); yaml: {yaml}",
        );
        assert_eq!(
            project_local_slot.is_dir(),
            !expect_shared,
            "the project-local slot is written only when asked for; yaml: {yaml}",
        );

        drop((root, mock_instance));
    }
}

/// The isolated linker's half of the package-map gate, mirroring
/// `hoisted_install_writes_no_package_map_unless_the_setting_is_on`.
///
/// An install that stops writing the map also has to take away the one
/// a previous install left: `pnpm run` finds the file by existence, so
/// a map kept across a dependency change would describe a `node_modules`
/// that has moved on. A repeat install with nothing to do never reaches
/// the link phase and so leaves the file alone, which is harmless —
/// with the setting off nothing reads it, and it still matches the
/// installed tree.
#[test]
fn an_isolated_install_clears_a_package_map_it_stops_maintaining() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, &serde_json::json!({ "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }));
    set_gvs_workspace_yaml(&workspace, "nodeExperimentalPackageMap: true\n");
    pacquet(&workspace).with_arg("install").assert().success();

    let package_map = workspace.join("node_modules/.package-map.json");
    assert!(package_map.is_file(), "the setting must produce a map to begin with");

    eprintln!("Adding a dependency with the setting back off...");
    set_gvs_workspace_yaml(&workspace, "");
    write_manifest(
        &workspace,
        &serde_json::json!({ "@pnpm.e2e/pkg-with-1-dep": "100.0.0", "@pnpm.e2e/foo": "100.0.0" }),
    );
    pacquet(&workspace).with_arg("install").assert().success();

    assert!(
        !package_map.exists(),
        "a map describing the previous dependency set must not survive at {package_map:?}",
    );

    drop((root, mock_instance));
}
