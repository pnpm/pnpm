#![cfg(unix)]

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
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
    assert_side_effects_materialized(false);
}

/// TS: `using side effects cache with nodeLinker=hoisted`
/// (`deps-restorer/test/index.ts:706`).
#[test]
fn side_effects_materialized_on_warm_frozen_reinstall_with_hoisted_linker() {
    assert_side_effects_materialized(true);
}

fn assert_side_effects_materialized(hoisted: bool) {
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
    if hoisted {
        yaml.push_str("nodeLinker: hoisted\n");
    }
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
    let postinstall_artifact = if hoisted {
        workspace.join(
            "node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js",
        )
    } else {
        workspace.join(
            "node_modules/.pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0\
             /node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js",
        )
    };

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

/// A cache hit skips the scripts, so it emits no `pnpm:lifecycle` output.
/// Without a report of its own the install is indistinguishable from one
/// whose dependencies have no build scripts, which leaves a user whose
/// script also writes outside the package directory nothing to go on.
///
/// Covers <https://github.com/pnpm/pnpm/issues/14717>.
#[test]
fn a_cached_build_is_reported_with_its_skipped_stages() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml.push_str("allowBuilds:\n  '@pnpm.e2e/pre-and-postinstall-scripts-example': true\n");
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");

    // The install that actually builds reports the build through
    // `pnpm:lifecycle` and must not claim anything was restored.
    let first_install = pacquet.with_arg("install").assert().success();
    let built = String::from_utf8_lossy(&first_install.get_output().stdout).into_owned();
    assert!(
        built.contains("pre-and-postinstall-scripts-example postinstall$"),
        "the first install must run and report the build:\n{built}",
    );
    assert!(
        !built.contains("restored from the side-effects cache"),
        "a build that ran must not be reported as restored:\n{built}",
    );

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    let restored = frozen_install_stdout(&workspace);
    assert!(
        !restored.contains("pre-and-postinstall-scripts-example postinstall$"),
        "the warm reinstall must hit the cache rather than rebuild:\n{restored}",
    );
    // Every stage the build consists of, in run order. `prepare` is a
    // project stage, so it is not among them.
    assert!(
        restored.contains(
            "@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0 (preinstall, install, postinstall)"
        ),
        "the report must name the package and each stage that did not run:\n{restored}",
    );
    // `sideEffectsCache.read`, not `sideEffectsCache`: the gate consults
    // `Config::side_effects_cache_read`, which `sideEffectsCacheReadonly`
    // also turns on, so a reader who set the boolean to false would still
    // see the skip and have followed dead advice.
    assert!(
        restored.contains("Set sideEffectsCache.read to false"),
        "the report must name the setting that actually gates the skip:\n{restored}",
    );

    // The advice has to work from the state the reader is in, including
    // the one where the boolean alone would not have been enough.
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    yaml.push_str("sideEffectsCacheReadonly: true\nsideEffectsCache:\n  read: false\n");
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    let rebuilt = frozen_install_stdout(&workspace);
    assert!(
        rebuilt.contains("pre-and-postinstall-scripts-example postinstall$"),
        "following the report's advice must run the scripts again:\n{rebuilt}",
    );
    assert!(
        !rebuilt.contains("restored from the side-effects cache"),
        "and must stop reporting a restore:\n{rebuilt}",
    );

    drop((root, mock_instance));
}

/// A fresh `pacquet install --frozen-lockfile` against an existing
/// workspace. The registry config lives in the workspace's `.npmrc` /
/// `pnpm-workspace.yaml` and the mock registry is a process-global
/// singleton kept alive by the caller, so this only needs its own
/// command — no extra `CommandTempCwd` / registry.
fn run_frozen_install(workspace: &Path) {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
}

/// The report is about the install, not about one project in it, so it
/// has to survive an install started from a workspace member — where the
/// lockfile directory is the workspace root and the working directory is
/// not. The default reporter drops a project-prefixed info message whose
/// prefix is not the working directory, so a prefixed report would go
/// missing exactly here.
#[test]
fn a_cached_build_is_reported_from_a_workspace_member() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml.push_str("packages:\n  - packages/*\n");
    yaml.push_str("allowBuilds:\n  '@pnpm.e2e/pre-and-postinstall-scripts-example': true\n");
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "version": "1.0.0", "private": true }).to_string(),
    )
    .expect("write the root package.json");
    let member = workspace.join("packages/a");
    fs::create_dir_all(&member).expect("create the member dir");
    fs::write(
        member.join("package.json"),
        serde_json::json!({
            "name": "pkg-a",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write the member package.json");

    pacquet.with_arg("install").assert().success();
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&member)
        .with_args(["install", "--frozen-lockfile"])
        .output()
        .expect("run the install from the member");
    assert!(output.status.success(), "install must succeed: {output:?}");
    let restored = String::from_utf8_lossy(&output.stdout);
    assert!(
        restored.contains(
            "@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0 (preinstall, install, postinstall)"
        ),
        "the report must survive an install started from a workspace member:\n{restored}",
    );

    drop((root, mock_instance));
}

/// [`run_frozen_install`], returning what the install printed.
fn frozen_install_stdout(workspace: &Path) -> String {
    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .with_args(["install", "--frozen-lockfile"])
        .output()
        .expect("run the install");
    assert!(output.status.success(), "install must succeed: {output:?}");
    String::from_utf8_lossy(&output.stdout).into_owned()
}
