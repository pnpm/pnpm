//! The global-virtual-store tests that exercise the build policy.

use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, StoreDir, StoreIndex, allow_builds_yaml, fs,
    hash_dirs, is_symlink_or_junction, pacquet, pkg_in_slot, pkg_version_dir,
    read_modules_manifest, set_gvs_workspace_yaml, sole_hash_dir, write_manifest,
};
use assert_cmd::assert::OutputAssertExt;
use pnpm_testing_utils::git_repo::GitRepoFixture;

/// TS: `GVS hashes are engine-agnostic for packages not in allowBuilds`
/// (`globalVirtualStore.ts:132`).
///
/// The hash of a package covers the engine only when the package — or
/// something in its dependency closure — is allowed to build. Allowing
/// the *transitive* dep therefore has to change the *parent's* hash.
#[test]
fn gvs_hashes_are_engine_agnostic_for_packages_not_in_allow_builds() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, &serde_json::json!({ "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }));

    eprintln!("Scenario 1: nothing may build — hashes must omit the engine...");
    set_gvs_workspace_yaml(&workspace, &allow_builds_yaml(&[]));
    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let version_dir = pkg_version_dir(&store_dir, "@pnpm.e2e/pkg-with-1-dep", "100.0.0");
    let hash_no_builds = sole_hash_dir(&version_dir);

    eprintln!("Scenario 2: the transitive dep may build — the parent hash must change...");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    set_gvs_workspace_yaml(
        &workspace,
        &allow_builds_yaml(&[("@pnpm.e2e/dep-of-pkg-with-1-dep", true)]),
    );
    pacquet(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();

    let hashes_after = hash_dirs(&version_dir);
    let hash_no_builds_name = hash_no_builds
        .file_name()
        .expect("hash dir name")
        .to_string_lossy()
        .into_owned();
    let hash_with_builds = hashes_after
        .iter()
        .find(|hash| *hash != &hash_no_builds_name)
        .expect("allowing a transitive dep to build must produce a new, engine-specific hash");

    assert_ne!(
        hash_with_builds, &hash_no_builds_name,
        "the engine-agnostic and engine-specific hashes must differ",
    );
    assert!(
        pkg_in_slot(&hash_no_builds, "@pnpm.e2e/pkg-with-1-dep").join("package.json").exists(),
        "the engine-agnostic slot must still be a valid layout",
    );
    assert!(
        pkg_in_slot(&version_dir.join(hash_with_builds), "@pnpm.e2e/pkg-with-1-dep")
            .join("package.json")
            .exists(),
        "the engine-specific slot must be a valid layout too",
    );

    drop((root, mock_instance));
}

/// TS: `GVS hashes are stable when allowBuilds targets an unrelated
/// package` (`globalVirtualStore.ts:172`). The complement of the previous
/// test: an `allowBuilds` entry outside the dependency closure must not
/// perturb any hash.
#[test]
fn gvs_hashes_are_stable_when_allow_builds_targets_an_unrelated_package() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, &serde_json::json!({ "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }));

    eprintln!("Scenario 1: nothing may build...");
    set_gvs_workspace_yaml(&workspace, &allow_builds_yaml(&[]));
    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let version_dir = pkg_version_dir(&store_dir, "@pnpm.e2e/pkg-with-1-dep", "100.0.0");
    let hashes_before = hash_dirs(&version_dir);

    eprintln!("Scenario 2: an unrelated package may build...");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    set_gvs_workspace_yaml(&workspace, &allow_builds_yaml(&[("some-unrelated-package", true)]));
    pacquet(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();

    assert_eq!(
        hash_dirs(&version_dir),
        hashes_before,
        "an allowBuilds entry outside the dependency closure must not change any hash",
    );

    drop((root, mock_instance));
}

/// TS: `GVS re-links when allowBuilds changes` (`globalVirtualStore.ts:205`):
/// the approval set the install ran under has to round-trip through
/// `.modules.yaml`.
#[test]
fn gvs_relinks_when_allow_builds_changes() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, &serde_json::json!({ "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }));

    eprintln!("Installing with nothing allowed to build...");
    set_gvs_workspace_yaml(&workspace, &allow_builds_yaml(&[]));
    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let version_dir = pkg_version_dir(&store_dir, "@pnpm.e2e/pkg-with-1-dep", "100.0.0");
    let hash_before = sole_hash_dir(&version_dir)
        .file_name()
        .expect("hash dir name")
        .to_string_lossy()
        .into_owned();

    assert_eq!(
        read_modules_manifest(&workspace).allow_builds,
        Some(std::collections::BTreeMap::new()),
        "an empty allowBuilds must round-trip as an empty map, not as an absent key",
    );

    eprintln!("Reinstalling with the transitive dep allowed to build...");
    set_gvs_workspace_yaml(
        &workspace,
        &allow_builds_yaml(&[("@pnpm.e2e/dep-of-pkg-with-1-dep", true)]),
    );
    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let hash_after = hash_dirs(&version_dir)
        .into_iter()
        .find(|hash| hash != &hash_before)
        .expect("an allowBuilds change must produce a new hash directory");
    assert!(
        pkg_in_slot(&version_dir.join(&hash_after), "@pnpm.e2e/pkg-with-1-dep")
            .join("package.json")
            .exists(),
        "the re-linked slot must be a valid layout",
    );

    let expected = std::collections::BTreeMap::from([(
        "@pnpm.e2e/dep-of-pkg-with-1-dep".to_string(),
        pnpm_modules_yaml::AllowBuildValue::Bool(true),
    )]);
    assert_eq!(
        read_modules_manifest(&workspace).allow_builds,
        Some(expected),
        ".modules.yaml must record the allowBuilds set the install ran under",
    );

    drop((root, mock_instance));
}

/// TS: `GVS successful build creates package directory with build
/// artifacts` (`globalVirtualStore.ts:250`).
#[test]
fn gvs_successful_build_creates_package_directory_with_build_artifacts() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        &serde_json::json!({ "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" }),
    );
    set_gvs_workspace_yaml(
        &workspace,
        &allow_builds_yaml(&[("@pnpm.e2e/pre-and-postinstall-scripts-example", true)]),
    );

    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let version_dir =
        pkg_version_dir(&store_dir, "@pnpm.e2e/pre-and-postinstall-scripts-example", "1.0.0");
    let pkg =
        pkg_in_slot(&sole_hash_dir(&version_dir), "@pnpm.e2e/pre-and-postinstall-scripts-example");

    assert!(pkg.join("package.json").exists(), "the built package must be in its GVS slot");
    assert!(
        pkg.join("generated-by-preinstall.js").exists(),
        "the preinstall artifact must be written into the GVS slot",
    );
    assert!(
        pkg.join("generated-by-postinstall.js").exists(),
        "the postinstall artifact must be written into the GVS slot",
    );
    assert!(
        !pkg.join(".pnpm-needs-build").exists(),
        "a successful build must remove its incomplete-build marker",
    );

    let store = StoreDir::new(&store_dir);
    let index = StoreIndex::open_readonly_in(&store).expect("open the store index");
    let row_key = index
        .keys()
        .expect("list the store index keys")
        .into_iter()
        .find(|key| key.ends_with("\t@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0"))
        .expect("the built package must have a store index row");
    let row = index
        .get(&row_key)
        .expect("read the store index row")
        .expect("row exists");
    if let Some(side_effects) = row.side_effects {
        for diff in side_effects.values() {
            assert!(
                !diff.added
                    .as_ref()
                    .is_some_and(|added| added.contains_key(".pnpm-needs-build")),
                "the incomplete-build marker must not be uploaded to the side-effects cache",
            );
        }
    }

    drop((root, mock_instance));
}

/// An install into a fresh `node_modules` runs the build of a slot an earlier
/// install built when the side-effects cache has nothing to restore for it.
/// The postinstall writes only into the installing project, so the
/// side-effects cache row it leaves is empty
/// ([pnpm/pnpm#17526](https://github.com/pnpm/pnpm/issues/17526)).
#[test]
fn gvs_reinstall_builds_a_built_slot_with_nothing_cached() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let repo = GitRepoFixture::init(root.path(), "hook-installer");
    repo.write_file(
        "package.json",
        &serde_json::json!({
            "name": "hook-installer",
            "version": "1.0.0",
            "scripts": {
                "postinstall": r#"node -e "require('fs').writeFileSync(require('path').join(process.env.INIT_CWD, 'hook-installed'), '')""#,
            },
        })
        .to_string(),
    );
    let spec = repo.git_url_at(&repo.commit("init"));
    write_manifest(&workspace, &serde_json::json!({ "hook-installer": spec }));
    set_gvs_workspace_yaml(
        &workspace,
        &allow_builds_yaml(&[(&format!("hook-installer@{spec}"), true)]),
    );
    let hook = workspace.join("hook-installed");

    for install in ["first", "second"] {
        pacquet(&workspace)
            .with_arg("install")
            .assert()
            .success();
        assert!(hook.exists(), "the {install} install did not run the postinstall");
        fs::remove_file(&hook).expect("remove the postinstall output");
        fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    }

    drop((root, mock_instance));
}

/// TS: `GVS: approve-builds scenario — install with no builds, then
/// reinstall with allowBuilds` (`globalVirtualStore.ts:290`). The
/// hash-directory move is what makes approval safe: the unbuilt slot stays
/// intact and the built one is a sibling.
#[test]
fn gvs_approve_builds_scenario_moves_artifacts_to_a_new_hash_dir() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        &serde_json::json!({ "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" }),
    );

    eprintln!("Installing with builds NOT approved...");
    set_gvs_workspace_yaml(
        &workspace,
        &format!("strictDepBuilds: false\n{}", allow_builds_yaml(&[])),
    );
    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let version_dir =
        pkg_version_dir(&store_dir, "@pnpm.e2e/pre-and-postinstall-scripts-example", "1.0.0");
    let hash_before = sole_hash_dir(&version_dir)
        .file_name()
        .expect("hash dir name")
        .to_string_lossy()
        .into_owned();
    assert!(
        !workspace
            .join("node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js")
            .exists(),
        "an unapproved package must not have run its postinstall",
    );

    eprintln!("Reinstalling with the build approved...");
    set_gvs_workspace_yaml(
        &workspace,
        &allow_builds_yaml(&[("@pnpm.e2e/pre-and-postinstall-scripts-example", true)]),
    );
    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let hash_after = hash_dirs(&version_dir)
        .into_iter()
        .find(|hash| hash != &hash_before)
        .expect("approving a build must produce a new hash directory");
    let pkg = pkg_in_slot(
        &version_dir.join(&hash_after),
        "@pnpm.e2e/pre-and-postinstall-scripts-example",
    );
    assert!(
        pkg.join("generated-by-preinstall.js").exists(),
        "the preinstall artifact must land in the new hash directory",
    );
    assert!(
        pkg.join("generated-by-postinstall.js").exists(),
        "the postinstall artifact must land in the new hash directory",
    );

    eprintln!("Artifacts must be reachable through node_modules...");
    let linked = workspace.join("node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example");
    assert!(is_symlink_or_junction(&linked).expect("stat the direct dep link"));
    assert!(
        linked.join("generated-by-preinstall.js").exists(),
        "the project link must resolve to the built slot",
    );
    assert!(
        linked.join("generated-by-postinstall.js").exists(),
        "the project link must resolve to the built slot",
    );

    drop((root, mock_instance));
}

