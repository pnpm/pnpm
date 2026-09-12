use super::{Path, prod_spec, write_json};
use crate::_utils::append_workspace_yaml_key;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fixtures::tarball_with_manifest,
};
use pretty_assertions::assert_eq;
use std::fs;

/// Write `<workspace>/localpkg/package.json`, the package the
/// directory-shaped selectors below point at.
fn write_local_package(workspace: &Path) {
    let package_dir = workspace.join("localpkg");
    fs::create_dir_all(&package_dir).expect("create local package dir");
    write_json(
        &package_dir.join("package.json"),
        &serde_json::json!({ "name": "localpkg", "version": "1.0.0" }),
    );
}

/// A bare relative path is a directory dependency, so it saves as
/// `link:` — the protocol the local resolver normalizes an
/// un-injected directory to.
#[test]
fn a_relative_directory_path_saves_as_a_link() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_local_package(&workspace);

    pacquet.with_args(["add", "./localpkg"]).assert().success();

    assert_eq!(prod_spec(&workspace, "localpkg"), "link:localpkg");
    assert!(
        workspace.join("node_modules/localpkg/package.json").exists(),
        "the local package must be installed",
    );

    drop((root, npmrc_info));
}

/// The `file:` protocol asks for copy semantics, so it is kept.
#[test]
fn the_file_protocol_on_a_directory_is_kept() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_local_package(&workspace);

    pacquet.with_args(["add", "file:./localpkg"]).assert().success();

    assert_eq!(prod_spec(&workspace, "localpkg"), "file:localpkg");

    drop((root, npmrc_info));
}

/// A local tarball's name lives in the `package.json` it bundles, so
/// the archive has to be read before the manifest entry can be
/// written.
#[test]
fn a_local_tarball_path_saves_as_a_file_spec() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    fs::write(
        workspace.join("pkg-from-tarball-1.0.0.tgz"),
        tarball_with_manifest(
            &serde_json::json!({ "name": "pkg-from-tarball", "version": "1.0.0" }),
        ),
    )
    .expect("write tarball");

    pacquet.with_args(["add", "./pkg-from-tarball-1.0.0.tgz"]).assert().success();

    assert_eq!(prod_spec(&workspace, "pkg-from-tarball"), "file:pkg-from-tarball-1.0.0.tgz");
    assert!(
        workspace.join("node_modules/pkg-from-tarball/package.json").exists(),
        "the tarball package must be installed",
    );

    drop((root, npmrc_info));
}

/// A remote tarball is the same shape one step further out: the name
/// is inside the archive, so the resolver downloads it.
///
/// The URL points at the mocked registry through `localhost` while it
/// is configured as `127.0.0.1`, so the prefix doesn't match and the
/// tarball resolver — not the npm resolver — claims it (see
/// `tarball_url_dependency.rs`).
#[test]
fn a_remote_tarball_url_is_saved_verbatim() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let tarball = format!(
        "{}is-positive/-/is-positive-1.0.0.tgz",
        mock_instance.url().replace("127.0.0.1", "localhost"),
    );

    pacquet.with_args(["add", &tarball]).assert().success();

    assert_eq!(prod_spec(&workspace, "is-positive"), tarball);
    assert!(
        workspace.join("node_modules/is-positive/package.json").exists(),
        "the remote tarball package must be installed",
    );

    drop((root, mock_instance));
}

/// A catalog entry is read by every project referencing it, so it
/// cannot hold a path that resolves against the project declaring it.
/// `catalogMode` has to leave such a specifier direct — cataloging it
/// writes an entry the next install refuses with
/// `ERR_PNPM_CATALOG_ENTRY_INVALID_SPEC`.
#[test]
fn a_local_directory_is_not_auto_cataloged() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_local_package(&workspace);
    append_workspace_yaml_key(&workspace, "catalogMode", "prefer");

    pacquet.with_args(["add", "./localpkg"]).assert().success();

    assert_eq!(prod_spec(&workspace, "localpkg"), "link:localpkg");
    let workspace_yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml"))
        .expect("read pnpm-workspace.yaml");
    assert!(
        !workspace_yaml.contains("localpkg"),
        "the local dependency must not reach the catalog:\n{workspace_yaml}",
    );

    drop((root, npmrc_info));
}

/// The name keys the manifest entry and names the `node_modules`
/// directory the package is linked into, so a directory that declares
/// none is refused instead of guessed at — and the project is left
/// untouched.
#[test]
fn a_directory_declaring_no_name_is_refused() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let package_dir = workspace.join("nameless");
    fs::create_dir_all(&package_dir).expect("create local package dir");
    write_json(&package_dir.join("package.json"), &serde_json::json!({ "version": "1.0.0" }));
    write_json(
        &workspace.join("package.json"),
        &serde_json::json!({ "name": "project", "version": "1.0.0" }),
    );
    let manifest_before =
        fs::read_to_string(workspace.join("package.json")).expect("read manifest");

    let output = pacquet.with_args(["add", "./nameless"]).output().expect("run pnpm add");
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDERR:\n{stderr}\n");
    assert!(!output.status.success());
    assert!(stderr.contains("ERR_PNPM_MISSING_PACKAGE_NAME"), "stderr:\n{stderr}");
    assert_eq!(
        fs::read_to_string(workspace.join("package.json")).expect("reread manifest"),
        manifest_before,
    );

    drop((root, npmrc_info));
}
