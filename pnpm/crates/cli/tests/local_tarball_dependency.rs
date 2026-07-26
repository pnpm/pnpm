//! Local `file:*.tgz` dependencies install end to end, recording both
//! lockfile blocks and linking a working `node_modules/<name>`.
//!
//! A `file:` tarball's name, version, and integrity live inside the
//! archive, not in its specifier. pacquet builds the lockfile before the
//! install pass, so the local resolver reads them during resolution —
//! the same reason the remote-tarball resolver downloads at resolve time
//! (see `tarball_url_dependency.rs`). Without the name the dep path
//! stays the bare `file:<path>`, which keys no `packages:` / `snapshots:`
//! entry, and the install exits 0 over a dangling symlink.
//!
//! Covers <https://github.com/pnpm/pnpm/issues/13379>.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fixtures::tarball_with_manifest,
};
use std::{fs, path::Path, process::Command};

fn write_tarball(workspace: &Path, file_name: &str, manifest: &serde_json::Value) {
    fs::write(workspace.join(file_name), tarball_with_manifest(manifest)).expect("write tarball");
}

#[test]
fn local_tarball_dependency_is_recorded_and_installed() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_tarball(
        &workspace,
        "pkg-from-tarball-1.0.0.tgz",
        &serde_json::json!({ "name": "pkg-from-tarball", "version": "1.0.0" }),
    );
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "dependencies": { "pkg-from-tarball": "file:./pkg-from-tarball-1.0.0.tgz" },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet.with_arg("install").assert().success();

    let lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("pkg-from-tarball@file:pkg-from-tarball-1.0.0.tgz:"),
        "the tarball must be keyed by <name>@file:<path> in packages: and snapshots::\n{lockfile}",
    );
    assert!(
        lockfile.contains("tarball: file:pkg-from-tarball-1.0.0.tgz"),
        "the resolution must point back at the local tarball:\n{lockfile}",
    );
    assert!(
        lockfile.contains("version: 1.0.0"),
        "the version read from the bundled manifest must be recorded:\n{lockfile}",
    );

    let installed = workspace.join(
        "node_modules/.pnpm/pkg-from-tarball@file+pkg-from-tarball-1.0.0.tgz/node_modules/pkg-from-tarball/package.json",
    );
    assert!(installed.exists(), "the tarball must be extracted into the virtual store");

    // The frozen install proves the recorded entries are complete enough
    // to install from without re-resolving.
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert!(installed.exists(), "a frozen install must materialize the tarball too");

    drop((root, mock_instance));
}

/// A local tarball's own dependencies are walked. They are readable only
/// from the manifest bundled in the archive, so before it was read at
/// resolve time the package installed with none of them.
#[test]
fn local_tarball_dependency_pulls_in_its_own_dependencies() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_tarball(
        &workspace,
        "tarball-with-deps-1.0.0.tgz",
        &serde_json::json!({
            "name": "tarball-with-deps",
            "version": "1.0.0",
            "dependencies": { "is-positive": "1.0.0" },
        }),
    );
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "dependencies": { "tarball-with-deps": "file:./tarball-with-deps-1.0.0.tgz" },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet.with_arg("install").assert().success();

    let lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("is-positive@1.0.0"),
        "the tarball's own dependency must be resolved:\n{lockfile}",
    );

    let installed = workspace.join(
        "node_modules/.pnpm/tarball-with-deps@file+tarball-with-deps-1.0.0.tgz/node_modules/is-positive/package.json",
    );
    assert!(installed.exists(), "the tarball's dependency must be linked beside it");

    drop((root, mock_instance));
}