/// TS: `GVS build failure keeps the slot and marks it for a rebuild`
/// (`globalVirtualStore.ts:338`).
///
/// A GVS hash directory is shared by every project whose dependency graph
/// hashes to it, so a failed build must not remove a slot other projects
/// may already link: <https://github.com/pnpm/pnpm/issues/15568>.
#[test]
fn gvs_build_failure_leaves_no_package_in_the_slot() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, &serde_json::json!({ "@pnpm.e2e/failing-postinstall": "1.0.0" }));
    set_gvs_workspace_yaml(
        &workspace,
        &allow_builds_yaml(&[("@pnpm.e2e/failing-postinstall", true)]),
    );

    eprintln!("Installing a package whose postinstall exits non-zero...");
    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .failure();

    let version_dir = pkg_version_dir(&store_dir, "@pnpm.e2e/failing-postinstall", "1.0.0");
    let scope_dir = sole_hash_dir(&version_dir).join("node_modules/@pnpm.e2e");
    let entries = fs::read_dir(&scope_dir)
        .map(|entries| {
            entries
                .map(|entry| entry.expect("read entry").file_name())
                .collect()
        })
        .unwrap_or_else(|_| Vec::new());
    assert!(
        entries.is_empty(),
        "a failed build must leave neither its package nor its staging directory: {entries:?}",
    );

    drop((root, mock_instance));
}

