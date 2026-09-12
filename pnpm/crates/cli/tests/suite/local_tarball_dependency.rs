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
    fixtures::{tarball_entries, tarball_with_manifest, tarball_without_manifest},
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

/// An entry at the archive root has no top-level directory to strip, so
/// it is keyed by its own name. pnpm 11 installs such an archive, and
/// rejecting it here instead fails the whole install with
/// `ERR_PNPM_TARBALL_IO_ERROR`, only after exhausting the network
/// retries.
///
/// Covers <https://github.com/pnpm/pnpm/issues/14701>.
#[test]
fn local_tarball_with_a_root_level_entry_installs() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    // macOS `bsdtar` emits the zero-length `AppleDouble` `._package` when
    // the source cannot store xattrs natively, so packing with `tar`
    // rather than `npm pack` produces this shape.
    let manifest = serde_json::json!({ "name": "pkg-root-entry", "version": "1.0.0" }).to_string();
    fs::write(
        workspace.join("pkg-root-entry-1.0.0.tgz"),
        tarball_entries(&[("._package", b""), ("package/package.json", manifest.as_bytes())]),
    )
    .expect("write tarball");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "dependencies": { "pkg-root-entry": "file:./pkg-root-entry-1.0.0.tgz" },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet.with_arg("install").assert().success();

    let installed = workspace.join(
        "node_modules/.pnpm/pkg-root-entry@file+pkg-root-entry-1.0.0.tgz/node_modules/pkg-root-entry",
    );
    assert!(
        installed.join("package.json").exists(),
        "the manifest under `package/` must still be extracted to the package root",
    );
    assert!(
        installed.join("._package").exists(),
        "the root-level entry must be extracted under its own name, as pnpm 11 does",
    );

    drop((root, mock_instance));
}

/// An archive with no wrapping directory at all keys its `package.json`
/// at the package root, so the resolve-time read finds the name and
/// version there and the dependency is recorded under them rather than
/// under the alias the consumer happened to give it. pnpm 11 records
/// `real-name@file:...` at `9.9.9` for this archive, and a lockfile the
/// two CLIs disagree about fails a `--frozen-lockfile` install.
#[test]
fn flat_local_tarball_is_recorded_under_its_bundled_name() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("flat-1.0.0.tgz"),
        tarball_entries(&[
            ("package.json", br#"{"name":"real-name","version":"9.9.9"}"#),
            ("index.js", b"module.exports = 1\n"),
            ("lib/helper.js", b"module.exports = 2\n"),
        ]),
    )
    .expect("write tarball");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "dependencies": { "myalias": "file:./flat-1.0.0.tgz" },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet.with_arg("install").assert().success();

    let lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("real-name@file:flat-1.0.0.tgz:"),
        "the bundled name must key the tarball, not the alias `myalias`:\n{lockfile}",
    );
    assert!(
        lockfile.contains("version: 9.9.9"),
        "the version read from the manifest at the archive root must be recorded:\n{lockfile}",
    );

    let installed =
        workspace.join("node_modules/.pnpm/real-name@file+flat-1.0.0.tgz/node_modules/real-name");
    assert!(
        installed.join("index.js").exists(),
        "the package must be materialized under its bundled name",
    );
    // A nested entry still loses its first component, so `lib/helper.js`
    // lands at the package root and `lib/` is gone. pnpm 11 flattens the
    // same way, its `parseString` trimming through the first separator
    // whatever that separator separates.
    assert!(installed.join("helper.js").exists(), "a nested entry is flattened, as on pnpm 11");
    assert!(!installed.join("lib").exists());

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
