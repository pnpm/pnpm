//! End-to-end coverage for local `file:*.tgz` dependencies.
//!
//! A `file:` tarball's name, version, and integrity live inside the
//! archive, not in its specifier, and pacquet builds the lockfile before
//! the install pass — so the local resolver reads them during
//! resolution, for the same reason the remote-tarball resolver downloads
//! at resolve time (see `tarball_url_dependency.rs`).
//!
//! Covers <https://github.com/pnpm/pnpm/issues/13379>.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fixtures::{tarball_with_manifest, tarball_without_manifest},
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

/// An archive with no `package.json` at all is the one tarball shape
/// with no name of its own to prefix its dep path with, so pnpm falls
/// back to the alias the consumer gave it.
///
/// Covers <https://github.com/pnpm/pnpm/issues/13410>.
#[test]
fn local_tarball_without_a_bundled_manifest_installs_under_its_alias() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(workspace.join("no-manifest-1.0.0.tgz"), tarball_without_manifest())
        .expect("write tarball");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "dependencies": { "no-manifest": "file:./no-manifest-1.0.0.tgz" },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet.with_arg("install").assert().success();

    let lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("no-manifest@file:no-manifest-1.0.0.tgz:"),
        "the alias must key the tarball in packages: and snapshots::\n{lockfile}",
    );
    assert!(
        lockfile.contains("version: 0.0.0"),
        "a package with no manifest is recorded at version 0.0.0:\n{lockfile}",
    );

    let installed = workspace
        .join("node_modules/.pnpm/no-manifest@file+no-manifest-1.0.0.tgz/node_modules/no-manifest");
    assert!(installed.join("README.md").exists(), "the archive's contents must be extracted");
    let placeholder =
        fs::read_to_string(installed.join("package.json")).expect("read the placeholder manifest");
    assert!(
        placeholder.contains("_pnpmPlaceholder"),
        "an extraction with no manifest of its own gets pnpm's placeholder: {placeholder}",
    );

    drop((root, mock_instance));
}

/// A local tarball's own dependencies are readable only from the
/// manifest bundled in the archive, so they exercise the resolve-time
/// read from a second angle: the dep path alone would not reveal them.
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