/// TS: `GVS removes an optional dependency whose build failed from its slot`
/// (`globalVirtualStore.ts`).
///
/// The package never reaches its slot, so the parent's link inside its own
/// shared slot finds nothing.
#[test]
fn gvs_removes_an_optional_dependency_whose_build_failed_from_its_slot() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        &serde_json::json!({ "@pnpm.e2e/pkg-with-failing-optional-dependency": "1.0.0" }),
    );
    set_gvs_workspace_yaml(
        &workspace,
        &allow_builds_yaml(&[("@pnpm.e2e/pkg-with-failing-postinstall", true)]),
    );
    let failed_version_dir =
        pkg_version_dir(&store_dir, "@pnpm.e2e/pkg-with-failing-postinstall", "1.0.0");
    let parent_version_dir =
        pkg_version_dir(&store_dir, "@pnpm.e2e/pkg-with-failing-optional-dependency", "1.0.0");

    for _ in 0..2 {
        pacquet(&workspace)
            .with_arg("install")
            .assert()
            .success();

        let failed_slot = sole_hash_dir(&failed_version_dir);
        assert!(
            !pkg_in_slot(&failed_slot, "@pnpm.e2e/pkg-with-failing-postinstall").exists(),
            "the optional dependency whose build failed must be removed from its slot",
        );
        let parent_slot = sole_hash_dir(&parent_version_dir);
        assert!(
            pkg_in_slot(&parent_slot, "@pnpm.e2e/pkg-with-failing-optional-dependency")
                .join("package.json")
                .exists(),
            "the parent must stay installed",
        );
        assert!(
            !pkg_in_slot(&parent_slot, "@pnpm.e2e/pkg-with-failing-postinstall")
                .join("package.json")
                .exists(),
            "the parent must not resolve the optional dependency whose build failed",
        );
    }

    drop((root, mock_instance));
}

