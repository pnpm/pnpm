#![cfg(unix)]

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path, process::Command};

/// Regression for <https://github.com/pnpm/pnpm/issues/12042#issuecomment-4682732058>:
/// a package approved via `allowBuilds` whose lifecycle script produces
/// files not in its tarball (e.g. `bun`'s postinstall downloading a
/// binary) loses that output on a warm frozen reinstall.
///
/// `sideEffectsCache` is on by default, so the first build seeds the
/// cache. On the second frozen install the `is_built` gate skips the
/// rebuild — the cached build output must still be materialized into the
/// freshly linked slot, mirroring pnpm's `getFlatMap` applying the
/// side-effects diff at import time. Without that, the slot is left with
/// only the pristine tarball files and the package is broken at runtime.
#[test]
fn side_effects_materialized_on_warm_frozen_reinstall() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    // `allowBuilds` in `pnpm-workspace.yaml`, exactly like the report.
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml.push_str("allowBuilds:\n  '@pnpm.e2e/pre-and-postinstall-scripts-example': true\n");
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");

    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");

    // `generated-by-postinstall.js` is written by the package's
    // postinstall and is not part of its tarball, so it only exists if
    // the build ran or its cached output was materialized.
    let postinstall_artifact = workspace.join(
        "node_modules/.pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0\
         /node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js",
    );

    eprintln!("First install (non-frozen, writes lockfile + populates store)...");
    pacquet.with_arg("install").assert().success();

    eprintln!("Wiping node_modules before the first frozen install...");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    eprintln!("Frozen install (builds, writes the side-effects cache)...");
    run_frozen_install(&workspace);
    assert!(postinstall_artifact.exists(), "postinstall must run on the first frozen install");

    eprintln!("Wiping node_modules (keep store + lockfile, like a fresh CI checkout)...");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    eprintln!("Frozen reinstall (warm store, hits the is_built gate)...");
    run_frozen_install(&workspace);
    assert!(
        postinstall_artifact.exists(),
        "the cached postinstall output must be materialized after a warm frozen reinstall",
    );

    drop((root, mock_instance));
}

/// A fresh `pacquet install --frozen-lockfile` against an existing
/// workspace. The registry config lives in the workspace's `.npmrc` /
/// `pnpm-workspace.yaml` and the mock registry is a process-global
/// singleton kept alive by the caller, so this only needs its own
/// command — no extra `CommandTempCwd` / registry.
fn run_frozen_install(workspace: &Path) {
    Command::cargo_bin("pacquet")
        .expect("find the pacquet binary")
        .with_current_dir(workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
}
