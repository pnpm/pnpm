use super::{
    allow_builds,
    append_workspace_yaml_key,
    set_strict_dep_builds,
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{
    AddMockedRegistry,
    CommandTempCwd,
};
use std::{
    fs,
    path::Path,
};

/// Regression test for the user-reported gap: `pacquet add <pkg>`
/// takes the fresh-lockfile path, which never ran the build phase —
/// so a blocked dependency build script was silently ignored, and
/// the install exited 0, unlike `pnpm add`. Under the default
/// `strictDepBuilds`, the install now fails with
/// `ERR_PNPM_IGNORED_BUILDS` after adding the dependency.
#[test]
fn add_fails_under_strict_dep_builds_when_a_build_is_ignored() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(workspace.join("package.json"), "{}\n").expect("write package.json");

    // No `allowBuilds` and `strictDepBuilds` defaults to true, so the
    // blocked build must fail the install with a non-zero exit, like
    // `pnpm add`.
    let output = pacquet
        .with_args(["add", "@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0"])
        .output()
        .expect("run pacquet add");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("pacquet add stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(!output.status.success(), "strict add with an ignored build must exit non-zero");
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("ERR_PNPM_IGNORED_BUILDS")
            && combined.contains("Ignored build scripts")
            && combined.contains("@pnpm.e2e/pre-and-postinstall-scripts-example"),
        "expected ERR_PNPM_IGNORED_BUILDS naming the package; got:\n{combined}",
    );

    // The dependency was still added and materialized; only its
    // blocked scripts did not run. Mirrors pnpm, which writes the
    // manifest and the artifacts before failing.
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(workspace.join("package.json")).unwrap())
            .expect("parse package.json");
    assert_eq!(
        manifest["dependencies"],
        serde_json::json!({ "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" }),
    );
    let pkg_dir = workspace.join(
        "node_modules/.pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0\
             /node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example",
    );
    assert!(pkg_dir.join("package.json").exists(), "dependency should be materialized");
    assert!(!pkg_dir.join("generated-by-preinstall.js").exists());
    assert!(!pkg_dir.join("generated-by-postinstall.js").exists());

    drop((root, mock_instance));
}

/// `pacquet install --ignore-scripts` must exit 0 even under the
/// default `strictDepBuilds`, with no `allowBuilds`: `--ignore-scripts`
/// suppresses every dependency build, so nothing is reported as an
/// ignored build and the strict gate never fires. The dependency is
/// still materialized, but its lifecycle scripts do not run. Mirrors
/// pnpm's `--ignore-scripts`, which leaves `ignoredBuilds` empty.
#[test]
fn install_ignore_scripts_does_not_fail_under_strict_dep_builds() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");

    // No `allowBuilds` and `strictDepBuilds` defaults to true: a plain
    // `pacquet install` would fail with `ERR_PNPM_IGNORED_BUILDS`.
    // `--ignore-scripts` must make it exit 0 instead.
    let output = pacquet
        .with_args(["install", "--ignore-scripts"])
        .output()
        .expect("run pacquet install --ignore-scripts");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("pacquet install --ignore-scripts stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(output.status.success(), "install --ignore-scripts must exit zero");
    assert!(
        !format!("{stdout}{stderr}").contains("ERR_PNPM_IGNORED_BUILDS"),
        "--ignore-scripts must not report ignored builds",
    );

    let pkg_dir = workspace.join(
        "node_modules/.pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0\
             /node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example",
    );
    assert!(pkg_dir.join("package.json").exists(), "dependency should be materialized");
    assert!(!pkg_dir.join("generated-by-preinstall.js").exists());
    assert!(!pkg_dir.join("generated-by-postinstall.js").exists());

    drop((root, mock_instance));
}