/// TS: `rebuild keeps a global virtual store slot whose optional build failed`
/// (`building/commands/test/build/index.ts`).
///
/// An `--ignore-scripts` install places the pristine package in a slot of
/// its own. A rebuild builds into the slot of the built package, so its
/// failure leaves the pristine slot to the projects that link it.
#[test]
fn gvs_rebuild_keeps_the_slot_whose_optional_build_failed() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    let manifest = serde_json::json!({
        "optionalDependencies": { "@pnpm.e2e/failing-postinstall": "1.0.0" },
    });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");
    set_gvs_workspace_yaml(
        &workspace,
        &allow_builds_yaml(&[("@pnpm.e2e/failing-postinstall", true)]),
    );

    pacquet(&workspace)
        .with_args(["install", "--ignore-scripts"])
        .assert()
        .success();
    let version_dir = pkg_version_dir(&store_dir, "@pnpm.e2e/failing-postinstall", "1.0.0");
    let pristine = pkg_in_slot(&sole_hash_dir(&version_dir), "@pnpm.e2e/failing-postinstall");
    pacquet(&workspace)
        .with_arg("rebuild")
        .assert()
        .success();

    assert!(pristine.join("package.json").exists(), "a failed rebuild must keep the pristine slot");

    drop((root, mock_instance));
}

