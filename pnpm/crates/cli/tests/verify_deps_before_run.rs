//! E2E coverage for the `verify-deps-before-run` gate, mirroring the
//! TypeScript scenarios in `pnpm11/pnpm/test/verifyDepsBeforeRun/` that
//! translate to pacquet (the interactive `prompt` flow needs a PTY and
//! is exercised only through its non-interactive error branch).

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::{fs, path::Path, process::Command};

fn pacquet_in(workspace: &Path) -> Command {
    Command::cargo_bin("pacquet").expect("find the pacquet binary").with_current_dir(workspace)
}

fn write_manifest(workspace: &Path, marker: &Path) {
    let manifest = json!({
        "name": "verify-deps-project",
        "version": "0.0.0",
        "scripts": {
            "hello": format!(r#"touch "{}""#, marker.display()),
        },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
}

/// The default action is `install` (pnpm's
/// `'verify-deps-before-run': 'install'`): a fresh project's first
/// `run` spawns an install before executing the script.
#[cfg(unix)]
#[test]
fn default_install_action_installs_before_running_the_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("marker.txt");
    write_manifest(&workspace, &marker);

    pacquet.with_args(["run", "hello"]).assert().success();
    assert!(marker.exists(), "the script must run after the spawned install");
    assert!(workspace.join("node_modules").exists(), "the gate must have spawned an install first");

    drop(root);
}

#[cfg(unix)]
#[test]
fn error_action_follows_the_dependency_state() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("marker.txt");
    write_manifest(&workspace, &marker);

    let output = pacquet
        .with_args(["--config.verify-deps-before-run=error", "run", "hello"])
        .output()
        .expect("spawn pacquet run");
    assert!(!output.status.success(), "running before any install must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_VERIFY_DEPS_BEFORE_RUN")
            && stderr.contains("Cannot check whether dependencies are outdated"),
        "expected the verify-deps error:\n{stderr}",
    );
    assert!(!marker.exists(), "the script must not run");

    pacquet_in(&workspace).with_arg("install").assert().success();
    pacquet_in(&workspace)
        .with_args(["--config.verify-deps-before-run=error", "run", "hello"])
        .assert()
        .success();
    assert!(marker.exists(), "the script must run once dependencies are in sync");

    // An mtime-only rewrite (same content) must still pass: the gate
    // re-checks the content against the lockfile instead of trusting
    // the mtime.
    std::thread::sleep(std::time::Duration::from_millis(20));
    let manifest = fs::read_to_string(workspace.join("package.json")).expect("read package.json");
    fs::write(workspace.join("package.json"), manifest).expect("rewrite package.json");
    pacquet_in(&workspace)
        .with_args(["--config.verify-deps-before-run=error", "run", "hello"])
        .assert()
        .success();

    // Deleting pnpm-lock.yaml in a dependency-less project leaves no
    // current lockfile to stand in for it, so the check fails like
    // pnpm's RUN_CHECK_DEPS_LOCKFILE_NOT_FOUND — and the pre-run check
    // must not recreate the file (pnpm's run path never restores the
    // lockfile; only the install command does).
    fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("remove pnpm-lock.yaml");
    let output = pacquet_in(&workspace)
        .with_args(["--config.verify-deps-before-run=error", "run", "hello"])
        .output()
        .expect("spawn pacquet run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDERR:\n{stderr}\n");
    assert!(!output.status.success(), "a missing lockfile must fail");
    assert!(stderr.contains("Cannot find a lockfile in"), "expected the lockfile error:\n{stderr}");
    assert!(
        !workspace.join("pnpm-lock.yaml").exists(),
        "the pre-run check must not write pnpm-lock.yaml",
    );
    pacquet_in(&workspace).with_arg("install").assert().success();
    assert!(workspace.join("pnpm-lock.yaml").exists(), "install must restore the lockfile");

    // A manifest that no longer matches the lockfile must fail again.
    let mut manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(workspace.join("package.json")).expect("read package.json"),
    )
    .expect("parse package.json");
    manifest["dependencies"] = json!({ "@pnpm.e2e/foo": "100.0.0" });
    fs::write(workspace.join("package.json"), manifest.to_string())
        .expect("write modified package.json");
    let output = pacquet_in(&workspace)
        .with_args(["--config.verify-deps-before-run=error", "run", "hello"])
        .output()
        .expect("spawn pacquet run");
    assert!(!output.status.success(), "an out-of-sync manifest must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_VERIFY_DEPS_BEFORE_RUN"),
        "expected the verify-deps error:\n{stderr}",
    );

    drop(root);
}

