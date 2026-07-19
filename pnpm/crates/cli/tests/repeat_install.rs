//! End-to-end coverage for installs over an **existing** `node_modules`
//! — the repair, reuse, and divergence scenarios of upstream's
//! `deps-restorer` and `deps-installer` suites. Every test installs
//! once, damages or drifts some part of the on-disk state, installs
//! again, and asserts the second install converges without rebuilding
//! what was still valid.

#![cfg(unix)] // pnpm CLI: 'program not found' on Windows runners.

pub mod _utils;
pub use _utils::*;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, os::unix::fs::MetadataExt, path::Path, process::Command};

/// Fresh `pnpm` invocation anchored in `workspace`, for tests that run
/// the binary more than once (the `CommandTempCwd` command is consumed
/// by its first use).
fn pacquet_in(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

fn write_workspace_yaml(workspace: &Path, extra: &str) {
    let yaml = format!(
        "storeDir: ../pacquet-store\ncacheDir: ../pacquet-cache\nenableGlobalVirtualStore: false\n{extra}"
    );
    fs::write(workspace.join("pnpm-workspace.yaml"), yaml).expect("write pnpm-workspace.yaml");
}

/// `version` field of the `package.json` under `workspace/relative`.
fn version_of(workspace: &Path, relative: &str) -> String {
    let text = fs::read_to_string(workspace.join(relative).join("package.json"))
        .unwrap_or_else(|error| panic!("read {relative}/package.json: {error}"));
    let manifest: serde_json::Value = serde_json::from_str(&text).expect("parse package.json");
    manifest["version"].as_str().expect("version is a string").to_string()
}

/// TS: `reinstalls missing packages to node_modules during headless
/// install` (`deps-installer misc.ts`): deleting a package's link and
/// its virtual-store copy makes the next frozen install emit
/// `pnpm:_broken_node_modules` for the missing dir and re-materialize
/// it.
#[test]
fn reinstalls_missing_packages_during_headless_install() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let first =
        pacquet.with_args(["add", "is-positive@1.0.0", "--reporter=ndjson"]).assert().success();
    let first_events = String::from_utf8_lossy(&first.get_output().stderr).into_owned();
    assert!(
        !first_events.contains("pnpm:_broken_node_modules"),
        "a clean install must not report broken modules",
    );

    let dep_location =
        workspace.join("node_modules/.pnpm/is-positive@1.0.0/node_modules/is-positive");
    fs::remove_dir_all(&dep_location).expect("remove the virtual-store copy");
    fs::remove_file(workspace.join("node_modules/is-positive"))
        .expect("remove the direct-dep symlink");

    let second = pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile", "--reporter=ndjson"])
        .assert()
        .success();
    let second_events = String::from_utf8_lossy(&second.get_output().stderr).into_owned();
    assert!(
        second_events.contains("pnpm:_broken_node_modules"),
        "the missing dir must be reported: {second_events}",
    );
    assert!(
        second_events.contains(dep_location.to_str().expect("utf-8 path")),
        "the event must carry the missing path",
    );
    assert_eq!(version_of(&workspace, "node_modules/is-positive"), "1.0.0");

    drop((root, mock_instance));
}

/// TS: `repeat install with no inner lockfile should not rewrite
/// packages in node_modules` (`deps-installer lockfile.ts:547`).
#[test]
fn repeat_install_with_no_inner_lockfile_keeps_packages_usable() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet.with_args(["add", "is-negative@1.0.0"]).assert().success();
    fs::remove_file(workspace.join("node_modules/.pnpm/lock.yaml"))
        .expect("remove the inner lockfile");

    pacquet_in(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();
    assert_eq!(version_of(&workspace, "node_modules/is-negative"), "1.0.0");

    drop((root, mock_instance));
}