/// <https://github.com/pnpm/pnpm/issues/15568>: an install that does not
/// build a package must not share a slot with the built package, or a later
/// install would have to build over files other projects link.
#[test]
fn gvs_ignore_scripts_and_building_installs_use_different_slots() {
    const PKG: &str = "@pnpm.e2e/pre-and-postinstall-scripts-example";

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, &serde_json::json!({ PKG: "1.0.0" }));
    set_gvs_workspace_yaml(&workspace, &allow_builds_yaml(&[(PKG, true)]));
    pacquet(&workspace)
        .with_args(["install", "--ignore-scripts"])
        .assert()
        .success();
    let version_dir = pkg_version_dir(&store_dir, PKG, "1.0.0");
    let pristine = pkg_in_slot(&sole_hash_dir(&version_dir), PKG);
    assert!(pristine.join("package.json").exists());
    assert!(!pristine.join("generated-by-postinstall.js").exists());

    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();

    assert_eq!(hash_dirs(&version_dir).len(), 2, "the built package must take a slot of its own");
    assert!(
        !pristine.join("generated-by-postinstall.js").exists(),
        "the build must not write into the pristine slot",
    );
    let linked = fs::canonicalize(workspace.join("node_modules").join(PKG)).expect("resolve link");
    assert!(linked.join("generated-by-postinstall.js").exists(), "the project links the build");

    drop((root, mock_instance));
}

/// TS: `GVS rebuilds successfully after simulated build failure cleanup`
/// (`globalVirtualStore.ts:367`). With the hash directory gone the warm
/// fast path must not fire — the install re-fetches, re-imports and
/// re-builds into a fresh slot.
#[test]
fn gvs_rebuilds_successfully_after_simulated_build_failure_cleanup() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        &serde_json::json!({ "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" }),
    );
    set_gvs_workspace_yaml(
        &workspace,
        &allow_builds_yaml(&[("@pnpm.e2e/pre-and-postinstall-scripts-example", true)]),
    );

    eprintln!("First install, with the build approved...");
    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let version_dir =
        pkg_version_dir(&store_dir, "@pnpm.e2e/pre-and-postinstall-scripts-example", "1.0.0");
    let hash_dir = sole_hash_dir(&version_dir);
    assert!(
        pkg_in_slot(&hash_dir, "@pnpm.e2e/pre-and-postinstall-scripts-example")
            .join("generated-by-postinstall.js")
            .exists(),
    );

    eprintln!("Simulating a previous build failure by removing the hash directory...");
    fs::remove_dir_all(&hash_dir).expect("remove the GVS hash dir");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    eprintln!("Frozen reinstall must rebuild the slot from scratch...");
    pacquet(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();

    let rebuilt = sole_hash_dir(&version_dir);
    assert!(
        pkg_in_slot(&rebuilt, "@pnpm.e2e/pre-and-postinstall-scripts-example")
            .join("generated-by-postinstall.js")
            .exists(),
        "the rebuilt slot must carry the build artifacts again",
    );

    drop((root, mock_instance));
}

/// A build that died before its package was renamed into the slot leaves
/// the slot without its package and, at worst, a staging directory. The
/// next install builds the package and places it.
#[test]
fn an_unplaced_build_is_placed_by_the_next_install() {
    const PKG: &str = "@pnpm.e2e/pre-and-postinstall-scripts-example";

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, &serde_json::json!({ PKG: "1.0.0" }));
    let workspace_settings =
        format!("{}sideEffectsCache: false\n", allow_builds_yaml(&[(PKG, true)]));
    set_gvs_workspace_yaml(&workspace, &workspace_settings);
    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let pkg = pkg_in_slot(&sole_hash_dir(&pkg_version_dir(&store_dir, PKG, "1.0.0")), PKG);
    let staging = pkg.with_file_name(".pre-and-postinstall-scripts-example-dead");
    fs::rename(&pkg, &staging).expect("leave the package in a dead install's staging directory");
    fs::remove_file(staging.join("generated-by-postinstall.js")).expect("remove build output");

    pacquet(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();

    assert!(
        pkg.join("generated-by-postinstall.js").exists(),
        "the package must be built and placed",
    );

    drop((root, mock_instance));
}

