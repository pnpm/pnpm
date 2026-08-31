//! E2E coverage for the `verify-deps-before-run` gate, mirroring the
//! TypeScript scenarios in `pnpm11/pnpm/test/verifyDepsBeforeRun/` that
//! translate to pacquet (the interactive `prompt` flow needs a PTY and
//! is exercised only through its non-interactive error branch).

use crate::_utils;
pub use _utils::*;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::bump_mtime,
};
use serde_json::json;
use std::{fs, path::Path};

fn write_manifest(workspace: &Path, marker: &Path) {
    write_manifest_with_dependency_groups(workspace, marker, json!({}));
}

/// The fixture manifest — a `hello` script that touches `marker` —
/// extended with the dependency groups the caller needs.
fn write_manifest_with_dependency_groups(
    workspace: &Path,
    marker: &Path,
    groups: serde_json::Value,
) {
    let serde_json::Value::Object(mut manifest) = json!({
        "name": "verify-deps-project",
        "version": "0.0.0",
        "scripts": {
            "hello": format!(r#"touch "{}""#, marker.display()),
        },
    }) else {
        unreachable!("the manifest literal is an object")
    };
    let serde_json::Value::Object(groups) = groups else {
        panic!("the dependency groups must be an object")
    };
    manifest.extend(groups);
    fs::write(workspace.join("package.json"), serde_json::Value::Object(manifest).to_string())
        .expect("write package.json");
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

/// The spawned install reproduces the dependency groups the last
/// install recorded, spelled the way the CLI accepts them, so a
/// production-only install leaves `pnpm run` working
/// ([pnpm/pnpm#14147](https://github.com/pnpm/pnpm/issues/14147)).
#[cfg(unix)]
#[test]
fn install_action_reruns_a_production_only_install() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let marker = workspace.join("marker.txt");
    let write_project = |foo_version: &str| {
        write_manifest_with_dependency_groups(
            &workspace,
            &marker,
            json!({
                "dependencies": {
                    "@pnpm.e2e/foo": foo_version,
                },
                "devDependencies": {
                    "@pnpm.e2e/bar": "100.0.0",
                },
            }),
        );
    };

    write_project("100.0.0");
    pacquet.with_args(["install", "--prod"]).assert().success();
    assert!(
        !workspace.join("node_modules/@pnpm.e2e/bar").exists(),
        "a production-only install must skip devDependencies",
    );

    write_project("100.1.0");
    bump_mtime(&workspace.join("package.json"));

    pacquet_in(&workspace).with_args(["run", "hello"]).assert().success();
    assert!(marker.exists(), "the script must run after the spawned install");
    let installed: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(workspace.join("node_modules/@pnpm.e2e/foo/package.json"))
            .expect("read the installed @pnpm.e2e/foo manifest"),
    )
    .expect("parse the installed @pnpm.e2e/foo manifest");
    assert_eq!(
        installed["version"], "100.1.0",
        "the spawned install must install the updated production dependency",
    );
    assert!(
        !workspace.join("node_modules/@pnpm.e2e/bar").exists(),
        "the spawned install must keep the recorded production-only groups",
    );

    drop((root, mock_instance));
}

