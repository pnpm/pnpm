#[cfg(unix)]
use crate::_utils::write_executable;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::{fs, path::Path};
#[cfg(target_os = "windows")]
use std::{thread, time::Duration};

#[test]
fn exec_sets_package_manager_environment() {
    for inherited in [None, Some("/parent/package-manager")] {
        let CommandTempCwd { mut pacquet, root, workspace, .. } = CommandTempCwd::init();
        let expected_execpath =
            fs::canonicalize(pacquet.get_program()).expect("resolve pnpm binary");
        let expected_cwd = fs::canonicalize(&workspace).expect("resolve working directory");
        for name in ["npm_execpath", "npm_node_execpath", "NODE", "INIT_CWD"] {
            match inherited {
                Some(value) => {
                    pacquet.env(name, value);
                }
                None => {
                    pacquet.env_remove(name);
                }
            }
        }
        let output = pacquet
            .args(["exec", "node", "-e", "console.log(JSON.stringify(Object.fromEntries(['npm_execpath', 'npm_node_execpath', 'NODE', 'INIT_CWD'].map(key => [key, process.env[key]]))))"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let env: serde_json::Value =
            serde_json::from_slice(&output).expect("parse child environment");
        let execpath = env["npm_execpath"].as_str().expect("package manager executable path");
        assert_eq!(fs::canonicalize(execpath).expect("resolve child execpath"), expected_execpath);
        let init_cwd = env["INIT_CWD"].as_str().expect("initial working directory");
        assert_eq!(fs::canonicalize(init_cwd).expect("resolve child cwd"), expected_cwd);
        assert_eq!(env["npm_node_execpath"], env["NODE"]);
        let node_path = env["npm_node_execpath"].as_str().expect("node executable path");
        assert!(Path::new(node_path).is_absolute(), "node executable path: {node_path}");
        drop(root);
    }
}

#[cfg(unix)]
#[test]
fn exec_sets_node_environment_with_non_utf8_path() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    let CommandTempCwd { mut pacquet, root, .. } = CommandTempCwd::init();
    let mut path = OsString::from_vec(b"/non-utf8-\xff:".to_vec());
    path.push(std::env::var_os("PATH").expect("PATH"));
    let output = pacquet
        .env("PATH", path)
        .args(["exec", "node", "-e", "console.log(JSON.stringify({ NODE: process.env.NODE, npm_node_execpath: process.env.npm_node_execpath }))"])
        .assert().success().get_output().stdout.clone();
    let env: serde_json::Value = serde_json::from_slice(&output).expect("parse child environment");
    let node_path = env["NODE"].as_str().expect("node executable path");
    assert_eq!(env["NODE"], env["npm_node_execpath"]);
    let expected_node = which::which("node").expect("find node");
    assert_eq!(fs::canonicalize(node_path).unwrap(), fs::canonicalize(expected_node).unwrap());
    drop(root);
}

#[cfg(target_os = "linux")]
#[test]
fn exec_preserves_non_utf8_node_paths() {
    use std::{
        ffi::OsString,
        os::unix::{ffi::OsStringExt, fs::symlink},
    };

    let CommandTempCwd { mut pacquet, root, workspace, .. } = CommandTempCwd::init();
    let bin_dir = workspace.join(OsString::from_vec(b"node-\xff".to_vec()));
    fs::create_dir(&bin_dir).expect("create Node directory");
    symlink(which::which("node").expect("find node"), bin_dir.join("node"))
        .expect("link Node executable");
    let mut path = bin_dir.into_os_string();
    path.push(":");
    path.push(std::env::var_os("PATH").expect("PATH"));
    pacquet
        .env("PATH", path)
        .args(["exec", "/bin/sh", "-c", r#""$NODE" -p 42; "$npm_node_execpath" -p 42"#])
        .assert()
        .success()
        .stdout("42\n42\n");
    drop(root);
}

#[test]
fn exec_clears_inherited_node_environment_without_node_on_path() {
    let CommandTempCwd { mut pacquet, root, workspace, .. } = CommandTempCwd::init();
    let node = which::which("node").expect("find node");
    let empty_path = workspace.join("empty-path");
    fs::create_dir(&empty_path).expect("create empty PATH directory");
    let output = pacquet
        .env("PATH", &empty_path)
        .env("NODE", "/stale/node")
        .env("npm_node_execpath", "/stale/node")
        .arg("exec").arg(node)
        .args(["-e", "console.log(JSON.stringify({ NODE: process.env.NODE, npm_node_execpath: process.env.npm_node_execpath }))"])
        .assert().success().get_output().stdout.clone();
    let env: serde_json::Value = serde_json::from_slice(&output).expect("parse child environment");
    assert_eq!(env, serde_json::json!({}));
    drop(root);
}

#[test]
fn exec_preserves_configured_node_environment() {
    for node_override in ["/configured/node", ""] {
        let CommandTempCwd { mut pacquet, root, workspace, .. } = CommandTempCwd::init();
        fs::write(workspace.join("package.json"), "{}").expect("write manifest");
        let hook = format!(
            "module.exports = {{ hooks: {{ updateConfig(config) {{ config.extraEnv = {{ ...config.extraEnv, NODE: '{node_override}' }}; return config }} }} }}",
        );
        fs::write(workspace.join(".pnpmfile.cjs"), hook).expect("write pnpmfile");
        let output = pacquet
            .args(["exec", "node", "-e", "console.log(JSON.stringify({ NODE: process.env.NODE, npm_node_execpath: process.env.npm_node_execpath }))"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let env: serde_json::Value =
            serde_json::from_slice(&output).expect("parse child environment");
        assert_eq!(env["NODE"], env["npm_node_execpath"]);
        if node_override.is_empty() {
            let node_path = env["NODE"].as_str().expect("node executable path");
            assert!(Path::new(node_path).is_absolute(), "node executable path: {node_path}");
        } else {
            assert_eq!(env["NODE"], node_override);
        }
        drop(root);
    }
}

#[test]
fn exec_does_not_resolve_node_from_project_bin() {
    let CommandTempCwd { mut pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").expect("write manifest");
    let bin_dir = workspace.join("node_modules").join(".bin");
    fs::create_dir_all(&bin_dir).expect("create .bin directory");
    let fake_node = bin_dir.join(if cfg!(windows) { "node.exe" } else { "node" });
    fs::write(
        &fake_node,
        if cfg!(windows) { "@echo off\r\necho fake" } else { "#!/bin/sh\necho fake\n" },
    )
    .expect("write fake node");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fake_node, fs::Permissions::from_mode(0o755))
            .expect("chmod fake node");
    }
    let expected_node = which::which("node").expect("find real node");
    let output = pacquet
        .args([
            "exec",
            expected_node.to_str().unwrap(),
            "-e",
            "console.log(JSON.stringify({ NODE: process.env.NODE, npm_node_execpath: process.env.npm_node_execpath }))",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let env: serde_json::Value = serde_json::from_slice(&output).expect("parse child environment");
    assert_eq!(env["NODE"], env["npm_node_execpath"]);
    let node_path = env["NODE"].as_str().expect("node executable path");
    assert_eq!(fs::canonicalize(node_path).unwrap(), fs::canonicalize(&expected_node).unwrap());
    assert_ne!(fs::canonicalize(node_path).unwrap(), fs::canonicalize(&fake_node).unwrap());
    drop(root);
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

    pacquet
        .with_arg("exec")
        .with_arg("say-hi")
        .assert()
        .success();
    assert!(marker_path.exists(), "the binary in node_modules/.bin should have run");

    drop(root);
}

/// From a plain subdirectory of a project the command runs where the user
/// stands, while the executable comes from the project — pnpm's `exec`
/// takes its working directory from `cliOptions.dir ?? process.cwd()` and
/// its `PATH` from the project.
#[cfg(unix)]
#[test]
fn exec_runs_in_the_cwd_with_the_projects_binaries() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{ "name": "outer" }"#)
        .expect("write package.json");
    let bin_dir = workspace.join("node_modules").join(".bin");
    fs::create_dir_all(&bin_dir).expect("create node_modules/.bin");
    let marker_path = workspace.join("cwd.txt");
    write_executable(
        &bin_dir.join("record-cwd"),
        &format!("#!/bin/sh\npwd > \"{}\"\n", marker_path.display()),
    );
    let subdir = workspace.join("src/utils");
    fs::create_dir_all(&subdir).expect("create the subdirectory");

    pacquet
        .with_current_dir(&subdir)
        .with_args(["exec", "record-cwd"])
        .assert()
        .success();

    let recorded = fs::read_to_string(&marker_path).expect("read the recorded cwd");
    assert_eq!(
        dunce::canonicalize(recorded.trim()).expect("canonicalize the recorded cwd"),
        dunce::canonicalize(&subdir).expect("canonicalize the subdirectory"),
    );

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

    pacquet
        .with_arg("exec")
        .with_arg("write-arg")
        .with_arg("hello-world")
        .assert()
        .success();
    let written = fs::read_to_string(&marker_path).expect("read marker");
    assert_eq!(written, "hello-world");

    drop(root);
}

/// `pacquet exec` with no command is an error, mirroring pnpm's
/// `ERR_PNPM_EXEC_MISSING_COMMAND`.
#[test]
fn exec_errors_when_no_command_given() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet
        .with_arg("exec")
        .output()
        .expect("spawn pacquet exec");
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

/// A non-recursive `exec` runs where pacquet was invoked, so the inherited
/// `PWD` already names the command's cwd and may hold the logical path
/// through a symlink: keep it rather than stamping the canonicalized cwd
/// over it ([pnpm/pnpm#1550](https://github.com/pnpm/pnpm/issues/1550)).
#[test]
#[cfg(unix)]
fn exec_keeps_inherited_pwd_when_running_in_the_invocation_dir() {
    let CommandTempCwd { mut pacquet, root, workspace, .. } = CommandTempCwd::init();
    let linked = root.path().join("linked");
    std::os::unix::fs::symlink(&workspace, &linked).expect("symlink the workspace");

    pacquet.current_dir(&linked);
    pacquet.env("PWD", &linked);
    pacquet
        .with_arg("exec")
        .with_arg("node")
        .with_arg("-e")
        .with_arg("require('fs').writeFileSync('pwd.txt', process.env.PWD)")
        .assert()
        .success();

    let pwd = fs::read_to_string(workspace.join("pwd.txt")).expect("read recorded PWD");
    assert_eq!(pwd, linked.to_string_lossy().as_ref(), "the inherited logical PWD should survive");

    drop(root);
}

/// Set on the test binary when [`exec_output_ends_while_a_detached_grandchild_runs`]
/// runs it under `pnpm exec`. The value is the file to write the waiter's PID to.
#[cfg(target_os = "windows")]
const WAITER_PID_ENV: &str = "PNPM_TEST_DETACHED_WAITER_PID";

/// A shell reading pnpm's redirected output sees it end when pnpm exits,
/// even if a detached grandchild is still running (pnpm/pnpm#16930).
///
/// The test binary runs itself under `pnpm exec` and starts the grandchild
/// the way Node.js starts a detached child: its own standard handles are
/// non-inheritable, and the grandchild inherits every other inheritable
/// handle. Windows PowerShell and some Node.js versions cannot stand in,
/// as they pass inheritable copies of their own output along.
#[cfg(target_os = "windows")]
#[test]
fn exec_output_ends_while_a_detached_grandchild_runs() {
    use std::{io::Read, process::Stdio, sync::mpsc};

    if let Some(waiter_pid_path) = std::env::var_os(WAITER_PID_ENV) {
        start_detached_waiter(Path::new(&waiter_pid_path));
        return;
    }

    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let waiter_pid_path = workspace.join("waiter-pid.txt");
    let test_binary = std::env::current_exe().expect("locate the test binary");

    let mut pacquet_process = pacquet
        .with_env(WAITER_PID_ENV, &waiter_pid_path)
        .with_arg("exec")
        .with_arg(test_binary)
        .with_args(["exec::exec_output_ends_while_a_detached_grandchild_runs", "--exact"])
        .with_stdin(Stdio::null())
        .with_stdout(Stdio::piped())
        .with_stderr(Stdio::null())
        .spawn()
        .expect("spawn pacquet exec");
    let mut stdout = pacquet_process.stdout.take().expect("pacquet stdout is piped");
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut output = String::new();
        let _ = stdout.read_to_string(&mut output);
        let _ = sender.send(output);
    });
    let status = pacquet_process.wait().expect("wait for pacquet exec");
    let output = receiver.recv_timeout(Duration::from_secs(20));

    if let Ok(pid) = fs::read_to_string(&waiter_pid_path) {
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/PID", pid.trim()])
            .output();
    }
    assert_eq!(status.code(), Some(0), "the detached spawn should succeed: {output:?}");
    assert!(waiter_pid_path.exists(), "the detached waiter should have started");
    assert!(
        output.is_ok(),
        "pnpm's output should end when pnpm exits, not when the detached grandchild does",
    );

    drop(root);
}

/// The side of [`exec_output_ends_while_a_detached_grandchild_runs`] that
/// runs under `pnpm exec`.
#[cfg(target_os = "windows")]
fn start_detached_waiter(waiter_pid_path: &Path) {
    use std::{os::windows::process::CommandExt, process::Stdio};
    use windows_sys::Win32::{
        Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation},
        System::{
            Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE},
            Threading::{CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS},
        },
    };

    // SAFETY: `GetStdHandle` returns a borrowed handle, and
    // `SetHandleInformation` only changes its inherit flag.
    unsafe {
        for stream in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            SetHandleInformation(GetStdHandle(stream), HANDLE_FLAG_INHERIT, 0);
        }
    }
    let waiter_pid = std::process::Command::new("node")
        .args(["-e", "setTimeout(() => {}, 60000)"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .expect("spawn the detached waiter")
        .id();
    fs::write(waiter_pid_path, waiter_pid.to_string()).expect("write the waiter's PID");
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

/// Regression test for
/// [pnpm/pnpm#3604](https://github.com/pnpm/pnpm/issues/3604): the extra bin
/// paths carry the workspace root's executables to every member, and a
/// configured `modulesDir` moves them off `node_modules/.bin`.
#[cfg(unix)]
#[test]
fn exec_resolves_a_workspace_root_command_from_the_configured_modules_dir() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - \"packages/*\"\nmodulesDir: vendor\n",
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(workspace.join("package.json"), r#"{ "name": "wsroot", "version": "1.0.0" }"#)
        .expect("write workspace-root package.json");
    let member = workspace.join("packages/foo");
    fs::create_dir_all(&member).expect("create the member dir");
    fs::write(member.join("package.json"), r#"{ "name": "foo", "version": "1.0.0" }"#)
        .expect("write member package.json");

    let bin_dir = workspace.join("vendor").join(".bin");
    fs::create_dir_all(&bin_dir).expect("create the workspace-root bin dir");
    write_executable(&bin_dir.join("greet"), "#!/bin/sh\necho configured\n");

    let output = pacquet
        .with_current_dir(&member)
        .with_args(["exec", "greet"])
        .output()
        .expect("run pacquet exec greet in the member");
    dbg!(&output);
    assert!(output.status.success(), "pacquet exec greet should succeed in the member");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("configured"),
        "the workspace root's configured bin dir must be on PATH",
    );

    drop(root);
}

/// The entry `pnpm exec` prepends for the project itself, which the
/// workspace-root case above reaches through `extraBinPaths` instead. A
/// leftover `node_modules/.bin` must not win.
#[cfg(unix)]
#[test]
fn exec_runs_a_project_command_from_the_configured_modules_dir() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "modulesDir: vendor\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(workspace.join("package.json"), r#"{ "name": "root-pkg", "version": "1.0.0" }"#)
        .expect("write package.json");

    for (modules_dir, marker) in [("vendor", "configured"), ("node_modules", "stale")] {
        let bin_dir = workspace.join(modules_dir).join(".bin");
        fs::create_dir_all(&bin_dir).expect("create the bin dir");
        write_executable(&bin_dir.join("greet"), &format!("#!/bin/sh\necho {marker}\n"));
    }

    let output = pacquet
        .with_args(["exec", "greet"])
        .output()
        .expect("run pacquet exec greet");
    dbg!(&output);
    assert!(output.status.success(), "pacquet exec greet should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("configured"), "the configured modules dir must win: {stdout}");
    assert!(!stdout.contains("stale"), "node_modules/.bin must not win: {stdout}");

    drop(root);
}
