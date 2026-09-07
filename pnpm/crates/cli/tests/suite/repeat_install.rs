//! End-to-end coverage for installs over an **existing** `node_modules`
//! — the repair, reuse, and divergence scenarios of upstream's
//! `deps-restorer` and `deps-installer` suites. Every test installs
//! once, damages or drifts some part of the on-disk state, installs
//! again, and asserts the second install converges without rebuilding
//! what was still valid.

#![cfg(unix)] // pnpm CLI: 'program not found' on Windows runners.

use crate::_utils;
pub use _utils::*;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fixtures::tarball_with_manifest,
};
use std::{fs, os::unix::fs::MetadataExt, path::Path};

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

/// TS: `available packages are relinked during forced install`
/// (`deps-restorer index.ts:469`): a forced frozen install relinks
/// every package the lockfile names, not just the diff against the
/// previous install — a file removed from an already-materialized
/// package's virtual-store copy comes back, and the unchanged package
/// is re-reported as `resolved` alongside the newly added one.
#[test]
fn available_packages_are_relinked_during_forced_install() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "@pnpm.e2e/foobarqar": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");
    pacquet.with_arg("install").assert().success();

    // Extend the manifest and wanted lockfile without touching
    // `node_modules` — the CLI equivalent of upstream's fixture swap
    // from `has-glob` to `has-glob-and-rimraf`.
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

    // Damage the already-materialized package: a plain frozen install
    // skips it as unchanged (its slot dir still exists), so only the
    // forced relink restores the file.
    let foobarqar_manifest = workspace.join(
        "node_modules/.pnpm/@pnpm.e2e+foobarqar@1.0.0/node_modules/@pnpm.e2e/foobarqar/package.json",
    );
    fs::remove_file(&foobarqar_manifest).expect("remove a file of the materialized package");

    let output = pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile", "--force", "--reporter=ndjson"])
        .output()
        .expect("run pacquet");
    assert_success(&output);

    assert!(workspace.join("node_modules/@pnpm.e2e/pkg-with-1-dep").exists());
    assert!(
        foobarqar_manifest.exists(),
        "the forced install must re-import the already-available package",
    );
    let resolved: Vec<String> = ndjson_records(&output)
        .iter()
        .filter(|record| record["name"] == "pnpm:progress" && record["status"] == "resolved")
        .filter_map(|record| record["packageId"].as_str().map(str::to_string))
        .collect();
    for package_id in ["@pnpm.e2e/foobarqar@1.0.0", "@pnpm.e2e/pkg-with-1-dep@100.0.0"] {
        assert!(
            resolved.iter().any(|id| id == package_id),
            "{package_id} must be re-reported as resolved: {resolved:?}",
        );
    }

    drop((root, mock_instance));
}

/// pnpm's `file:` is a copy taken at install time, not a symlink, so
/// each install must re-copy: the source can change with no lockfile
/// change to signal it.
#[test]
fn a_directory_dependency_is_recopied_when_its_source_changes() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let local = workspace.join("local-pkg");
    fs::create_dir_all(&local).expect("create the local package dir");
    let write_local = |marker: &str| {
        fs::write(
            local.join("package.json"),
            serde_json::json!({ "name": "local-pkg", "version": "1.0.0" }).to_string(),
        )
        .expect("write the local package.json");
        fs::write(local.join("marker.txt"), marker).expect("write the marker");
    };
    write_local("first");

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "private": true,
            "dependencies": { "local-pkg": "file:./local-pkg" },
        })
        .to_string(),
    )
    .expect("write the root package.json");

    pacquet.with_arg("install").assert().success();
    let linked_marker = workspace.join("node_modules/local-pkg/marker.txt");
    assert_eq!(
        fs::read_to_string(&linked_marker).expect("read the linked marker"),
        "first",
        "the first install should materialize the directory dependency",
    );

    // The version is left alone so nothing in the lockfile changes and
    // only a re-copy of the source can surface the edit.
    write_local("second");
    pacquet_in(&workspace).with_arg("install").assert().success();

    assert_eq!(
        fs::read_to_string(&linked_marker).expect("read the linked marker"),
        "second",
        "the second install should re-copy the directory into its slot",
    );

    drop((root, mock_instance));
}

