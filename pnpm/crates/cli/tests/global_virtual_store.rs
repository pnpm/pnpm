//! Ports of the upstream global-virtual-store suites — the primary
//! `installing/deps-installer/test/install/globalVirtualStore.ts` and the
//! CLI-level `pnpm/test/install/globalVirtualStore.ts`.
//!
//! Upstream drives these through the programmatic `install()` API and can
//! monkey-patch `storeController.fetchPackage` to count fetches. Pacquet's
//! equivalent surface is the CLI, so the ports assert the same on-disk
//! contract — slot layout, hash-directory identity across `allowBuilds`
//! changes, build artifacts, and `.modules.yaml` state — instead of the
//! call counts. Where that loses a signal it is called out on the test.
//!
//! `known_failures` at the bottom holds the one case whose subject under
//! test pacquet has not built yet.
#![cfg(unix)] // the GVS slot assertions read symlinks

pub mod _utils;
pub use _utils::*;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_store_dir::STORE_VERSION;
use pacquet_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::is_symlink_or_junction,
};
use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// `<store_dir>/v11/links` — the root every GVS slot hangs off.
fn gvs_root(store_dir: &Path) -> PathBuf {
    store_dir.join(STORE_VERSION).join("links")
}

/// The `<gvs>/<scope>/<name>/<version>` directory whose children are the
/// per-dependency-graph hash directories.
fn pkg_version_dir(store_dir: &Path, name: &str, version: &str) -> PathBuf {
    gvs_root(store_dir).join(name).join(version)
}

