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
fn approve_builds_runs_the_build_with_ndjson_and_silent_reporters() {
    for reporter in ["--reporter=ndjson", "--reporter=silent"] {
        let (harness, workspace) = install_with_ignored_build();

        pacquet(&workspace)
            .with_args([reporter, "approve-builds", "@pnpm.e2e/install-script-example"])
            .assert()
            .success();

        assert!(
            workspace.join(INSTALL_MARKER).exists(),
            "approving the build under {reporter} must run its install script",
        );

        drop(harness);
    }
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

#[test]
fn rebuild_runs_with_ndjson_and_silent_reporters() {
    for reporter in ["--reporter=ndjson", "--reporter=silent"] {
        let harness = CommandTempCwd::init().add_mocked_registry();
        let workspace = harness.workspace.clone();
        fs::write(workspace.join("package.json"), "{}").expect("write package.json");
        pacquet(&workspace).with_arg("install").assert().success();

        pacquet(&workspace).with_args([reporter, "rebuild"]).assert().success();

        drop(harness);
    }
}

/// Package names of the two build-script fixtures used by the multi-dep tests.
const PREPOST: &str = "@pnpm.e2e/pre-and-postinstall-scripts-example";
const INSTALL: &str = "@pnpm.e2e/install-script-example";

/// The postinstall marker the `@pnpm.e2e/pre-and-postinstall-scripts-example`
/// package writes when its build runs.
const PREPOST_MARKER: &str = "node_modules/.pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0\
     /node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js";

/// Install both build-script fixtures with their builds ignored (neither in
/// `allowBuilds`).
fn install_two_with_ignored_builds() -> (CommandTempCwd<AddMockedRegistry>, std::path::PathBuf) {
    let harness = CommandTempCwd::init().add_mocked_registry();
    let workspace = harness.workspace.clone();

    let package_json = serde_json::json!({
        "dependencies": { PREPOST: "1.0.0", INSTALL: "1.0.0" },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");
    disable_strict_dep_builds(&workspace);

    pacquet(&workspace).with_arg("install").assert().success();

    assert!(!workspace.join(PREPOST_MARKER).exists(), "prepost build must be ignored initially");
    assert!(!workspace.join(INSTALL_MARKER).exists(), "install build must be ignored initially");
    (harness, workspace)
}

/// The `allowBuilds` map recorded in the workspace manifest.
fn allow_builds(workspace: &Path) -> std::collections::BTreeMap<String, bool> {
    #[derive(serde::Deserialize)]
    struct Manifest {
        #[serde(rename = "allowBuilds", default)]
        allow_builds: std::collections::BTreeMap<String, bool>,
    }
    let text = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read yaml");
    serde_saphyr::from_str::<Manifest>(&text).expect("parse yaml").allow_builds
}

/// Seed an existing `allowBuilds: { name: true }` entry in the manifest.
fn seed_allow_build(workspace: &Path, name: &str) {
    let path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&path).unwrap_or_default();
    if !yaml.is_empty() && !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    writeln!(yaml, "allowBuilds:\n  '{name}': true").expect("format allowBuilds");
    fs::write(&path, yaml).expect("write pnpm-workspace.yaml");
}

// Ports pnpm's `approve all builds with --all flag`.
#[test]
fn approve_builds_all_flag_builds_everything() {
    let (harness, workspace) = install_two_with_ignored_builds();

    pacquet(&workspace).with_args(["approve-builds", "--all"]).assert().success();

    assert!(workspace.join(PREPOST_MARKER).exists(), "prepost built under --all");
    assert!(workspace.join(INSTALL_MARKER).exists(), "install built under --all");
    assert_eq!(
        allow_builds(&workspace),
        std::collections::BTreeMap::from([
            (INSTALL.to_string(), true),
            (PREPOST.to_string(), true),
        ]),
    );

    drop(harness);
}

// Ports pnpm's `deny builds via !pkg positional arguments`.
#[test]
fn approve_builds_approves_and_denies_via_positional_args() {
    let (harness, workspace) = install_two_with_ignored_builds();

    let deny_install = format!("!{INSTALL}");
    pacquet(&workspace)
        .with_args(["approve-builds", PREPOST, deny_install.as_str()])
        .assert()
        .success();

    assert!(workspace.join(PREPOST_MARKER).exists(), "approved package built");
    assert!(!workspace.join(INSTALL_MARKER).exists(), "denied package not built");
    assert_eq!(
        allow_builds(&workspace),
        std::collections::BTreeMap::from([
            (INSTALL.to_string(), false),
            (PREPOST.to_string(), true),
        ]),
    );

    drop(harness);
}

// Ports pnpm's `deny-only via !pkg keeps other builds pending`.
#[test]
fn approve_builds_deny_only_keeps_other_pending() {
    let (harness, workspace) = install_two_with_ignored_builds();

    let deny_install = format!("!{INSTALL}");
    pacquet(&workspace).with_args(["approve-builds", deny_install.as_str()]).assert().success();

    // Only the denied package is decided; the other stays pending.
    assert_eq!(
        allow_builds(&workspace),
        std::collections::BTreeMap::from([(INSTALL.to_string(), false)]),
    );

    let output = stdout_of(pacquet(&workspace).with_arg("ignored-builds").assert());
    let (automatic, explicit) = output
        .split_once("Explicitly ignored package builds (via allowBuilds):")
        .expect("explicit section present");
    assert!(automatic.contains(PREPOST), "the undecided package stays pending: {output}");
    assert!(explicit.contains(INSTALL), "the denied package is explicitly ignored: {output}");

    drop(harness);
}

// Ports pnpm's `positional args preserve existing allowBuilds entries`.
#[test]
fn approve_builds_preserves_existing_allow_builds_entries() {
    let (harness, workspace) = install_two_with_ignored_builds();
    seed_allow_build(&workspace, "@pnpm.e2e/existing-package");

    pacquet(&workspace).with_args(["approve-builds", PREPOST]).assert().success();

    let builds = allow_builds(&workspace);
    assert_eq!(builds.get("@pnpm.e2e/existing-package"), Some(&true), "existing entry kept");
    assert_eq!(builds.get(PREPOST), Some(&true), "approved package recorded");
    assert!(!builds.contains_key(INSTALL), "unmentioned package not touched: {builds:?}");

    drop(harness);
}

// Ports pnpm's `--all with positional arguments throws error`.
#[test]
fn approve_builds_all_with_args_is_rejected() {
    let CommandTempCwd { workspace, root, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").expect("write package.json");

    let assert =
        pacquet(&workspace).with_args(["approve-builds", "--all", "foo"]).assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(stderr.contains("ERR_PNPM_APPROVE_BUILDS_ALL_WITH_ARGS"), "stderr: {stderr}");

    drop(root);
}

#[test]
fn approve_builds_global_is_rejected() {
    let CommandTempCwd { workspace, root, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").expect("write package.json");

    let assert = pacquet(&workspace).with_args(["approve-builds", "--global"]).assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_APPROVE_BUILDS_NOT_SUPPORTED_WITH_GLOBAL"),
        "stderr: {stderr}",
    );

    drop(root);
}