/// `warn` reports the drift but still runs the script.
#[cfg(unix)]
#[test]
fn warn_action_warns_and_runs_the_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("marker.txt");
    write_manifest(&workspace, &marker);

    let output = pacquet
        .with_args(["--config.verify-deps-before-run=warn", "run", "hello"])
        .output()
        .expect("spawn pacquet run");
    assert!(output.status.success(), "warn mode must not block the script");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Your node_modules are out of sync with your lockfile."),
        "expected the out-of-sync warning:\n{stderr}",
    );
    assert!(marker.exists(), "the script must run");
    assert!(!workspace.join("node_modules").exists(), "warn mode must not install");

    drop(root);
}

/// `prompt` cannot ask in a non-interactive environment and must fail
/// with the dedicated hint instead of hanging.
#[test]
fn prompt_action_errors_when_not_interactive() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &workspace.join("marker.txt"));

    let output = pacquet
        .with_args(["--config.verify-deps-before-run=prompt", "run", "hello"])
        .output()
        .expect("spawn pacquet run");
    assert!(!output.status.success(), "prompt mode must fail without a TTY");
    let stderr = String::from_utf8_lossy(&output.stderr);
    // miette wraps the help text, so collapse whitespace before matching.
    let stderr_flat = stderr.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        stderr.contains("ERR_PNPM_VERIFY_DEPS_BEFORE_RUN")
            && stderr_flat
                .contains("cannot prompt for confirmation in non-interactive environments"),
        "expected the non-interactive prompt error:\n{stderr}",
    );

    drop(root);
}

/// `false` disables the gate entirely: the script runs and nothing is
/// installed.
#[cfg(unix)]
#[test]
fn false_disables_the_gate() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("marker.txt");
    write_manifest(&workspace, &marker);

    pacquet.with_args(["--config.verify-deps-before-run=false", "run", "hello"]).assert().success();
    assert!(marker.exists(), "the script must run");
    assert!(!workspace.join("node_modules").exists(), "no install may be spawned");

    drop(root);
}

/// Every spawned script sees `pnpm_config_verify_deps_before_run=false`,
/// so a nested `pnpm run` / `pnpm exec` never re-enters the check
/// (pnpm/pnpm#10060). Mirrors the TS `checkEnv` assertions.
#[cfg(unix)]
#[test]
fn scripts_get_the_check_disabled_through_their_env() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = json!({
        "name": "verify-deps-project",
        "version": "0.0.0",
        "scripts": {
            "checkEnv": r#"[ "$pnpm_config_verify_deps_before_run" = "false" ]"#,
        },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    pacquet.with_args(["run", "checkEnv"]).assert().success();

    drop(root);
}

/// The `pnpm_config_verify_deps_before_run` env var outranks even the
/// CLI `--config.` override — that priority is what makes the script
/// env stamp above an effective recursion breaker.
#[cfg(unix)]
#[test]
fn env_var_outranks_the_cli_config_override() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("marker.txt");
    write_manifest(&workspace, &marker);

    pacquet
        .with_env("pnpm_config_verify_deps_before_run", "false")
        .with_args(["--config.verify-deps-before-run=error", "run", "hello"])
        .assert()
        .success();
    assert!(marker.exists(), "the script must run with the check disabled by env");

    drop(root);
}

/// The exec path stamps the same recursion guard as the lifecycle env
/// builder.
#[cfg(unix)]
#[test]
fn exec_children_get_the_check_disabled_through_their_env() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &workspace.join("marker.txt"));

    pacquet
        .with_args([
            "--config.verify-deps-before-run=false",
            "exec",
            "sh",
            "-c",
            r#"[ "$pnpm_config_verify_deps_before_run" = "false" ]"#,
        ])
        .assert()
        .success();

    drop(root);
}

/// pnpm assigns the `pnpm_config_verify_deps_before_run` env var
/// verbatim, so an unrecognized value is truthy there: the check runs
/// but matches no action, and the script proceeds.
#[cfg(unix)]
#[test]
fn unrecognized_env_value_checks_without_acting() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("marker.txt");
    write_manifest(&workspace, &marker);

    pacquet
        .with_env("pnpm_config_verify_deps_before_run", "definitely-not-an-action")
        .with_args(["--config.verify-deps-before-run=error", "run", "hello"])
        .assert()
        .success();
    assert!(marker.exists(), "the script must run");
    assert!(!workspace.join("node_modules").exists(), "no action may fire");

    drop(root);
}

/// `pnpm exec` runs the same gate as `pnpm run`.
#[cfg(unix)]
#[test]
fn exec_runs_the_gate_too() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &workspace.join("marker.txt"));

    let output = pacquet
        .with_args(["--config.verify-deps-before-run=error", "exec", "true"])
        .output()
        .expect("spawn pacquet exec");
    assert!(!output.status.success(), "exec before any install must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_VERIFY_DEPS_BEFORE_RUN"),
        "expected the verify-deps error:\n{stderr}",
    );

    pacquet_in(&workspace).with_arg("install").assert().success();
    pacquet_in(&workspace)
        .with_args(["--config.verify-deps-before-run=error", "exec", "true"])
        .assert()
        .success();

    drop(root);
}