/// TS: `subdeps are updated on repeat install if outer pnpm-lock.yaml
/// does not match the inner one` (`deps-installer lockfile.ts:368`).
/// The outer/inner divergence is produced by pinning the subdep's
/// version through a direct dependency, bumping the pin, and
/// regenerating only the outer lockfile.
#[test]
fn subdeps_updated_when_outer_lockfile_diverges_from_inner() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest = |pin: &str| {
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
                "@pnpm.e2e/dep-of-pkg-with-1-dep": pin,
            },
        })
        .to_string()
    };
    fs::write(workspace.join("package.json"), manifest("100.0.0")).expect("write package.json");
    pacquet.with_arg("install").assert().success();
    let subdep_in_parent_slot = workspace.join(
        "node_modules/.pnpm/@pnpm.e2e+pkg-with-1-dep@100.0.0/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep",
    );
    assert_eq!(version_of(&workspace, subdep_in_parent_slot.to_str().expect("utf-8")), "100.0.0");

    // Bump the pin and regenerate only the outer lockfile: the inner
    // one (and node_modules) still holds 100.0.0 while the outer now
    // records 100.1.0 for both the direct dep and the subdep edge.
    fs::write(workspace.join("package.json"), manifest("100.1.0")).expect("bump the pin");
    pacquet_in(&workspace).with_args(["install", "--lockfile-only"]).assert().success();

    pacquet_in(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();
    assert_eq!(
        version_of(&workspace, subdep_in_parent_slot.to_str().expect("utf-8")),
        "100.1.0",
        "the diverged subdep must be updated to match the outer lockfile",
    );

    drop((root, mock_instance));
}

/// TS: `installing non-prod deps then all deps`
/// (`deps-restorer index.ts:237`): a dev-only headless install leaves
/// prod deps out of `node_modules` and the current lockfile; the
/// follow-up full install adds them without disturbing the dev deps.
/// `once` is both a prod dep and a subdep of the dev dep `inflight`,
/// so it must not surface at the root until the prod group installs.
#[test]
fn installing_non_prod_deps_then_all_deps() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "is-positive": "1.0.0", "once": "^1.4.0" },
            "devDependencies": { "inflight": "1.0.6" },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet.with_args(["install", "--lockfile-only"]).assert().success();
    pacquet_in(&workspace).with_args(["install", "--frozen-lockfile", "--dev"]).assert().success();

    assert!(workspace.join("node_modules/inflight").exists());
    assert!(
        !workspace.join("node_modules/once").exists(),
        "the prod dep must not surface at the root of a dev-only install",
    );
    let current = read_current_lockfile(&workspace);
    let has_is_positive = current
        .packages
        .as_ref()
        .is_some_and(|packages| packages.keys().any(|key| key.to_string() == "is-positive@1.0.0"));
    assert!(!has_is_positive, "the excluded prod dep must not enter the current lockfile");

    pacquet_in(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();
    assert!(workspace.join("node_modules/once").exists());
    assert!(workspace.join("node_modules/inflight").exists());
    let current = read_current_lockfile(&workspace);
    let has_is_positive = current
        .packages
        .as_ref()
        .is_some_and(|packages| packages.keys().any(|key| key.to_string() == "is-positive@1.0.0"));
    assert!(has_is_positive, "the full install must record the prod dep in the current lockfile");

    drop((root, mock_instance));
}

/// TS: `available packages are used when node_modules is not clean`
/// (`deps-restorer index.ts:432`): with the store wiped, a frozen
/// install over a dirty `node_modules` must reuse the packages already
/// on disk (their files never re-enter the store) and fetch only the
/// newly wanted ones.
#[test]
fn available_packages_used_when_node_modules_not_clean() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "@pnpm.e2e/foobarqar": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");
    pacquet.with_arg("install").assert().success();

    let foobarqar_manifest = workspace
        .join("node_modules/.pnpm/@pnpm.e2e+foobarqar@1.0.0/node_modules/@pnpm.e2e/foobarqar/package.json");
    let inode_before = fs::metadata(&foobarqar_manifest).expect("stat foobarqar").ino();

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/foobarqar": "1.0.0",
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
            },
        })
        .to_string(),
    )
    .expect("extend package.json");
    pacquet_in(&workspace).with_args(["install", "--lockfile-only"]).assert().success();

    // Wipe the store: the still-valid packages must be served from the
    // dirty `node_modules`, not refetched.
    let store_dir = workspace.parent().expect("workspace has a parent").join("pacquet-store");
    fs::remove_dir_all(&store_dir).expect("wipe the store");

    pacquet_in(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

    assert!(workspace.join("node_modules/@pnpm.e2e/pkg-with-1-dep").exists());
    assert_eq!(
        fs::metadata(&foobarqar_manifest).expect("stat foobarqar").ino(),
        inode_before,
        "the already-materialized package must be reused, not re-imported",
    );
    let refetched: Vec<String> = index_file_contents(&store_dir)
        .keys()
        .filter(|key| key.contains("foobarqar"))
        .cloned()
        .collect();
    assert!(
        refetched.is_empty(),
        "the reused package must not re-enter the wiped store: {refetched:?}",
    );

    drop((root, mock_instance));
}
