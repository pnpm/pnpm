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
fn make_connected_detached_node_script(ready_path: &Path, port: u16) -> String {
    let ready_path_json =
        serde_json::to_string(&ready_path.to_string_lossy()).expect("quote ready path");
    let detached_script = format!(
        "const fs = require('fs'); const socket = require('net').createConnection({{ host: '127.0.0.1', port: {port} }}, () => fs.writeFileSync({ready_path_json}, 'ready')); socket.on('data', () => process.exit(0))",
    );
    let detached_script_json = serde_json::to_string(&detached_script).expect("quote script");
    format!(
        "const {{ spawn }} = require('child_process'); const fs = require('fs'); const child = spawn(process.execPath, ['-e', {detached_script_json}], {{ detached: true, stdio: 'ignore' }}); child.unref(); const deadline = Date.now() + 10000; while (!fs.existsSync({ready_path_json}) && Date.now() < deadline) Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 10); process.exit(fs.existsSync({ready_path_json}) ? 1 : 42)",
    )
}

#[cfg(target_os = "windows")]
fn assert_connection_closes(mut connection: std::net::TcpStream) {
    use std::io::{ErrorKind, Read, Write};

    // A socket accepted from a non-blocking listener inherits that mode on
    // Windows, which makes the read below return `WouldBlock` at once and
    // the timeout moot — the assertion would race the job object's cleanup
    // rather than wait for it.
    connection.set_nonblocking(false).expect("set detached child connection blocking");
    connection
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("set detached child connection timeout");
    let mut byte = [0];
    let process_exited = match connection.read(&mut byte) {
        Ok(0) => true,
        Ok(_) => false,
        Err(err)
            if matches!(
                err.kind(),
                ErrorKind::BrokenPipe
                    | ErrorKind::ConnectionAborted
                    | ErrorKind::ConnectionReset
                    | ErrorKind::NotConnected,
            ) =>
        {
            true
        }
        Err(err) if matches!(err.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => false,
        Err(err) => panic!("read detached child connection: {err}"),
    };
    if !process_exited {
        let _ = connection.write_all(b"exit");
        panic!("the detached process should be cleaned up");
    }
}

/// Wait for the detached fixture to connect, terminating `parent` if it never
/// does so a failed test does not leave the chain running.
#[cfg(target_os = "windows")]
fn accept_detached_connection(
    listener: &std::net::TcpListener,
    parent: &mut std::process::Child,
) -> std::net::TcpStream {
    use std::io::ErrorKind;

    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        match listener.accept() {
            Ok((connection, _)) => return connection,
            Err(err) if err.kind() == ErrorKind::WouldBlock && Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                let _ = parent.kill();
                let _ = parent.wait();
                panic!("the detached process did not connect before the deadline");
            }
            Err(err) => panic!("accept detached child connection: {err}"),
        }
    }
}

/// Launch `pacquet` the way `nr` from `@antfu/ni` does: as a direct child of
/// Node without a shell, which places it in libuv's Job Object. `on_close` is
/// the JavaScript run with pacquet's exit `code` once it exits.
#[cfg(target_os = "windows")]
fn node_launching_pacquet(
    pacquet: &std::process::Command,
    args: &[&str],
    on_close: &str,
) -> std::process::Command {
    let pacquet_path_json = serde_json::to_string(&pacquet.get_program().to_string_lossy())
        .expect("quote pacquet path");
    let args_json = serde_json::to_string(args).expect("quote pacquet args");
    let mut node = std::process::Command::new("node");
    node.arg("-e").arg(format!(
        "const child = require('child_process').spawn({pacquet_path_json}, {args_json}, {{ stdio: 'inherit' }}); child.on('close', code => {{ {on_close} }})",
    ));
    if let Some(dir) = pacquet.get_current_dir() {
        node.current_dir(dir);
    }
    for (name, value) in pacquet.get_envs() {
        match value {
            Some(value) => node.env(name, value),
            None => node.env_remove(name),
        };
    }
    node
}