/// With `strictDepBuilds: false`, an ignored build is a non-fatal
/// warning printed to stdout and the install exits 0 — the
/// ignored-build-scripts warning box.
#[test]
fn add_warns_without_strict_dep_builds_when_a_build_is_ignored() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(workspace.join("package.json"), "{}\n").expect("write package.json");
    set_strict_dep_builds(&workspace, false);

    let output = pacquet
        .with_args(["add", "@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0"])
        .output()
        .expect("run pacquet add");
    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("pacquet add stdout:\n{stdout}");
    assert!(output.status.success(), "non-strict add must exit zero");
    assert!(
        stdout.contains("Ignored build scripts")
            && stdout.contains("@pnpm.e2e/pre-and-postinstall-scripts-example"),
        "expected an ignored-build-scripts warning naming the package; got:\n{stdout}",
    );

    let pkg_dir = workspace.join(
        "node_modules/.pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0\
             /node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example",
    );
    assert!(!pkg_dir.join("generated-by-preinstall.js").exists());

    drop((root, mock_instance));
}

/// `strictDepBuilds` must stay enforced across reruns: after a strict
/// install fails with `ERR_PNPM_IGNORED_BUILDS`, a warm rerun must
/// fail again rather than short-circuit to exit 0 via an up-to-date
/// fast path — otherwise rerunning install would bypass the gate.
#[test]
fn strict_install_keeps_failing_on_warm_rerun() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");

    // First install (strict default, no `allowBuilds`): fails, but
    // still materializes the dep and records the ignored build in
    // `.modules.yaml`.
    let first = pacquet
        .with_arg("install")
        .output()
        .expect("run pacquet install");
    assert!(!first.status.success(), "first strict install with an ignored build must fail");

    // Warm rerun: the lockfile and `.modules.yaml` are unchanged, so
    // the up-to-date fast path would normally exit 0 — but strict mode
    // must keep failing until the build is approved.
    let CommandTempCwd {
        pacquet: rerun, root: rerun_root, ..
    } = CommandTempCwd::init().add_mocked_registry();
    let rerun_out = rerun
        .with_current_dir(&workspace)
        .with_arg("install")
        .output()
        .expect("run pacquet install again");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&rerun_out.stdout),
        String::from_utf8_lossy(&rerun_out.stderr),
    );
    eprintln!("rerun output:\n{combined}");
    assert!(
        !rerun_out.status.success(),
        "a warm rerun must not bypass strictDepBuilds; got:\n{combined}",
    );
    assert!(
        combined.contains("ERR_PNPM_IGNORED_BUILDS"),
        "rerun should still report ERR_PNPM_IGNORED_BUILDS; got:\n{combined}",
    );

    drop((root, mock_instance, rerun_root));
}

/// A corrupt / unreadable `.modules.yaml` must not let a strict rerun
/// short-circuit to exit 0: the up-to-date fast paths can't prove the
/// absence of recorded ignored builds from an unparsable state file,
/// so they conservatively fall through to the full install, which
/// fails again with `ERR_PNPM_IGNORED_BUILDS`.
#[test]
fn strict_install_keeps_failing_with_unreadable_modules_yaml() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");

    let first = pacquet
        .with_arg("install")
        .output()
        .expect("run pacquet install");
    assert!(!first.status.success(), "first strict install with an ignored build must fail");

    // Corrupt the recorded state so it can't be parsed.
    fs::write(workspace.join("node_modules/.modules.yaml"), "}{ not: valid: yaml")
        .expect("corrupt .modules.yaml");

    let CommandTempCwd {
        pacquet: rerun, root: rerun_root, ..
    } = CommandTempCwd::init().add_mocked_registry();
    let rerun_out = rerun
        .with_current_dir(&workspace)
        .with_arg("install")
        .output()
        .expect("run pacquet install again");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&rerun_out.stdout),
        String::from_utf8_lossy(&rerun_out.stderr),
    );
    eprintln!("rerun output:\n{combined}");
    assert!(
        !rerun_out.status.success(),
        "a strict rerun with a corrupt .modules.yaml must not exit 0; got:\n{combined}",
    );

    drop((root, mock_instance, rerun_root));
}

