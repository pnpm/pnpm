use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::fs;

#[cfg(unix)]
fn write_executable(path: &std::path::Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, body).expect("write executable");
    let mut perms = fs::metadata(path).expect("stat executable").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).expect("chmod executable");
}

/// `pacquet run <script>` looks up the named entry under
/// `scripts` in the workspace's `package.json` and spawns it via
/// the executor. A successful invocation should produce the side
/// effect declared by the script (here, creating a marker file)
/// and exit 0.
#[cfg(unix)]
#[test]
fn run_executes_declared_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest_path = workspace.join("package.json");
    let marker_path = workspace.join("marker.txt");
    // Path is double-quoted in the shell command so a tempdir
    // path containing a space (rare on Linux, common on macOS
    // under `/var/folders/...`) doesn't get split into two
    // `touch` arguments.
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": {
            "touch-marker": format!(r#"touch "{}""#, marker_path.display()),
        },
    })
    .to_string();
    fs::write(&manifest_path, manifest).expect("write package.json");

    pacquet.with_arg("run").with_arg("touch-marker").assert().success();
    assert!(marker_path.exists(), "script should have created the marker file");

    drop(root);
}

/// Positional arguments after the script name flow through to the
/// spawned shell verbatim, joined by spaces. Mirrors
/// `pnpm run <script> -- <args>` minus the npm `--` separator
/// (pacquet does not require it).
#[cfg(unix)]
#[test]
fn run_passes_extra_arguments_to_the_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest_path = workspace.join("package.json");
    let marker_path = workspace.join("args.txt");
    // `printf %s "$1"` writes the first argument into the marker,
    // letting the assertion below pin the exact argument flow.
    // Inner sh redirect quotes the temp path so a space in the
    // path doesn't split the redirect target. Outer single
    // quotes wrap the inner command; the embedded double quote
    // around `{}` survives because it's inside the outer single
    // quotes.
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": {
            "echo-args": format!(
                r#"sh -c 'printf %s "$1" > "{}"' --"#,
                marker_path.display(),
            ),
        },
    })
    .to_string();
    fs::write(&manifest_path, manifest).expect("write package.json");

    pacquet.with_arg("run").with_arg("echo-args").with_arg("hello-world").assert().success();
    let written = fs::read_to_string(&marker_path).expect("read marker");
    assert_eq!(written, "hello-world");

    drop(root);
}

/// Without `--if-present`, calling a script that does not exist fails
/// with pnpm's `NO_SCRIPT` error.
#[test]
fn run_errors_on_missing_script_without_if_present() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest_path = workspace.join("package.json");
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": { "build": "echo built" },
    })
    .to_string();
    fs::write(&manifest_path, manifest).expect("write package.json");

    let output =
        pacquet.with_arg("run").with_arg("nonexistent").output().expect("spawn pacquet run");
    assert!(!output.status.success(), "missing script must surface as a failure");

    drop(root);
}

/// `pnpm run start` with no `start` script and no `server.js` file fails
/// with `NO_SCRIPT_OR_SERVER`, matching pnpm's runLifecycleHook guard. (A
/// bare `node server.js` fallback would instead surface node's
/// "Cannot find module" error, so the assertion pins the pnpm message.)
#[test]
fn run_start_without_script_or_server_errors() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": { "build": "echo built" },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    let output = pacquet.with_arg("run").with_arg("start").output().expect("spawn pacquet run");
    assert!(!output.status.success(), "run start without script or server.js must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_NO_SCRIPT_OR_SERVER")
            || stderr.contains("Missing script start or file server.js"),
        "should surface NO_SCRIPT_OR_SERVER:\n{stderr}",
    );

    drop(root);
}