/// TS: `approve-builds updates GVS symlinks and runs builds at correct
/// hash directory` (`pnpm/test/install/globalVirtualStore.ts:34`).
///
/// The CLI-level counterpart of
/// [`gvs_approve_builds_scenario_moves_artifacts_to_a_new_hash_dir`]:
/// the same hash move, but driven by the real `approve-builds` command,
/// which also has to persist the approval into `pnpm-workspace.yaml`.
#[test]
fn approve_builds_updates_gvs_symlinks_and_runs_builds_at_the_new_hash_dir() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        &serde_json::json!({ "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" }),
    );
    set_gvs_workspace_yaml(&workspace, "strictDepBuilds: false\n");

    eprintln!("Install with the build unapproved...");
    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let version_dir =
        pkg_version_dir(&store_dir, "@pnpm.e2e/pre-and-postinstall-scripts-example", "1.0.0");
    let hash_before = sole_hash_dir(&version_dir)
        .file_name()
        .expect("hash dir name")
        .to_string_lossy()
        .into_owned();
    assert!(
        !workspace
            .join("node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js")
            .exists(),
        "the build must not have run before approval",
    );

    eprintln!("Running approve-builds --all...");
    pacquet(&workspace)
        .with_args(["approve-builds", "--all"])
        .assert()
        .success();

    let hash_after = hash_dirs(&version_dir)
        .into_iter()
        .find(|hash| hash != &hash_before)
        .expect("approve-builds must move the package to a new, engine-specific hash directory");
    let pkg = pkg_in_slot(
        &version_dir.join(&hash_after),
        "@pnpm.e2e/pre-and-postinstall-scripts-example",
    );
    assert!(pkg.join("generated-by-preinstall.js").exists());
    assert!(pkg.join("generated-by-postinstall.js").exists());

    eprintln!("The artifacts must be reachable through node_modules...");
    let linked = workspace.join("node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example");
    assert!(
        linked.join("generated-by-postinstall.js").exists(),
        "approve-builds must repoint the project link at the built slot",
    );

    eprintln!("The approval must be persisted into pnpm-workspace.yaml...");
    let yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml"))
        .expect("read pnpm-workspace.yaml");
    assert!(
        yaml.contains("@pnpm.e2e/pre-and-postinstall-scripts-example"),
        "approve-builds must record the approval in pnpm-workspace.yaml; got:\n{yaml}",
    );

    drop((root, mock_instance));
}

/// A dependency's build script in a shared slot sees the workspace root's
/// bins and the privately hoisted bins, as it does with a local virtual store.
/// This is by design, although the slot hash records neither: `NODE_PATH`
/// already exposes the root and hoisted `node_modules` to the same scripts,
/// and builds that run a tool they do not declare would fail without them.
#[test]
fn gvs_dependency_build_scripts_see_the_workspace_root_and_hoisted_bins() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    for (name, bin, marker) in [
        ("tool", "gvs-root-tool", "root-tool-ran"),
        ("hoisted-tool", "gvs-hoisted-tool", "hoisted-tool-ran"),
    ] {
        fs::create_dir_all(workspace.join(name)).expect("mkdir tool");
        fs::write(
            workspace.join(name).join("package.json"),
            serde_json::json!({ "name": name, "version": "1.0.0", "bin": { bin: "cli.js" } })
                .to_string(),
        )
        .expect("write tool package.json");
        fs::write(
            workspace.join(name).join("cli.js"),
            format!("#!/usr/bin/env node\nrequire('fs').writeFileSync('{marker}', '')\n"),
        )
        .expect("write tool cli.js");
    }

    fs::create_dir_all(workspace.join("wrapper")).expect("mkdir wrapper");
    fs::write(
        workspace.join("wrapper/package.json"),
        serde_json::json!({
            "name": "wrapper",
            "version": "1.0.0",
            "dependencies": { "hoisted-tool": "file:../hoisted-tool" },
        })
        .to_string(),
    )
    .expect("write wrapper/package.json");

    fs::create_dir_all(workspace.join("dep")).expect("mkdir dep");
    fs::write(
        workspace.join("dep/package.json"),
        serde_json::json!({
            "name": "dep",
            "version": "1.0.0",
            "scripts": { "postinstall": "gvs-root-tool && gvs-hoisted-tool" },
        })
        .to_string(),
    )
    .expect("write dep/package.json");

    write_manifest(
        &workspace,
        &serde_json::json!({ "tool": "file:tool", "wrapper": "file:wrapper", "dep": "file:dep" }),
    );
    set_gvs_workspace_yaml(&workspace, &allow_builds_yaml(&[("dep@file:dep", true)]));

    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let slot = sole_hash_dir(&pkg_version_dir(&store_dir, "@/dep", "directory"));
    let dep_dir = pkg_in_slot(&slot, "dep");
    assert!(dep_dir.join("root-tool-ran").exists(), "the workspace root's bin did not run");
    assert!(dep_dir.join("hoisted-tool-ran").exists(), "the hoisted bin did not run");

    drop((root, mock_instance));
}