/// A workspace whose only dependency (`@pnpm.e2e/pkg-with-1-dep`, one
/// subdep) is declared by the member, installed with
/// `nodeLinker: hoisted`. Returns the path of a file inside the hoisted
/// subdep, whose inode tells a repeat install that re-imported the tree
/// from one that left it alone.
fn install_hoisted_workspace_member(
    pacquet: std::process::Command,
    workspace: &Path,
) -> std::path::PathBuf {
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    yaml.push_str("nodeLinker: hoisted\npackages:\n  - member\n");
    fs::write(&workspace_yaml, yaml).expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "ws-root", "private": true }).to_string(),
    )
    .expect("write the root package.json");
    fs::create_dir_all(workspace.join("member")).expect("mkdir member");
    fs::write(
        workspace.join("member/package.json"),
        serde_json::json!({
            "name": "member",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write the member package.json");

    pacquet.with_arg("install").assert().success();

    assert!(
        !workspace.join("member/node_modules").exists(),
        "hoisted keeps the member's registry deps in the root node_modules",
    );
    workspace.join("node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json")
}

/// pnpm/pnpm#14001: under `nodeLinker: hoisted` every project's
/// dependencies are installed into the *root* modules directory, so a
/// member's own `node_modules` is normally absent. The workspace-state
/// fast path must check the root directory for every project instead of
/// reading that absence as a project that was never installed.
#[test]
fn repeat_hoisted_install_with_workspace_member_deps_is_up_to_date() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let hoisted_manifest = install_hoisted_workspace_member(pacquet, &workspace);
    let inode_before = fs::metadata(&hoisted_manifest).expect("stat the hoisted dep").ino();

    let second = pacquet_in(&workspace).with_arg("install").assert().success();
    let second_output = String::from_utf8_lossy(&second.get_output().stdout).into_owned();
    assert!(
        second_output.contains("Already up to date"),
        "the repeat install must short-circuit: {second_output}",
    );
    assert_eq!(
        fs::metadata(&hoisted_manifest).expect("stat the hoisted dep").ino(),
        inode_before,
        "the second install must re-import nothing",
    );

    drop((root, mock_instance));
}

/// pnpm/pnpm#14495: an unchanged local tarball does not make a hoisted
/// workspace reinstall its registry dependency tree.
#[test]
fn repeat_hoisted_install_with_unchanged_local_tarball_is_up_to_date() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    yaml.push_str("nodeLinker: hoisted\npackages:\n  - member\n");
    fs::write(&workspace_yaml, yaml).expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "ws-root", "private": true }).to_string(),
    )
    .expect("write the root package.json");
    fs::create_dir_all(workspace.join("member")).expect("mkdir member");
    fs::write(
        workspace.join("member/package.json"),
        serde_json::json!({
            "name": "member",
            "version": "1.0.0",
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
                "local-pkg": "file:../vendor/local-pkg.tgz",
            },
        })
        .to_string(),
    )
    .expect("write the member package.json");
    fs::create_dir_all(workspace.join("vendor")).expect("mkdir vendor");
    fs::write(
        workspace.join("vendor/local-pkg.tgz"),
        tarball_with_manifest(&serde_json::json!({
            "name": "local-pkg",
            "version": "1.0.0",
        })),
    )
    .expect("write local tarball");

    pacquet.with_arg("install").assert().success();
    let hoisted_manifest =
        workspace.join("node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json");
    let inode_before = fs::metadata(&hoisted_manifest).expect("stat the hoisted dep").ino();

    let second = pacquet_in(&workspace).with_arg("install").assert().success();
    let second_output = String::from_utf8_lossy(&second.get_output().stdout).into_owned();
    assert!(
        second_output.contains("Already up to date"),
        "the unchanged tarball must leave the fast path available: {second_output}",
    );
    assert_eq!(
        fs::metadata(&hoisted_manifest).expect("stat the hoisted dep").ino(),
        inode_before,
        "the second install must re-import nothing",
    );

    drop((root, mock_instance));
}

/// A hoisted install writes no virtual-store slot, so the pipeline must
/// not probe one: it reported every package of the tree it had just
/// written as broken (pnpm/pnpm#14001). Dropping the workspace-state
/// file gets this install past the repeat-install short-circuit and into
/// the pipeline.
#[test]
fn repeat_hoisted_install_reports_nothing_broken() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let hoisted_manifest = install_hoisted_workspace_member(pacquet, &workspace);
    fs::remove_file(workspace.join("node_modules/.pnpm-workspace-state-v1.json"))
        .expect("remove the workspace state");

    let second =
        pacquet_in(&workspace).with_args(["install", "--reporter=ndjson"]).assert().success();
    let second_events = String::from_utf8_lossy(&second.get_output().stderr).into_owned();
    assert!(
        !second_events.contains("pnpm:_broken_node_modules"),
        "a hoisted tree has no virtual-store slots to report broken: {second_events}",
    );
    assert!(hoisted_manifest.is_file(), "the hoisted dep must still be in place");

    drop((root, mock_instance));
}