/// Sorted hash-directory names under a `<name>/<version>` directory.
///
/// Upstream reads these with `fs.readdirSync` and asserts on the count and
/// on identity across installs; sorting keeps the comparison stable.
fn hash_dirs(pkg_version_dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(pkg_version_dir)
        .unwrap_or_else(|err| panic!("read hash dirs under {pkg_version_dir:?}: {err}"))
        .map(|entry| entry.expect("read hash dir entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The single hash directory under `<name>/<version>`, asserting there is
/// exactly one — upstream's `expect(files).toHaveLength(1)`.
fn sole_hash_dir(pkg_version_dir: &Path) -> PathBuf {
    let hashes = hash_dirs(pkg_version_dir);
    assert_eq!(
        hashes.len(),
        1,
        "expected exactly one hash directory under {pkg_version_dir:?}, got {hashes:?}",
    );
    pkg_version_dir.join(&hashes[0])
}

/// `<hash>/node_modules/<name>` — where the package's files actually live.
fn pkg_in_slot(hash_dir: &Path, name: &str) -> PathBuf {
    hash_dir.join("node_modules").join(name)
}

/// Rewrite `pnpm-workspace.yaml` as the harness's `storeDir` / `cacheDir`
/// plus `enableGlobalVirtualStore: true` and `extra_yaml`.
///
/// [`enable_gvs_in_workspace_yaml`] asserts it is flipping the harness's
/// `enableGlobalVirtualStore: false` line, so it can only be called once.
/// These tests re-set `allowBuilds` between installs, so they need a form
/// that is idempotent.
fn set_gvs_workspace_yaml(workspace: &Path, extra_yaml: &str) {
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let existing = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    let mut yaml: String = existing
        .lines()
        .filter(|line| line.starts_with("storeDir:") || line.starts_with("cacheDir:"))
        .fold(String::new(), |mut acc, line| {
            acc.push_str(line);
            acc.push('\n');
            acc
        });
    assert!(
        !yaml.is_empty(),
        "expected the `storeDir` / `cacheDir` keys written by \
         `CommandTempCwd::add_mocked_registry` — has the helper changed?",
    );
    yaml.push_str("enableGlobalVirtualStore: true\n");
    yaml.push_str(extra_yaml);
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
}

fn write_manifest(workspace: &Path, deps: &serde_json::Value) {
    let manifest = serde_json::json!({ "dependencies": deps });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");
}

/// A fresh `Command` for the pacquet binary — `assert_cmd`'s `Command` is
/// single-use, so every sequential install needs its own.
fn pacquet(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

fn read_modules_manifest(workspace: &Path) -> pacquet_modules_yaml::Modules {
    pacquet_modules_yaml::read_modules_manifest::<pacquet_modules_yaml::Host>(
        &workspace.join("node_modules"),
    )
    .expect("read .modules.yaml")
    .expect(".modules.yaml must exist after an install")
}

/// Render an `allowBuilds:` block for [`set_gvs_workspace_yaml`]'s
/// `extra_yaml`. An empty slice yields `allowBuilds: {}` — upstream's
/// `allowBuilds: {}`, which is materially different from omitting the key
/// because it pins "nothing may build" rather than "no opinion".
fn allow_builds_yaml(entries: &[(&str, bool)]) -> String {
    if entries.is_empty() {
        return "allowBuilds: {}\n".to_string();
    }
    let mut yaml = String::from("allowBuilds:\n");
    for (spec, value) in entries {
        writeln!(yaml, "  '{spec}': {value}").expect("format an allowBuilds entry");
    }
    yaml
}

/// TS: `using a global virtual store` (`globalVirtualStore.ts:21`), which
/// is also the CLI-level `pnpm/test/install/globalVirtualStore.ts:11`.
/// Both halves of the upstream test: a fresh install populates the GVS and
/// the private hoist, then wiping `node_modules` *and* the GVS and
/// reinstalling frozen rebuilds the identical layout.
#[test]
fn using_a_global_virtual_store() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    set_gvs_workspace_yaml(&workspace, "privateHoistPattern:\n  - '*'\n");
    write_manifest(&workspace, &serde_json::json!({ "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }));

    let assert_layout = |phase: &str| {
        assert!(
            workspace
                .join(
                    "node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json"
                )
                .exists(),
            "{phase}: the transitive dep must be privately hoisted",
        );
        assert!(
            workspace.join("node_modules/.pnpm/lock.yaml").exists(),
            "{phase}: the current lockfile must be written",
        );
        let version_dir = pkg_version_dir(&store_dir, "@pnpm.e2e/pkg-with-1-dep", "100.0.0");
        let hash_dir = sole_hash_dir(&version_dir);
        assert!(
            pkg_in_slot(&hash_dir, "@pnpm.e2e/pkg-with-1-dep").join("package.json").exists(),
            "{phase}: the package must be materialized in its GVS slot",
        );
        assert!(
            pkg_in_slot(&hash_dir, "@pnpm.e2e/dep-of-pkg-with-1-dep").join("package.json").exists(),
            "{phase}: the slot's own node_modules must carry the transitive dep",
        );
    };

    eprintln!("Fresh install with GVS enabled...");
    pacquet(&workspace).with_arg("install").assert().success();
    assert_layout("fresh install");

    eprintln!("Wiping node_modules and the whole GVS, then reinstalling frozen...");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    fs::remove_dir_all(gvs_root(&store_dir)).expect("remove the GVS root");
    pacquet(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();
    assert_layout("frozen reinstall from a cold GVS");

    drop((root, mock_instance));
}

/// TS: `reinstall from warm global virtual store after deleting
/// node_modules` (`globalVirtualStore.ts:63`) and the CLI-level `warm GVS
/// reinstall skips internal linking` (`pnpm/test/install/globalVirtualStore.ts:80`).
///
/// Upstream additionally wraps `storeController.fetchPackage` to assert it
/// is never called. Pacquet's CLI exposes no such seam, so the port
/// asserts the observable consequence instead: the warm slot is reused
/// rather than rebuilt beside a second hash directory, and the project
/// tree — including `.bin` — is fully restored from it.
#[test]
fn reinstall_from_warm_global_virtual_store_after_deleting_node_modules() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    set_gvs_workspace_yaml(&workspace, "privateHoistPattern:\n  - '*'\n");
    write_manifest(
        &workspace,
        &serde_json::json!({
            "@pnpm.e2e/hello-world-js-bin": "1.0.0",
            "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
        }),
    );

    eprintln!("First install — warms the GVS...");
    pacquet(&workspace).with_arg("install").assert().success();

    let version_dir = pkg_version_dir(&store_dir, "@pnpm.e2e/pkg-with-1-dep", "100.0.0");
    let hashes_before = hash_dirs(&version_dir);

    eprintln!("Deleting node_modules only — the GVS stays warm...");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    assert!(gvs_root(&store_dir).is_dir(), "the GVS must survive the node_modules wipe");

    eprintln!("Frozen reinstall — must reattach from the warm GVS...");
    pacquet(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

    assert_eq!(
        hash_dirs(&version_dir),
        hashes_before,
        "a warm reinstall must reuse the existing slot, not materialize a second one",
    );
    assert!(
        workspace.join("node_modules/@pnpm.e2e/pkg-with-1-dep/package.json").exists(),
        "the direct dep must be relinked into the project",
    );
    assert!(
        workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin/package.json").exists(),
        "every direct dep must be relinked, not just the first",
    );
    assert!(
        workspace
            .join("node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep")
            .try_exists()
            .expect("stat the hoisted dep"),
        "the private hoist must be rebuilt from the warm GVS",
    );
    assert!(
        workspace.join("node_modules/.bin/hello-world-js-bin").exists(),
        "bins must be relinked after a warm reinstall",
    );
    assert!(
        workspace.join("node_modules/.pnpm/lock.yaml").exists(),
        "the current lockfile must be rewritten",
    );

    drop((root, mock_instance));
}

/// TS: `modules are correctly updated when using a global virtual store`
/// (`globalVirtualStore.ts:107`). Bumping one dependency's version must
/// materialize a slot for the new version.
#[test]
fn modules_are_correctly_updated_when_using_a_global_virtual_store() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    set_gvs_workspace_yaml(&workspace, "");
    write_manifest(
        &workspace,
        &serde_json::json!({
            "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
            "@pnpm.e2e/peer-c": "1.0.0",
        }),
    );

    eprintln!("Installing with peer-c 1.0.0...");
    pacquet(&workspace).with_arg("install").assert().success();

    eprintln!("Bumping peer-c to 2.0.0 and reinstalling...");
    write_manifest(
        &workspace,
        &serde_json::json!({
            "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
            "@pnpm.e2e/peer-c": "2.0.0",
        }),
    );
    pacquet(&workspace).with_arg("install").assert().success();

    assert!(
        workspace.join("node_modules/.pnpm/lock.yaml").exists(),
        "the current lockfile must be rewritten",
    );
    let version_dir = pkg_version_dir(&store_dir, "@pnpm.e2e/peer-c", "2.0.0");
    let hash_dir = sole_hash_dir(&version_dir);
    assert!(
        pkg_in_slot(&hash_dir, "@pnpm.e2e/peer-c").join("package.json").exists(),
        "the newly-resolved version must be materialized in its own GVS slot",
    );

    drop((root, mock_instance));
}

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
    pacquet(&workspace).with_arg("install").assert().success();
    let version_dir = pkg_version_dir(&store_dir, "@pnpm.e2e/pkg-with-1-dep", "100.0.0");
    let hash_no_builds = sole_hash_dir(&version_dir);

    eprintln!("Scenario 2: the transitive dep may build — the parent hash must change...");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    set_gvs_workspace_yaml(
        &workspace,
        &allow_builds_yaml(&[("@pnpm.e2e/dep-of-pkg-with-1-dep", true)]),
    );
    pacquet(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

    let hashes_after = hash_dirs(&version_dir);
    let hash_no_builds_name =
        hash_no_builds.file_name().expect("hash dir name").to_string_lossy().into_owned();
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
    pacquet(&workspace).with_arg("install").assert().success();
    let version_dir = pkg_version_dir(&store_dir, "@pnpm.e2e/pkg-with-1-dep", "100.0.0");
    let hashes_before = hash_dirs(&version_dir);

    eprintln!("Scenario 2: an unrelated package may build...");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    set_gvs_workspace_yaml(&workspace, &allow_builds_yaml(&[("some-unrelated-package", true)]));
    pacquet(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

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
    pacquet(&workspace).with_arg("install").assert().success();

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
    pacquet(&workspace).with_arg("install").assert().success();

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
        pacquet_modules_yaml::AllowBuildValue::Bool(true),
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

    pacquet(&workspace).with_arg("install").assert().success();

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
    pacquet(&workspace).with_arg("install").assert().success();

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
    pacquet(&workspace).with_arg("install").assert().success();

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

/// TS: `GVS build failure cleans up broken package directory`
/// (`globalVirtualStore.ts:338`).
///
/// A GVS hash directory is shared by every project whose dependency graph
/// hashes to it, so a half-built one must not survive a failed build — the
/// next install would take the warm path into broken files.
#[test]
fn gvs_build_failure_cleans_up_broken_package_directory() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, &serde_json::json!({ "@pnpm.e2e/failing-postinstall": "1.0.0" }));
    set_gvs_workspace_yaml(
        &workspace,
        &allow_builds_yaml(&[("@pnpm.e2e/failing-postinstall", true)]),
    );

    eprintln!("Installing a package whose postinstall exits non-zero...");
    pacquet(&workspace).with_arg("install").assert().failure();

    let version_dir = pkg_version_dir(&store_dir, "@pnpm.e2e/failing-postinstall", "1.0.0");
    if version_dir.exists() {
        for hash in hash_dirs(&version_dir) {
            let pkg = pkg_in_slot(&version_dir.join(&hash), "@pnpm.e2e/failing-postinstall");
            assert!(
                !pkg.exists(),
                "the failed build's slot must be removed so the next install re-fetches; \
                 {pkg:?} survived",
            );
        }
    }

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
    pacquet(&workspace).with_arg("install").assert().success();

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
    pacquet(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

    let rebuilt = sole_hash_dir(&version_dir);
    assert!(
        pkg_in_slot(&rebuilt, "@pnpm.e2e/pre-and-postinstall-scripts-example")
            .join("generated-by-postinstall.js")
            .exists(),
        "the rebuilt slot must carry the build artifacts again",
    );

    drop((root, mock_instance));
}

/// TS: `injected local packages work with global virtual store`
/// (`globalVirtualStore.ts:461`).
///
/// The materialization half is already pinned by
/// `injected_workspace_dep_with_dedupe_off_materialises_under_gvs` in
/// `dedupe_injected_deps.rs`; this port covers the half that one does not
/// — `.modules.yaml.injectedDeps` pointing into the GVS.
#[test]
fn injected_local_packages_work_with_global_virtual_store() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "ws-root", "version": "0.0.0", "private": true }).to_string(),
    )
    .expect("write root package.json");
    set_gvs_workspace_yaml(&workspace, "packages:\n  - 'project-*'\ndedupeInjectedDeps: false\n");

    fs::create_dir_all(workspace.join("project-1")).expect("mkdir project-1");
    fs::write(
        workspace.join("project-1/package.json"),
        serde_json::json!({
            "name": "project-1",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/dep-of-pkg-with-1-dep": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write project-1/package.json");
    fs::write(workspace.join("project-1/foo.js"), "").expect("write project-1/foo.js");

    fs::create_dir_all(workspace.join("project-2")).expect("mkdir project-2");
    fs::write(
        workspace.join("project-2/package.json"),
        serde_json::json!({
            "name": "project-2",
            "version": "1.0.0",
            "dependencies": { "project-1": "workspace:1.0.0" },
            "dependenciesMeta": { "project-1": { "injected": true } },
        })
        .to_string(),
    )
    .expect("write project-2/package.json");

    pacquet(&workspace).with_arg("install").assert().success();

    assert!(
        workspace.join("project-2/node_modules/project-1").exists(),
        "project-2 must have the injected workspace package installed",
    );

    let injected_deps = read_modules_manifest(&workspace)
        .injected_deps
        .expect(".modules.yaml must record injectedDeps under GVS");
    let locations =
        injected_deps.get("project-1").expect("injectedDeps must have an entry for project-1");
    assert!(!locations.is_empty(), "the injectedDeps entry must list at least one location");

    let gvs_root = gvs_root(&store_dir);
    let location = Path::new(&locations[0]);
    // `.modules.yaml` stores injected-dep locations relative to the
    // project root, matching pnpm — upstream's assertion joins the
    // recorded value straight onto the test's cwd.
    let resolved =
        if location.is_absolute() { location.to_path_buf() } else { workspace.join(location) };
    let resolved = dunce::canonicalize(&resolved)
        .unwrap_or_else(|err| panic!("canonicalize injected dep location {resolved:?}: {err}"));
    let gvs_root = dunce::canonicalize(&gvs_root).expect("canonicalize the GVS root");
    assert!(
        resolved.starts_with(&gvs_root),
        "the injected dep must be materialized inside the GVS ({gvs_root:?}), got {resolved:?}",
    );
    assert!(
        resolved.join("foo.js").exists(),
        "the injected copy must carry the source project's files",
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

/// The three negatives every `virtualStoreOnly` install shares: no
/// importer symlinks, no hoisted packages, no `.bin`.
fn assert_no_post_import_linking(workspace: &Path) {
    assert!(
        !workspace.join("node_modules/@pnpm.e2e/pkg-with-1-dep").exists(),
        "importer-level symlinks must not be created",
    );
    assert!(
        !workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep").exists(),
        "nothing must be hoisted",
    );
    assert!(!workspace.join("node_modules/.bin").exists(), "no bins must be linked");
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
    pacquet(&workspace).with_arg("install").assert().success();

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
    pacquet(&workspace).with_args(["approve-builds", "--all"]).assert().success();

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

mod known_failures {
    //! Global-virtual-store cases whose subject under test pacquet has
    //! not built yet. Each stubs the boundary through
    //! [`pacquet_testing_utils::allow_known_failure`] so the test exits
    //! early rather than masking a real bug.

    use pacquet_testing_utils::{
        allow_known_failure,
        known_failure::{KnownFailure, KnownResult},
    };

    fn needs_build_marker() -> KnownResult<()> {
        Err(KnownFailure::new(
            "Pacquet does not implement the `.pnpm-needs-build` marker. \
             pnpm writes the file into a GVS slot between import and \
             build, removes it on success, and treats its presence on a \
             later install as \"this slot is half-built, re-fetch and \
             re-build\". Pacquet's import pipeline never writes it, the \
             warm-slot fast path never looks for it, and the \
             side-effects upload does not exclude it — so an interrupted \
             build leaves a slot the next install trusts. Porting it \
             means all three: the write site, the detect sites, and the \
             upload exclusion.",
        ))
    }

    /// TS: `GVS .pnpm-needs-build marker triggers re-import on next
    /// install` (`globalVirtualStore.ts:411`).
    #[test]
    fn needs_build_marker_triggers_reimport_on_next_install() {
        allow_known_failure!(needs_build_marker());
    }

    /// The tail of TS `GVS successful build creates package directory
    /// with build artifacts` (`globalVirtualStore.ts:250`): the marker
    /// must never reach the side-effects cache diff, or every cache hit
    /// would re-materialize a file that forces a rebuild forever.
    #[test]
    fn needs_build_marker_is_not_uploaded_to_the_side_effects_cache() {
        allow_known_failure!(needs_build_marker());
    }
}