/// A git dependency's build runs in every global-virtual-store slot of its
/// commit, not only in the first one. The override moves its dependency, so
/// the second install links the same commit into a new slot, which reads the
/// store-index row the first install wrote
/// ([pnpm/pnpm#17224](https://github.com/pnpm/pnpm/issues/17224)).
#[test]
fn gvs_builds_each_slot_of_a_git_dependency() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    let repo = GitRepoFixture::init(root.path(), "native-addon");
    repo.write_file(
        "package.json",
        &serde_json::json!({
            "name": "native-addon",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/dep-of-pkg-with-1-dep": "^100.0.0" },
            "scripts": { "install": r#"node -e "require('fs').writeFileSync('built', '')""# },
        })
        .to_string(),
    );
    let spec = repo.git_url_at(&repo.commit("init"));
    write_manifest(&workspace, &serde_json::json!({ "native-addon": spec }));
    let allow_builds = allow_builds_yaml(&[(&format!("native-addon@{spec}"), true)]);

    for dependency_version in ["100.0.0", "100.1.0"] {
        set_gvs_workspace_yaml(
            &workspace,
            &format!(
                "{allow_builds}overrides:\n  '@pnpm.e2e/dep-of-pkg-with-1-dep': {dependency_version}\n",
            ),
        );
        pacquet(&workspace)
            .with_arg("install")
            .assert()
            .success();
    }

    let version_dir = pkg_version_dir(&store_dir, "native-addon", "1.0.0");
    let slots = hash_dirs(&version_dir);
    assert_eq!(slots.len(), 2, "each dependency version must give the git dependency its own slot");
    for slot in slots {
        let pkg = pkg_in_slot(&version_dir.join(&slot), "native-addon");
        assert!(pkg.join("built").exists(), "the slot {slot} was not built");
        assert!(!pkg.join(".pnpm-needs-build").exists(), "the slot {slot} is still marked unbuilt");
    }

    drop((root, mock_instance));
}

/// A package is built in a staging directory, so its build scripts must find
/// the bins of its own dependencies there. With hoisting off, nothing else
/// puts `hello-world-js-bin`, which the scripts call, on their `PATH`.
#[test]
fn a_staged_build_runs_the_bins_of_its_dependencies() {
    const PKG: &str = "@pnpm.e2e/pre-and-postinstall-scripts-example";

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, &serde_json::json!({ PKG: "1.0.0" }));
    let workspace_settings = format!("{}hoist: false\n", allow_builds_yaml(&[(PKG, true)]));
    set_gvs_workspace_yaml(&workspace, &workspace_settings);
    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let pkg = pkg_in_slot(&sole_hash_dir(&pkg_version_dir(&store_dir, PKG, "1.0.0")), PKG);
    assert!(pkg.join("generated-by-postinstall.js").exists(), "the build must have run");
    assert!(
        pkg.join("node_modules/.bin/hello-world-js-bin").exists(),
        "the dependency's bin must be placed with the package",
    );

    drop((root, mock_instance));
}