/// An empty `start` script (`"start": ""`) is falsy in pnpm
/// (`!m.scripts.start`), so it falls back to the `node server.js` path
/// like a missing one — and with no `server.js` it must raise
/// `NO_SCRIPT_OR_SERVER` rather than silently exit 0.
#[test]
fn run_empty_start_script_hits_server_js_guard() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": { "start": "" },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    let output = pacquet.with_arg("run").with_arg("start").output().expect("spawn pacquet run");
    assert!(!output.status.success(), "empty start without server.js must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_NO_SCRIPT_OR_SERVER")
            || stderr.contains("Missing script start or file server.js"),
        "should surface NO_SCRIPT_OR_SERVER:\n{stderr}",
    );

    drop(root);
}

/// With `--if-present`, the same missing script becomes a no-op
/// and pacquet exits cleanly. Required for orchestration tools
/// that probe optional scripts without wanting to fail the
/// pipeline.
#[test]
fn run_with_if_present_is_a_noop_for_missing_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest_path = workspace.join("package.json");
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": { "build": "echo built" },
    })
    .to_string();
    fs::write(&manifest_path, manifest).expect("write package.json");

    pacquet.with_arg("run").with_arg("--if-present").with_arg("nonexistent").assert().success();

    drop(root);
}

/// `pnpm run` with no script name lists the available scripts, grouped
/// into lifecycle scripts and others. Mirrors pnpm's `printProjectCommands`.
#[test]
fn run_lists_scripts_when_no_name_given() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": { "build": "echo built", "test": "echo tested" },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    let output = pacquet.with_arg("run").output().expect("spawn pacquet run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("STDOUT:\n{stdout}\n");
    assert!(output.status.success(), "listing scripts should succeed");
    assert!(stdout.contains("Commands available via"), "should list non-lifecycle scripts");
    assert!(stdout.contains("build"), "should list the build script");
    assert!(stdout.contains("Lifecycle scripts:"), "should group lifecycle scripts");

    drop(root);
}

/// With `enablePrePostScripts`, `pnpm run <name>` also runs `pre<name>`
/// and `post<name>`. Driven here through the `PNPM_CONFIG_*` env overlay.
#[cfg(unix)]
#[test]
fn run_runs_pre_and_post_when_enabled() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let pre = workspace.join("pre.txt");
    let main = workspace.join("main.txt");
    let post = workspace.join("post.txt");
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": {
            "prebuild": format!(r#"touch "{}""#, pre.display()),
            "build": format!(r#"touch "{}""#, main.display()),
            "postbuild": format!(r#"touch "{}""#, post.display()),
        },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    pacquet
        .with_env("PNPM_CONFIG_ENABLE_PRE_POST_SCRIPTS", "true")
        .with_arg("run")
        .with_arg("build")
        .assert()
        .success();

    assert!(pre.exists(), "prebuild should have run");
    assert!(main.exists(), "build should have run");
    assert!(post.exists(), "postbuild should have run");

    drop(root);
}

/// A failing script's exit code becomes pacquet's exit code.
#[cfg_attr(target_os = "windows", ignore = "uses a POSIX shell `exit` builtin")]
#[test]
fn run_propagates_failing_script_exit_code() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": { "fail": "exit 5" },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    let output = pacquet.with_arg("run").with_arg("fail").output().expect("spawn pacquet run");
    assert_eq!(output.status.code(), Some(5), "the script's exit code must propagate");

    drop(root);
}

/// A script body with embedded quotes reaches the child untouched. On
/// Windows the default `cmd /d /s /c` path is `windows_verbatim_args`, so
/// the script must be appended with `raw_arg`; a plain `arg` would escape
/// the inner quotes and break `node -e "..."`. Runs everywhere (it is a
/// no-op risk on POSIX) but is load-bearing on Windows CI.
#[test]
fn run_preserves_embedded_quotes_in_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": { "say": r#"node -e "process.stdout.write('verbatim-ok')""# },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    let output = pacquet.with_arg("run").with_arg("say").output().expect("spawn pacquet run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "the script must exit 0, got: {output:?}");
    assert!(stdout.contains("verbatim-ok"), "embedded quotes must survive; stdout: {stdout:?}");

    drop(root);
}