#[test]
fn dedupe_peers_lockfile_regeneration_installs_before_running_the_script() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    append_workspace_yaml_key(&workspace, "dedupePeers", true);
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "verify-deps-project",
            "version": "0.0.0",
            "dependencies": {
                "@pnpm.e2e/foo": "100.0.0",
            },
            "scripts": {
                "hello": r#"node -e "require('fs').appendFileSync('postinstall.log', 'h')""#,
                "postinstall": r#"node -e "require('fs').appendFileSync('postinstall.log', 'x')""#,
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet.with_arg("install").assert().success();
    assert_eq!(
        fs::read_to_string(workspace.join("postinstall.log")).expect("read postinstall log"),
        "x",
    );

    fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("remove pnpm-lock.yaml");
    pacquet_in(&workspace).with_args(["install", "--lockfile-only"]).assert().success();
    bump_mtime(&workspace.join("pnpm-lock.yaml"));
    let regenerated_lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read regenerated lockfile");

    let output = pacquet_in(&workspace)
        .with_args(["run", "hello"])
        .output()
        .expect("run script after lockfile regeneration");
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDERR:\n{stderr}\n");
    assert!(output.status.success(), "the script must run successfully");
    assert!(
        stderr.contains("Lockfile is up to date, resolution step is skipped"),
        "the verifier install must reuse the regenerated lockfile:\n{stderr}",
    );
    let policy_verdict = stderr
        .find("Lockfile passes supply-chain policies")
        .expect("the verifier must report its lockfile policy verdict");
    let frozen_install = stderr
        .find("Lockfile is up to date, resolution step is skipped")
        .expect("the verifier must report the frozen install");
    let up_to_date = stderr
        .find("Already up to date")
        .expect("the verifier must report that no packages changed");
    assert!(
        policy_verdict < frozen_install && frozen_install < up_to_date,
        "the verifier messages must match pnpm's order:\n{stderr}",
    );
    assert_eq!(
        fs::read_to_string(workspace.join("pnpm-lock.yaml"))
            .expect("read lockfile after verifier install"),
        regenerated_lockfile,
    );
    assert_eq!(
        fs::read_to_string(workspace.join("postinstall.log")).expect("read postinstall log"),
        "xxh",
    );

    pacquet_in(&workspace).with_args(["run", "hello"]).assert().success();
    assert_eq!(
        fs::read_to_string(workspace.join("postinstall.log")).expect("read postinstall log"),
        "xxhh",
    );

    drop((root, mock_instance));
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
    let manifest = fs::read_to_string(workspace.join("package.json")).expect("read package.json");
    fs::write(workspace.join("package.json"), manifest).expect("rewrite package.json");
    bump_mtime(&workspace.join("package.json"));
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
    bump_mtime(&workspace.join("package.json"));
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

/// A present-but-empty env var disables the gate outright, still
/// overriding the CLI: pnpm applies the variable on presence alone, and
/// the empty string is falsy there.
#[cfg(unix)]
#[test]
fn empty_env_value_disables_the_gate() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("marker.txt");
    write_manifest(&workspace, &marker);

    pacquet
        .with_env("pnpm_config_verify_deps_before_run", "")
        .with_args(["--config.verify-deps-before-run=error", "run", "hello"])
        .assert()
        .success();
    assert!(marker.exists(), "the script must run with the gate disabled by the empty env var");
    assert!(!workspace.join("node_modules").exists(), "no check or install may run");

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

#[test]
fn exec_keeps_verifier_output_out_of_child_stdout() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "workspace-root", "version": "0.0.0" }).to_string(),
    )
    .expect("write root package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    let project = workspace.join("packages/project");
    fs::create_dir_all(&project).expect("create workspace project");
    write_manifest(&project, &project.join("marker.txt"));

    let output = pacquet_in(&project)
        .with_args([
            "exec",
            "node",
            "-e",
            r#"process.stdout.write(JSON.stringify({workspace:"test"}))"#,
        ])
        .output()
        .expect("spawn pacquet exec");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDOUT:\n{stdout}\n\nSTDERR:\n{stderr}\n");
    assert!(output.status.success(), "exec failed");
    assert_eq!(stdout, r#"{"workspace":"test"}"#);
    assert!(
        stderr.contains("Scope: all 2 workspace projects") && stderr.contains("Done in"),
        "the verifier install must report on stderr:\n{stderr}",
    );

    drop(root);
}

#[test]
fn ndjson_exec_keeps_verifier_output_machine_readable() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &workspace.join("marker.txt"));

    let output = pacquet
        .with_args([
            "--reporter=ndjson",
            "exec",
            "node",
            "-e",
            r#"process.stdout.write(JSON.stringify({workspace:"test"}))"#,
        ])
        .output()
        .expect("spawn ndjson pacquet exec");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDOUT:\n{stdout}\n\nSTDERR:\n{stderr}\n");
    assert!(output.status.success(), "ndjson exec failed");
    assert_eq!(stdout, r#"{"workspace":"test"}"#);
    assert!(!stderr.is_empty(), "the verifier install must report NDJSON events");
    for line in stderr.lines() {
        serde_json::from_str::<serde_json::Value>(line)
            .unwrap_or_else(|err| panic!("invalid NDJSON line {line:?}: {err}"));
    }

    drop(root);
}

#[test]
fn silent_exec_suppresses_verifier_output() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &workspace.join("marker.txt"));

    let output = pacquet
        .with_args([
            "exec",
            "--silent",
            "node",
            "-e",
            r#"process.stdout.write(JSON.stringify({workspace:"test"}))"#,
        ])
        .output()
        .expect("spawn silent pacquet exec");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDOUT:\n{stdout}\n\nSTDERR:\n{stderr}\n");
    assert!(output.status.success(), "silent exec failed");
    assert_eq!(stdout, r#"{"workspace":"test"}"#);
    assert_eq!(stderr, "");

    drop(root);
}

#[test]
fn silent_recursive_exec_suppresses_verifier_output() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &workspace.join("marker.txt"));
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - .\n")
        .expect("write pnpm-workspace.yaml");

    let output = pacquet
        .with_args([
            "--silent",
            "--recursive",
            "exec",
            "node",
            "-e",
            r#"process.stdout.write(JSON.stringify({workspace:"test"}))"#,
        ])
        .output()
        .expect("spawn silent recursive pacquet exec");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDOUT:\n{stdout}\n\nSTDERR:\n{stderr}\n");
    assert!(output.status.success(), "silent recursive exec failed");
    assert_eq!(stdout, r#"{"workspace":"test"}"#);
    assert_eq!(stderr, "");

    drop(root);
}

#[test]
#[cfg_attr(not(unix), ignore = "the fixture script uses the POSIX `touch` command")]
fn silent_recursive_run_suppresses_verifier_output() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("marker.txt");
    write_manifest(&workspace, &marker);
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - .\n")
        .expect("write pnpm-workspace.yaml");

    let output = pacquet
        .with_args(["--silent", "--recursive", "run", "hello"])
        .output()
        .expect("spawn silent recursive pacquet run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDOUT:\n{stdout}\n\nSTDERR:\n{stderr}\n");
    assert!(output.status.success(), "silent recursive run failed");
    assert_eq!(stdout, "");
    assert_eq!(stderr, "");
    assert!(marker.exists(), "the script must run after the verifier install");

    drop(root);
}
