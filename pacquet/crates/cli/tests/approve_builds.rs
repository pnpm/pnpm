//! Integration tests for `pacquet ignored-builds`, `approve-builds`, and
//! `rebuild`. Ports the observable behavior of pnpm's
//! [`approveBuilds`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/commands/src/policy/approveBuilds.ts)
//! and [`ignoredBuilds`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/commands/src/policy/ignoredBuilds.ts)
//! tests: a dependency whose build was ignored is listed by
//! `ignored-builds`, approved by `approve-builds` (which writes
//! `allowBuilds` and re-runs the build), and re-run by `rebuild`.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fmt::Write as _, fs, path::Path, process::Command};

/// The install marker the `@pnpm.e2e/install-script-example` package's
/// `install` lifecycle script writes when it runs.
const INSTALL_MARKER: &str = "node_modules/.pnpm/@pnpm.e2e+install-script-example@1.0.0\
     /node_modules/@pnpm.e2e/install-script-example/generated-by-install.js";

/// Append `strictDepBuilds: false` so an install that intentionally leaves
/// a build ignored completes instead of failing with
/// `ERR_PNPM_IGNORED_BUILDS`.
fn disable_strict_dep_builds(workspace: &Path) {
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = match fs::read_to_string(&yaml_path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => panic!("read pnpm-workspace.yaml: {err}"),
    };
    if !yaml.is_empty() && !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    writeln!(yaml, "strictDepBuilds: false").expect("format strictDepBuilds");
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
}

/// A fresh `pacquet` command rooted at `workspace` (each `assert_cmd`
/// `Command` is single-use, so sequential steps build their own).
fn pacquet(workspace: &Path) -> Command {
    Command::cargo_bin("pacquet").expect("find the pacquet binary").with_current_dir(workspace)
}

/// Set up a workspace that depends on `@pnpm.e2e/install-script-example`
/// and install it with its build ignored (not in `allowBuilds`).
fn install_with_ignored_build() -> (CommandTempCwd<AddMockedRegistry>, std::path::PathBuf) {
    let harness = CommandTempCwd::init().add_mocked_registry();
    let workspace = harness.workspace.clone();

    let package_json = serde_json::json!({
        "dependencies": { "@pnpm.e2e/install-script-example": "1.0.0" },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");
    disable_strict_dep_builds(&workspace);

    pacquet(&workspace).with_arg("install").assert().success();

    assert!(
        !workspace.join(INSTALL_MARKER).exists(),
        "the build must be ignored on the initial install",
    );
    (harness, workspace)
}

fn stdout_of(command: assert_cmd::assert::Assert) -> String {
    String::from_utf8(command.success().get_output().stdout.clone()).expect("utf-8 stdout")
}

#[test]
fn ignored_builds_lists_the_blocked_dependency() {
    let (harness, workspace) = install_with_ignored_build();

    let output = stdout_of(pacquet(&workspace).with_arg("ignored-builds").assert());
    assert!(
        output.contains("Automatically ignored builds during installation:"),
        "output: {output}",
    );
    assert!(output.contains("@pnpm.e2e/install-script-example"), "output: {output}");

    drop(harness);
}

#[test]
fn approve_builds_with_args_runs_the_build() {
    let (harness, workspace) = install_with_ignored_build();

    pacquet(&workspace)
        .with_args(["approve-builds", "@pnpm.e2e/install-script-example"])
        .assert()
        .success();

    assert!(
        workspace.join(INSTALL_MARKER).exists(),
        "approving the build must run its install script",
    );

    let yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read yaml");
    assert!(
        yaml.contains("allowBuilds:") && yaml.contains("@pnpm.e2e/install-script-example"),
        "allowBuilds entry written: {yaml}",
    );

    // With its only ignored build approved, `.modules.yaml` no longer
    // records an `ignoredBuilds` field, so the package is no longer listed.
    // (pnpm prints "Cannot identify as no node_modules found" when the
    // field is absent — "None" is reserved for a present-but-empty list.)
    let output = stdout_of(pacquet(&workspace).with_arg("ignored-builds").assert());
    assert!(
        !output.contains("@pnpm.e2e/install-script-example"),
        "the approved build is no longer reported as ignored: {output}",
    );

    drop(harness);
}

#[test]
fn approve_builds_deny_keeps_the_build_ignored() {
    let (harness, workspace) = install_with_ignored_build();

    pacquet(&workspace)
        .with_args(["approve-builds", "!@pnpm.e2e/install-script-example"])
        .assert()
        .success();

    assert!(
        !workspace.join(INSTALL_MARKER).exists(),
        "denying the build must not run its install script",
    );

    let output = stdout_of(pacquet(&workspace).with_arg("ignored-builds").assert());
    assert!(
        output.contains("Explicitly ignored package builds (via allowBuilds):"),
        "denied build is reported as explicitly ignored: {output}",
    );

    drop(harness);
}

#[test]
fn approve_builds_with_nothing_pending_reports_so() {
    let harness = CommandTempCwd::init().add_mocked_registry();
    let workspace = harness.workspace.clone();
    fs::write(workspace.join("package.json"), "{}").expect("write package.json");
    pacquet(&workspace).with_arg("install").assert().success();

    let output = stdout_of(pacquet(&workspace).with_arg("approve-builds").assert());
    assert!(output.contains("There are no packages awaiting approval"), "output: {output}");

    drop(harness);
}

#[test]
fn rebuild_reruns_an_approved_build() {
    let (harness, workspace) = install_with_ignored_build();

    // Approve via `allowBuilds` and run the build once.
    pacquet(&workspace)
        .with_args(["approve-builds", "@pnpm.e2e/install-script-example"])
        .assert()
        .success();
    let marker = workspace.join(INSTALL_MARKER);
    assert!(marker.exists(), "approve-builds must have run the build");

    // Delete the marker and rebuild: the script must run again.
    fs::remove_file(&marker).expect("remove install marker");
    pacquet(&workspace)
        .with_args(["rebuild", "@pnpm.e2e/install-script-example"])
        .assert()
        .success();
    assert!(marker.exists(), "rebuild must re-run the install script");

    drop(harness);
}