/// TS: `the list of ignored builds is preserved after a repeat
/// install` (`pnpm/test/install/lifecycleScripts.ts:245`). A repeat
/// install re-reports every ignored build and leaves the recorded
/// set intact — dropping an entry would make an approved-nothing
/// rerun look clean.
#[test]
fn ignored_builds_are_preserved_after_a_repeat_install() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(workspace.join("package.json"), "{}\n").expect("write package.json");

    // Both commands exit non-zero: under the default `strictDepBuilds`
    // an ignored build is `ERR_PNPM_IGNORED_BUILDS`, not a warning. The
    // add still materializes the packages and records them, and the
    // point under test is that the repeat install re-reports the same
    // set rather than a stale rerun looking clean.
    let add_out = pacquet
        .with_args([
            "add",
            "@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0",
            "@pnpm.e2e/install-script-example@1.0.0",
            "--config.optimistic-repeat-install=false",
        ])
        .output()
        .expect("run pacquet add");
    assert!(
        !add_out.status.success(),
        "a strict add with ignored builds must fail; got:\n{}{}",
        String::from_utf8_lossy(&add_out.stdout),
        String::from_utf8_lossy(&add_out.stderr),
    );

    let CommandTempCwd {
        pacquet: rerun, root: rerun_root, ..
    } = CommandTempCwd::init().add_mocked_registry();
    let rerun_out = rerun
        .with_current_dir(&workspace)
        .with_args(["install", "--config.optimistic-repeat-install=false"])
        .output()
        .expect("run pacquet install again");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&rerun_out.stdout),
        String::from_utf8_lossy(&rerun_out.stderr),
    );
    eprintln!("repeat install output:\n{combined}");
    assert!(!rerun_out.status.success(), "the strict repeat install must keep failing");
    assert!(
        combined.contains("Ignored build scripts"),
        "the repeat install must report the ignored builds again; got:\n{combined}",
    );

    let recorded = read_ignored_builds(&workspace);
    assert_eq!(
        recorded,
        [
            "@pnpm.e2e/install-script-example@1.0.0",
            "@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0",
        ],
    );

    drop((root, mock_instance, rerun_root));
}

/// TS: `strictDepBuilds fails for packages with cached side-effects`
/// (`pnpm/test/install/lifecycleScripts.ts:380`,
/// <https://github.com/pnpm/pnpm/issues/11035>). Revoking a build
/// approval must fail the strict gate even though the store already
/// holds that package's build output — the side-effects cache is an
/// optimization, never an approval.
#[test]
fn strict_dep_builds_fails_for_packages_with_cached_side_effects() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");
    set_strict_dep_builds(&workspace, true);
    append_workspace_yaml_key(&workspace, "optimisticRepeatInstall", false);
    allow_builds(&workspace, &[("@pnpm.e2e/pre-and-postinstall-scripts-example", true)]);

    pacquet
        .with_arg("install")
        .assert()
        .success();
    let built_marker = workspace.join(
            "node_modules/.pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0\
             /node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js",
        );
    assert!(built_marker.exists(), "the approved build must run and populate the cache");

    allow_builds(&workspace, &[]);

    let CommandTempCwd {
        pacquet: rerun, root: rerun_root, ..
    } = CommandTempCwd::init().add_mocked_registry();
    let rerun_out = rerun
        .with_current_dir(&workspace)
        .with_arg("install")
        .output()
        .expect("run pacquet install after revoking approval");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&rerun_out.stdout),
        String::from_utf8_lossy(&rerun_out.stderr),
    );
    eprintln!("revoked-approval install output:\n{combined}");
    assert!(
        !rerun_out.status.success(),
        "a cached build must not satisfy the strict gate; got:\n{combined}",
    );
    assert!(
        combined.contains("Ignored build scripts"),
        "the revoked package must be reported as ignored; got:\n{combined}",
    );

    drop((root, mock_instance, rerun_root));
}

fn read_ignored_builds(workspace: &Path) -> Vec<String> {
    let mut recorded: Vec<String> = pnpm_modules_yaml::read_modules_manifest::<
        pnpm_modules_yaml::Host,
    >(&workspace.join("node_modules"))
    .expect("read .modules.yaml")
    .expect(".modules.yaml exists")
    .ignored_builds
    .unwrap_or_default()
    .into_iter()
    .map(|dep_path| dep_path.as_str().to_string())
    .collect();
    recorded.sort();
    recorded
}