/// JavaScript for [`node_launching_pacquet`] that records pacquet's exit in
/// `exited_path` and keeps Node alive until `release_path` appears, so the
/// test can tell a process killed by pacquet's job from one killed by Node's.
#[cfg(target_os = "windows")]
fn linger_after_pacquet_exits(exited_path: &Path, release_path: &Path) -> String {
    let exited_path_json =
        serde_json::to_string(&exited_path.to_string_lossy()).expect("quote exited path");
    let release_path_json =
        serde_json::to_string(&release_path.to_string_lossy()).expect("quote release path");
    format!(
        "const fs = require('fs'); fs.writeFileSync({exited_path_json}, String(code)); const deadline = Date.now() + 30000; const wait = () => fs.existsSync({release_path_json}) || Date.now() > deadline ? process.exit(code) : setTimeout(wait, 50); wait()",
    )
}

fn wait_for_file(path: &Path) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    path.exists()
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

    let marker_exists = wait_for_file(&marker_path);
    eprintln!("DETACHED MARKER EXISTS: {marker_exists}");
    assert!(marker_exists, "the detached process should survive a successful pnpm exec");

    drop(root);
}

#[cfg(target_os = "windows")]
#[test]
fn exec_preserves_a_detached_process_after_success_when_node_launches_pnpm() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker_path = workspace.join("detached-marker.txt");
    let detached_script = make_detached_node_script(&marker_path, 0);

    node_launching_pacquet(
        &pacquet,
        &["exec", "node", "-e", &detached_script],
        "process.exit(code)",
    )
    .assert()
    .success();

    let marker_exists = wait_for_file(&marker_path);
    eprintln!("DETACHED MARKER EXISTS: {marker_exists}");
    assert!(
        marker_exists,
        "the detached process should survive a successful pnpm exec launched from Node",
    );

    drop(root);
}

#[cfg(target_os = "windows")]
#[test]
fn exec_cleans_up_a_detached_process_after_failure() {
    use std::net::TcpListener;

    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let ready_path = workspace.join("detached-ready.txt");
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("listen for detached child");
    listener.set_nonblocking(true).expect("set listener nonblocking");
    let port = listener.local_addr().expect("read listener address").port();

    let mut pacquet_process = pacquet
        .with_arg("exec")
        .with_arg("node")
        .with_arg("-e")
        .with_arg(make_connected_detached_node_script(&ready_path, port))
        .spawn()
        .expect("spawn pacquet exec");

    let connection = accept_detached_connection(&listener, &mut pacquet_process);
    let status = pacquet_process.wait().expect("wait for pacquet exec");
    assert_eq!(status.code(), Some(1), "the fixture must reach its intentional failure");
    assert_connection_closes(connection);

    drop(root);
}

/// Node keeps running after pacquet exits, so a detached process that is
/// still connected at that point escaped pacquet's job and would only die
/// with Node.
#[cfg(target_os = "windows")]
#[test]
fn exec_cleans_up_a_detached_process_after_failure_when_node_launches_pnpm() {
    use std::net::TcpListener;

    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let ready_path = workspace.join("detached-ready.txt");
    let exited_path = workspace.join("pnpm-exited.txt");
    let release_path = workspace.join("release-node.txt");
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("listen for detached child");
    listener.set_nonblocking(true).expect("set listener nonblocking");
    let port = listener.local_addr().expect("read listener address").port();
    let detached_script = make_connected_detached_node_script(&ready_path, port);

    let mut node_process = node_launching_pacquet(
        &pacquet,
        &["exec", "node", "-e", &detached_script],
        &linger_after_pacquet_exits(&exited_path, &release_path),
    )
    .spawn()
    .expect("spawn node launching pacquet exec");

    let connection = accept_detached_connection(&listener, &mut node_process);
    let pacquet_exited = wait_for_file(&exited_path);
    if !pacquet_exited {
        let _ = node_process.kill();
        let _ = node_process.wait();
        panic!("pacquet exec did not exit before the deadline");
    }
    assert_connection_closes(connection);

    fs::write(&release_path, "").expect("release node");
    let status = node_process.wait().expect("wait for node launching pacquet exec");
    assert_eq!(status.code(), Some(1), "node must forward the fixture's intentional failure");

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
