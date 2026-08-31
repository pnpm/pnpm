use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::{
    fs,
    path::Path,
    thread,
    time::{Duration, Instant},
};

#[cfg(unix)]
fn write_executable(path: &std::path::Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, body).expect("write executable");
    let mut perms = fs::metadata(path).expect("stat executable").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).expect("chmod executable");
}

fn make_detached_node_script(marker_path: &Path, parent_exit_code: i32) -> String {
    let marker_json = serde_json::to_string(&marker_path.to_string_lossy()).expect("quote marker");
    let detached_script =
        format!("setTimeout(() => require('fs').writeFileSync({marker_json}, 'survived'), 500)");
    let detached_script_json = serde_json::to_string(&detached_script).expect("quote script");
    format!(
        "const {{ spawn }} = require('child_process'); const child = spawn(process.execPath, ['-e', {detached_script_json}], {{ detached: true, stdio: 'ignore' }}); child.unref(); process.exit({parent_exit_code})",
    )
}

#[cfg(target_os = "windows")]
fn make_long_lived_detached_node_script(pid_path: &Path) -> String {
    let pid_path_json = serde_json::to_string(&pid_path.to_string_lossy()).expect("quote PID path");
    let detached_script = format!(
        "const fs = require('fs'); fs.writeFileSync({pid_path_json}, String(process.pid)); setTimeout(() => {{}}, 60000)",
    );
    let detached_script_json = serde_json::to_string(&detached_script).expect("quote script");
    format!(
        "const {{ spawn }} = require('child_process'); const fs = require('fs'); const child = spawn(process.execPath, ['-e', {detached_script_json}], {{ detached: true, stdio: 'ignore' }}); child.unref(); while (!fs.existsSync({pid_path_json})) Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 10); process.exit(1)",
    )
}

#[cfg(target_os = "windows")]
fn assert_process_exits(pid: u32) {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0, WAIT_TIMEOUT},
        System::Threading::{
            OpenProcess, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess,
            WaitForSingleObject,
        },
    };

    // SAFETY: the PID came from the detached child. A non-null handle is valid
    // until it is closed below, and the requested access rights only wait for
    // or terminate that child if the regression leaves it running.
    unsafe {
        let process = OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE, 0, pid);
        if process.is_null() {
            return;
        }
        let wait = WaitForSingleObject(process, 10_000);
        if wait == WAIT_TIMEOUT {
            TerminateProcess(process, 1);
            WaitForSingleObject(process, 10_000);
        }
        CloseHandle(process);
        assert_eq!(wait, WAIT_OBJECT_0, "the detached process should be cleaned up");
    }
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
        .with_arg(format!(r#"touch "{}""#, marker_path.display()))
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

#[test]
fn exec_preserves_a_detached_process_after_success() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker_path = workspace.join("detached-marker.txt");

    pacquet
        .with_arg("exec")
        .with_arg("node")
        .with_arg("-e")
        .with_arg(make_detached_node_script(&marker_path, 0))
        .assert()
        .success();

    let deadline = Instant::now() + Duration::from_secs(10);
    while !marker_path.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    let marker_exists = marker_path.exists();
    eprintln!("DETACHED MARKER EXISTS: {marker_exists}");
    assert!(marker_exists, "the detached process should survive a successful pnpm exec");

    drop(root);
}

#[cfg(target_os = "windows")]
#[test]
fn exec_cleans_up_a_detached_process_after_failure() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let pid_path = workspace.join("detached-pid.txt");

    pacquet
        .with_arg("exec")
        .with_arg("node")
        .with_arg("-e")
        .with_arg(make_long_lived_detached_node_script(&pid_path))
        .assert()
        .failure();

    let pid = fs::read_to_string(&pid_path)
        .expect("read detached process PID")
        .parse()
        .expect("parse detached process PID");
    assert_process_exits(pid);

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
