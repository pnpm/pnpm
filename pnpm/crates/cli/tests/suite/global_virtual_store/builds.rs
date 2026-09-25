//! The global-virtual-store tests that exercise the build policy.

use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, StoreDir, StoreIndex, allow_builds_yaml,
    corrupt_pristine_file, fs, hash_dirs, is_symlink_or_junction, pacquet, pkg_in_slot,
    pkg_version_dir, read_modules_manifest, set_gvs_workspace_yaml, sole_hash_dir, write_manifest,
};
use assert_cmd::assert::OutputAssertExt;
use pnpm_fs::DirLock;
use std::{
    process::{Child, Stdio},
    thread,
    time::Duration,
};

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

/// TS: `GVS re-links when allowBuilds changes` (`globalVirtualStore.ts:205`),
/// which is also the `.modules.yaml` half listed under the
/// "`.modules.yaml` Write And Verify" section of the porting plan: the
/// approval set the install ran under has to round-trip through
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
fn gvs_build_failure_keeps_the_slot_marked_for_a_rebuild() {
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
    let pkg = pkg_in_slot(&sole_hash_dir(&version_dir), "@pnpm.e2e/failing-postinstall");
    assert!(pkg.join("package.json").exists(), "the failed build's slot must stay in place");
    assert_eq!(
        fs::read_to_string(pkg.join(".pnpm-needs-build")).expect("read the marker"),
        "started",
        "the failed build's slot must be marked for a rebuild",
    );

    drop((root, mock_instance));
}

/// An optional dependency whose build fails is skipped, but its GVS slot
/// stays for the same reason as [`gvs_build_failure_keeps_the_slot_marked_for_a_rebuild`]:
/// other projects may link it.
#[test]
fn gvs_optional_build_failure_keeps_the_slot() {
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
        .with_arg("install")
        .assert()
        .success();

    let version_dir = pkg_version_dir(&store_dir, "@pnpm.e2e/failing-postinstall", "1.0.0");
    let pkg = pkg_in_slot(&sole_hash_dir(&version_dir), "@pnpm.e2e/failing-postinstall");
    assert!(
        pkg.join("package.json").exists(),
        "the failed optional build's slot must stay in place",
    );

    drop((root, mock_instance));
}