/// A failing `test` script prints pnpm's stage-specific lifecycle error
/// (`Test failed. See above for more details.`) rather than the generic
/// exit-code line, matching reportLifecycleError's `test` special case.
#[cfg_attr(target_os = "windows", ignore = "uses a POSIX shell `exit` builtin")]
#[test]
fn run_failing_test_script_prints_test_failed_message() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": { "test": "exit 1" },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    let output = pacquet.with_arg("run").with_arg("test").output().expect("spawn pacquet run");
    assert_eq!(output.status.code(), Some(1), "the script's exit code must propagate");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Test failed. See above for more details."),
        "test-stage failure should print pnpm's test message:\n{stderr}",
    );

    drop(root);
}

/// A script that invokes a locally-installed binary resolves it through
/// `node_modules/.bin`, which `pnpm run` prepends to `PATH`.
#[cfg(unix)]
#[test]
fn run_finds_local_bin_on_path() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let bin_dir = workspace.join("node_modules").join(".bin");
    fs::create_dir_all(&bin_dir).expect("create node_modules/.bin");
    let marker = workspace.join("marker.txt");
    write_executable(
        &bin_dir.join("say-hi"),
        &format!("#!/bin/sh\ntouch \"{}\"\n", marker.display()),
    );
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": { "hi": "say-hi" },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    pacquet.with_arg("run").with_arg("hi").assert().success();
    assert!(marker.exists(), "the local bin should be resolved via node_modules/.bin");

    drop(root);
}

/// With a non-silent reporter (e.g. `--reporter=ndjson`), `pacquet run`
/// echoes `$ <script>` to stderr before spawning the script —
/// matching pnpm's `runLifecycleHook.ts:110`
/// (`process.stderr.write(chalk.dim($ ${...})...)`). The default Silent
/// reporter (no human-facing reporter exists in pacquet yet) suppresses
/// it.
#[cfg(unix)]
#[test]
fn run_echoes_script_to_stderr_when_reporter_not_silent() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": { "build": "true" },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    let output = pacquet
        .with_arg("--reporter=ndjson")
        .with_arg("run")
        .with_arg("build")
        .output()
        .expect("spawn pacquet run");
    assert!(output.status.success(), "the script should succeed");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("$ true"),
        "stderr should echo the script body with a `$ ` prefix:\n{stderr}",
    );

    drop(root);
}

/// `pacquet run start` with no `start` script and a `server.js` file
/// SUCCEEDS via the `node server.js` fallback. The fallback resolves
/// `node` against the inherited `PATH`, so the test prepends a fake
/// `node` shim (a shell script that writes a marker) to `PATH` and
/// verifies it was invoked with `server.js`. Mirrors the success side
/// of pnpm's `runLifecycleHook.ts:75-83` start-fallback.
#[cfg(unix)]
#[test]
fn run_start_falls_back_to_node_server_js_when_present() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": { "build": "echo built" },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
    // `server.js` is probed via `existsSync('server.js')`, resolved
    // against the process cwd (which `CommandTempCwd` sets to the
    // workspace), so the file goes here, not under any project subdir.
    fs::write(workspace.join("server.js"), "// placeholder").expect("write server.js");

    let shim_dir = workspace.join("shim");
    fs::create_dir_all(&shim_dir).expect("create shim dir");
    let marker = workspace.join("node-args.txt");
    // Shim writes its argv to the marker, letting the assertion pin
    // the exact `node server.js` invocation without needing real
    // node on PATH.
    write_executable(
        &shim_dir.join("node"),
        &format!("#!/bin/sh\nprintf %s \"$*\" > \"{}\"\n", marker.display()),
    );

    let existing_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", shim_dir.display(), existing_path);
    pacquet.with_env("PATH", new_path).with_arg("run").with_arg("start").assert().success();

    let written = fs::read_to_string(&marker).expect("read marker");
    assert_eq!(written, "server.js");

    drop(root);
}
