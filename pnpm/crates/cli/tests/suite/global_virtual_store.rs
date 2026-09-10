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
#![cfg(unix)] // the GVS slot assertions read symlinks

pub use _utils::*;

use crate::_utils;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_store_dir::{STORE_VERSION, StoreDir, StoreIndex};
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::is_symlink_or_junction,
};
use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
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

/// Replace a file the importer materialized with garbage, standing in
/// for a slot left half-written by a crashed install.
///
/// Unlinking before writing matters: when the store and the slot sit on
/// one filesystem the importer hardlinks, so truncating the slot's copy
/// in place would rewrite the store's content-addressed file too — and
/// then no re-import could ever restore the original bytes.
fn corrupt_pristine_file(path: &Path) {
    fs::remove_file(path).unwrap_or_else(|err| panic!("unlink {path:?}: {err}"));
    fs::write(path, "{}").unwrap_or_else(|err| panic!("corrupt {path:?}: {err}"));
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
    let mut yaml = harness_store_and_cache_yaml(workspace);
    yaml.push_str("enableGlobalVirtualStore: true\n");
    yaml.push_str(extra_yaml);
    fs::write(workspace.join("pnpm-workspace.yaml"), yaml).expect("write pnpm-workspace.yaml");
}

/// The harness's `storeDir` / `cacheDir` lines on their own, for a test
/// that writes its own virtual-store setting on top.
fn harness_store_and_cache_yaml(workspace: &Path) -> String {
    let existing = fs::read_to_string(workspace.join("pnpm-workspace.yaml"))
        .expect("read pnpm-workspace.yaml");
    let yaml: String = existing
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
    yaml
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

fn read_modules_manifest(workspace: &Path) -> pnpm_modules_yaml::Modules {
    pnpm_modules_yaml::read_modules_manifest::<pnpm_modules_yaml::Host>(
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
    let output = pacquet(&workspace)
        .with_args(["install", "--frozen-lockfile", "--reporter=ndjson"])
        .output()
        .expect("run the frozen reinstall");
    assert_success(&output);
    let added: Vec<u64> = ndjson_records(&output)
        .iter()
        .filter(|record| record["name"] == "pnpm:stats")
        .filter_map(|record| record["added"].as_u64())
        .collect();
    assert_eq!(
        added,
        vec![0],
        "a wiped node_modules loses the current lockfile, but the content-addressed \
         GVS slots are still authoritative, so nothing may be re-materialized",
    );

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