/// <https://github.com/pnpm/pnpm/issues/15568>: installs sharing a GVS slot
/// must not write into it while another install builds it.
#[test]
fn gvs_install_waits_for_another_install_building_the_same_slot() {
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

    let hash_dir = sole_hash_dir(&pkg_version_dir(&store_dir, PKG, "1.0.0"));
    let pkg = pkg_in_slot(&hash_dir, PKG);
    assert!(pkg.join(".pnpm-needs-build").is_file(), "the deferred build must leave its marker");
    // As a build that died mid-way leaves the slot, which only a re-import
    // makes safe to build again.
    fs::write(pkg.join(".pnpm-needs-build"), "started").expect("mark the slot mid-build");
    fs::remove_file(pkg.join("README.md")).expect("remove a pristine file");

    let lock = DirLock::acquire(
        hash_dir.join(".pnpm-build.lock"),
        Duration::ZERO,
        Duration::from_mins(30),
    )
    .expect("take the slot lock")
    .expect("the slot lock is free");
    let mut install = ReapOnDrop(
        pacquet(&workspace)
            .with_arg("install")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn the install"),
    );

    thread::sleep(Duration::from_secs(3));
    let exited = install.0.try_wait().expect("poll the install");
    eprintln!("install exit status while the slot lock was held: {exited:?}");
    assert!(exited.is_none(), "the install must wait for the slot lock");
    assert!(
        !pkg.join("generated-by-postinstall.js").exists(),
        "the install must not build the slot while another install holds its lock",
    );
    assert!(
        !pkg.join("README.md").exists(),
        "the install must not re-import the slot while another install holds its lock",
    );

    drop(lock);
    let status = install.0.wait().expect("wait for the install");
    assert!(status.success(), "the install must finish once the slot lock is released");
    assert!(pkg.join("generated-by-postinstall.js").exists());
    assert!(pkg.join("README.md").exists());
    assert!(!pkg.join(".pnpm-needs-build").exists());

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

/// TS: `GVS .pnpm-needs-build marker triggers re-import on next install`
/// (`globalVirtualStore.ts:411`).
#[test]
fn needs_build_marker_triggers_reimport_on_next_install() {
    for with_installability_constraint in [false, true] {
        let CommandTempCwd { root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

        let dependencies = if with_installability_constraint {
            let constraint = workspace.join("constraint");
            fs::create_dir(&constraint).expect("create constrained dependency");
            fs::write(
                constraint.join("package.json"),
                serde_json::json!({
                    "name": "constraint",
                    "version": "1.0.0",
                    "engines": { "node": ">=18" },
                })
                .to_string(),
            )
            .expect("write constrained dependency manifest");
            serde_json::json!({
                "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0",
                "constraint": "file:./constraint",
            })
        } else {
            serde_json::json!({
                "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0",
            })
        };
        write_manifest(&workspace, &dependencies);
        let workspace_settings = format!(
            "{}nodeVersion: 20.0.0\nsideEffectsCache: false\n",
            allow_builds_yaml(&[("@pnpm.e2e/pre-and-postinstall-scripts-example", true)]),
        );
        set_gvs_workspace_yaml(&workspace, &workspace_settings);

        eprintln!("First install, with the build approved...");
        pacquet(&workspace)
            .with_arg("install")
            .assert()
            .success();

        let version_dir =
            pkg_version_dir(&store_dir, "@pnpm.e2e/pre-and-postinstall-scripts-example", "1.0.0");
        let pkg = pkg_in_slot(
            &sole_hash_dir(&version_dir),
            "@pnpm.e2e/pre-and-postinstall-scripts-example",
        );
        let marker = pkg.join(".pnpm-needs-build");
        let postinstall_artifact = pkg.join("generated-by-postinstall.js");
        let package_manifest = pkg.join("package.json");
        let pristine_manifest =
            fs::read_to_string(&package_manifest).expect("read package manifest");
        assert!(postinstall_artifact.exists());
        assert!(!marker.exists());

        eprintln!("Simulating a crash after import but before the build...");
        fs::write(&marker, "").expect("write the incomplete-build marker");
        fs::remove_file(&postinstall_artifact).expect("remove the build artifact");
        corrupt_pristine_file(&package_manifest);

        eprintln!("Frozen reinstall with intact project links must rebuild the marked slot...");
        pacquet(&workspace)
            .with_args(["install", "--frozen-lockfile"])
            .assert()
            .success();

        assert!(!marker.exists(), "the successful retry must remove the marker");
        assert!(postinstall_artifact.exists(), "the successful retry must recreate build output");
        assert_eq!(
            fs::read_to_string(&package_manifest).expect("read the restored manifest"),
            pristine_manifest,
            "the retry must re-import pristine package files before rebuilding",
        );

        eprintln!("Marking the slot again to exercise the optimistic repeat-install paths...");
        fs::write(&marker, "").expect("write the second incomplete-build marker");
        fs::remove_file(&postinstall_artifact).expect("remove the rebuilt artifact");
        fs::write(&package_manifest, "{}").expect("corrupt the pristine file again");
        pacquet(&workspace)
            .with_arg("install")
            .assert()
            .success();

        assert!(!marker.exists(), "the ordinary reinstall must remove the marker");
        assert!(
            postinstall_artifact.exists(),
            "the ordinary reinstall must not report up to date before rebuilding",
        );
        assert_eq!(
            fs::read_to_string(&package_manifest).expect("read the twice-restored manifest"),
            pristine_manifest,
            "the ordinary reinstall must re-import the package before rebuilding",
        );

        drop((root, mock_instance));
    }
}

#[test]
fn orphan_needs_build_marker_does_not_invalidate_repeat_install() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0",
            },
            "scripts": {
                "postinstall": r#"node -e "require('fs').appendFileSync('root-postinstall.txt', 'run\n')""#,
            },
        })
        .to_string(),
    )
    .expect("write package.json");
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
    let orphan_pkg = pkg_in_slot(
        &version_dir.join("0000000000000000000000000000000000000000000000000000000000000000"),
        "@pnpm.e2e/pre-and-postinstall-scripts-example",
    );
    fs::create_dir_all(&orphan_pkg).expect("create orphan GVS slot");
    fs::write(orphan_pkg.join(".pnpm-needs-build"), "").expect("write orphan build marker");

    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(workspace.join("root-postinstall.txt")).expect("read script output"),
        "run\n",
        "an unrelated GVS slot must not cause the root lifecycle scripts to run again",
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

/// A slot's hash does not record the workspace root's bins, so a build
/// that another project reuses must not depend on them. Only the pinned
/// runtime's `node` reaches the script, which the engine part of the hash
/// does record.
#[test]
fn gvs_dependency_build_scripts_do_not_see_the_workspace_root_bins() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    fs::create_dir_all(workspace.join("tool")).expect("mkdir tool");
    fs::write(
        workspace.join("tool/package.json"),
        serde_json::json!({
            "name": "tool",
            "version": "1.0.0",
            "bin": { "gvs-root-tool": "cli.js" },
        })
        .to_string(),
    )
    .expect("write tool/package.json");
    fs::write(
        workspace.join("tool/cli.js"),
        "#!/usr/bin/env node\nrequire('fs').writeFileSync('root-tool-ran', '')\n",
    )
    .expect("write tool/cli.js");

    fs::create_dir_all(workspace.join("dep")).expect("mkdir dep");
    fs::write(
        workspace.join("dep/package.json"),
        serde_json::json!({
            "name": "dep",
            "version": "1.0.0",
            "scripts": {
                "postinstall": r#"gvs-root-tool || node -e "require('fs').writeFileSync('root-tool-missing', '')""#,
            },
        })
        .to_string(),
    )
    .expect("write dep/package.json");

    write_manifest(&workspace, &serde_json::json!({ "tool": "file:tool", "dep": "file:dep" }));
    set_gvs_workspace_yaml(&workspace, &allow_builds_yaml(&[("dep@file:dep", true)]));

    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let slot = sole_hash_dir(&pkg_version_dir(&store_dir, "@/dep", "directory"));
    let dep_dir = pkg_in_slot(&slot, "dep");
    assert!(dep_dir.join("root-tool-missing").exists(), "the postinstall script did not run");
    assert!(
        !dep_dir.join("root-tool-ran").exists(),
        "the postinstall script ran a bin from the workspace root's node_modules/.bin",
    );

    drop((root, mock_instance));
}

/// Kills and reaps a spawned install when a failed assertion unwinds past it,
/// so it cannot keep writing into the store while later tests run.
struct ReapOnDrop(Child);

impl Drop for ReapOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
