//! End-to-end coverage for a `file:` directory dependency that has no
//! `package.json` of its own.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::fs;

/// The directory has no manifest, so its version is unknown. A ranged
/// `packageExtensions` selector must not match it, while a bare `name`
/// selector still does.
///
/// Covers <https://github.com/pnpm/pnpm/issues/15007>.
#[test]
fn ranged_package_extension_skips_a_directory_without_a_manifest() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::create_dir_all(workspace.join("debug")).expect("create the dependency directory");
    fs::write(workspace.join("debug/index.js"), "").expect("write index.js");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "dependencies": { "debug": "file:./debug" },
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packageExtensions:\n  'debug@<1':\n    dependencies:\n      is-positive: 1.0.0\n  debug:\n    dependencies:\n      is-negative: 1.0.0\n",
    )
    .expect("write pnpm-workspace.yaml");

    pacquet
        .with_arg("install")
        .assert()
        .success();

    let lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    assert!(
        !lockfile.contains("is-positive"),
        "the ranged selector must not match a directory without a manifest:\n{lockfile}",
    );
    assert!(
        lockfile.contains("is-negative"),
        "the bare selector must match a directory without a manifest:\n{lockfile}",
    );

    drop((root, mock_instance));
}
