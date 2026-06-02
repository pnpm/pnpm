use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;

#[cfg(unix)]
use std::fs;

#[cfg(unix)]
fn write_executable(path: &std::path::Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, body).expect("write executable");
    let mut perms = fs::metadata(path).expect("stat executable").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).expect("chmod executable");
}

/// `pacquet exec <command>` resolves the command against the project's
/// `node_modules/.bin` directory and runs it. Mirrors pnpm's exec, which
/// prepends `./node_modules/.bin` to PATH before spawning.
#[cfg(unix)]
#[test]
fn exec_runs_binary_from_node_modules_bin() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let bin_dir = workspace.join("node_modules").join(".bin");
    fs::create_dir_all(&bin_dir).expect("create node_modules/.bin");
    let marker_path = workspace.join("marker.txt");
    write_executable(
        &bin_dir.join("say-hi"),
        &format!("#!/bin/sh\ntouch \"{}\"\n", marker_path.display()),
    );

    pacquet.with_arg("exec").with_arg("say-hi").assert().success();
    assert!(marker_path.exists(), "the binary in node_modules/.bin should have run");

    drop(root);
}

/// Arguments after the command name flow through to the spawned binary.
#[cfg(unix)]
#[test]
fn exec_passes_arguments_to_the_command() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let bin_dir = workspace.join("node_modules").join(".bin");
    fs::create_dir_all(&bin_dir).expect("create node_modules/.bin");
    let marker_path = workspace.join("args.txt");
    write_executable(
        &bin_dir.join("write-arg"),
        &format!("#!/bin/sh\nprintf %s \"$1\" > \"{}\"\n", marker_path.display()),
    );

    pacquet.with_arg("exec").with_arg("write-arg").with_arg("hello-world").assert().success();
    let written = fs::read_to_string(&marker_path).expect("read marker");
    assert_eq!(written, "hello-world");

    drop(root);
}

/// `pacquet exec` with no command is an error, mirroring pnpm's
/// `ERR_PNPM_EXEC_MISSING_COMMAND`.
#[test]
fn exec_errors_when_no_command_given() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet.with_arg("exec").output().expect("spawn pacquet exec");
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDERR:\n{stderr}\n");
    assert!(!output.status.success(), "exec with no command must fail");
    assert!(
        stderr.contains("requires a command to run"),
        "the failure must be the missing-command diagnostic, not an incidental crash",
    );

    drop(root);
}

/// A command that cannot be resolved against PATH surfaces as a failure,
/// mirroring pnpm's "Command not found" error.
#[test]
fn exec_errors_when_command_not_found() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet
        .with_arg("exec")
        .with_arg("definitely-not-a-real-binary-xyzzy")
        .output()
        .expect("spawn pacquet exec");
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDERR:\n{stderr}\n");
    assert!(!output.status.success(), "a missing command must fail");
    assert!(
        stderr.contains("definitely-not-a-real-binary-xyzzy") && stderr.contains("not found"),
        "the failure must name the missing command, not be an incidental crash",
    );

    drop(root);
}

/// `--shell-mode` / `-c` runs the command through the platform shell
/// rather than resolving it as a binary.
///
/// Compiles everywhere but is ignored on Windows: the assertion relies on
/// the POSIX `touch` command, which `cmd.exe` does not provide.
#[test]
#[cfg_attr(target_os = "windows", ignore = "relies on the POSIX `touch` command")]
fn exec_shell_mode_runs_shell_command() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker_path = workspace.join("shell-marker.txt");

    pacquet
        .with_arg("exec")
        .with_arg("-c")
        .with_arg(format!("touch \"{}\"", marker_path.display()))
        .assert()
        .success();
    assert!(marker_path.exists(), "shell-mode command should have run");

    drop(root);
}

/// A shell-mode command with embedded quotes reaches the shell untouched.
/// On Windows the default `cmd /d /s /c` path is `windows_verbatim_args`,
/// so the joined command must be appended with `raw_arg`; a plain `arg`
/// would escape the inner quotes and break `node -e "..."`. Runs on every
/// platform but is load-bearing on Windows CI.
#[test]
fn exec_shell_mode_preserves_embedded_quotes() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet
        .with_arg("exec")
        .with_arg("-c")
        .with_arg(r#"node -e "process.stdout.write('shell-quote-ok')""#)
        .output()
        .expect("spawn pacquet exec");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "shell-mode command must exit 0, got: {output:?}");
    assert!(stdout.contains("shell-quote-ok"), "embedded quotes must survive; stdout: {stdout:?}");

    drop(root);
}

/// The child's non-zero exit code is propagated as pacquet's own exit
/// code, mirroring pnpm's `{ exitCode }` return.
///
/// Compiles everywhere but is ignored on Windows: shell-mode runs through
/// `cmd.exe` there, and pacquet does not yet honor the verbatim-argument
/// handling that exit-code propagation through `cmd /c` would require.
#[test]
#[cfg_attr(
    target_os = "windows",
    ignore = "shell-mode exit-code propagation through cmd.exe is not wired up yet"
)]
fn exec_propagates_nonzero_exit_code() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet
        .with_arg("exec")
        .with_arg("-c")
        .with_arg("exit 3")
        .output()
        .expect("spawn pacquet exec");
    assert_eq!(output.status.code(), Some(3), "the child's exit code must propagate");

    drop(root);
}

/// pnpm's `makeEnv` stamps `PNPM_PACKAGE_NAME` from the project's
/// `package.json#name` (makeEnv.ts:30-32). Have the spawned command
/// echo the env var to a marker file and assert it reads back the
/// expected name. Also exercises `read_package_name` end-to-end.
#[cfg(unix)]
#[test]
fn exec_stamps_pnpm_package_name_from_manifest() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = serde_json::json!({
        "name": "@scope/mypkg",
        "version": "0.0.0",
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
    let marker = workspace.join("pkgname.txt");

    pacquet
        .with_arg("exec")
        .with_arg("sh")
        .with_arg("-c")
        .with_arg(format!(r#"printf %s "$PNPM_PACKAGE_NAME" > "{}""#, marker.display()))
        .assert()
        .success();

    let written = fs::read_to_string(&marker).expect("read marker");
    assert_eq!(written, "@scope/mypkg");

    drop(root);
}