/// <https://github.com/pnpm/pnpm/issues/15568>: installs that share a store
/// and build the same package at the same time all succeed, and every
/// project links the one built package, with no staging directory left
/// behind.
#[test]
fn concurrent_installs_build_one_shared_slot() {
    const PKG: &str = "@pnpm.e2e/pre-and-postinstall-scripts-example";

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, &serde_json::json!({ PKG: "1.0.0" }));
    set_gvs_workspace_yaml(&workspace, &allow_builds_yaml(&[(PKG, true)]));
    let projects: Vec<_> = std::iter::once(workspace.clone())
        .chain((1..4).map(|index| {
            let project = root
                .path()
                .join(format!("project-{index}"));
            fs::create_dir(&project).expect("create project");
            for entry in fs::read_dir(&workspace).expect("read workspace") {
                let entry = entry.expect("read workspace entry");
                if entry
                    .file_type()
                    .expect("stat workspace entry")
                    .is_file()
                {
                    fs::copy(entry.path(), project.join(entry.file_name())).expect("copy file");
                }
            }
            project
        }))
        .collect();

    let installs: Vec<_> = projects
        .iter()
        .map(|project| {
            pacquet(project)
                .with_arg("install")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect("spawn an install")
        })
        .collect();
    for install in installs {
        let output = install.wait_with_output().expect("wait for an install");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    }

    let slot = sole_hash_dir(&pkg_version_dir(&store_dir, PKG, "1.0.0"));
    for project in &projects {
        let linked =
            fs::canonicalize(project.join("node_modules").join(PKG)).expect("resolve the link");
        assert_eq!(linked, fs::canonicalize(pkg_in_slot(&slot, PKG)).expect("resolve the slot"));
        assert!(linked.join("generated-by-postinstall.js").exists(), "the package must be built");
    }
    let leftovers: Vec<_> = fs::read_dir(slot.join("node_modules/@pnpm.e2e"))
        .expect("read the slot's scope")
        .map(|entry| entry.expect("read entry").file_name())
        .filter(|name| name.to_string_lossy().starts_with('.'))
        .collect();
    assert!(leftovers.is_empty(), "staging directories left behind: {leftovers:?}");

    drop((root, mock_instance));
}

#[test]
fn a_failed_optional_build_leaves_no_project_link() {
    const PKG: &str = "@pnpm.e2e/failing-postinstall";

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest = serde_json::json!({ "optionalDependencies": { PKG: "1.0.0" } });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");
    set_gvs_workspace_yaml(&workspace, &allow_builds_yaml(&[(PKG, true)]));
    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();

    assert!(
        fs::symlink_metadata(workspace.join("node_modules").join(PKG)).is_err(),
        "the project must not link a package that never reached its slot",
    );

    drop((root, mock_instance));
}

#[test]
fn a_rebuilt_local_directory_package_runs_the_bins_of_its_dependencies() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let local = root.path().join("local");
    fs::create_dir(&local).expect("create the local package");
    let write_local = |version: &str| {
        let manifest = serde_json::json!({
            "name": "local",
            "version": version,
            "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" },
            "scripts": {
                "postinstall": r#"hello-world-js-bin && node -e "require('fs').writeFileSync('built.txt', '')""#,
            },
        });
        fs::write(local.join("package.json"), manifest.to_string()).expect("write local manifest");
    };
    write_local("1.0.0");
    write_manifest(&workspace, &serde_json::json!({ "local": "file:../local" }));
    set_gvs_workspace_yaml(&workspace, "dangerouslyAllowAllBuilds: true\nhoist: false\n");

    for install in ["first", "second"] {
        fs::write(local.join("data.txt"), install).expect("change the local package");
        pacquet(&workspace)
            .with_arg("install")
            .assert()
            .success();
        let linked = fs::canonicalize(workspace.join("node_modules/local")).expect("resolve link");
        assert!(linked.join("built.txt").exists(), "the {install} install must build the package");
        assert_eq!(fs::read_to_string(linked.join("data.txt")).expect("read data"), install);
    }

    drop((root, mock_instance));
}
