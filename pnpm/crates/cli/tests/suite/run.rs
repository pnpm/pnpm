use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::{
    fs,
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

/// `pnpm run <script> -- <args>` forwards the separator, so an argument
/// shaped like an option of the script's own program reaches the script
/// instead of being claimed by it (pnpm/pnpm#13295). Asserts what the
/// script received rather than the echoed command line, and mirrors the
/// TypeScript counterpart in `pnpm11/pnpm/test/run.ts` ("run: pass the
/// args to the command that is specified in the build script"), down to
/// only the main stage seeing the arguments.
#[test]
fn run_forwards_the_separator_and_option_shaped_arguments() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    // A recorder file rather than an inlined `node -e` program, so no
    // path has to survive quoting into a JS string literal on either
    // platform. Same approach as the TypeScript test.
    fs::write(
        workspace.join("recordArgs.js"),
        "require('fs').writeFileSync('args.json', \
JSON.stringify(require('./args.json').concat([process.argv.slice(2)])), 'utf8')",
    )
    .expect("write recordArgs.js");
    fs::write(workspace.join("args.json"), "[]").expect("seed args.json");
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "test",
            "version": "0.0.0",
            "scripts": {
                "prefoo": "node recordArgs",
                "foo": "node recordArgs",
                "postfoo": "node recordArgs",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet
        .with_args([
            "--config.enable-pre-post-scripts",
            "run",
            "foo",
            // A pnpm-settings-shaped token before the separator: the
            // pre-clap config pass must leave it for the script too
            // (pnpm/pnpm#13302), which `--other=1` alone would not catch.
            "--config.foo=bar",
            "arg",
            "--",
            "--other=1",
            "-s",
            "--if-present",
        ])
        .assert()
        .success();

    let recorded: Vec<Vec<String>> =
        serde_json::from_str(&fs::read_to_string(workspace.join("args.json")).expect("read args"))
            .expect("parse args.json");
    assert_eq!(
        recorded,
        vec![
            Vec::<String>::new(),
            vec![
                "--config.foo=bar".to_string(),
                "arg".to_string(),
                "--".to_string(),
                "--other=1".to_string(),
                "-s".to_string(),
                "--if-present".to_string(),
            ],
            Vec::<String>::new(),
        ],
    );

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

/// pnpm also accepts `--if-present` ahead of the script name
/// (`pnpm --if-present <script>`), where the script dispatches through
/// the shorthand fallback instead of an explicit `run`. The missing
/// script must be the same clean no-op — not an exec fallback error.
#[test]
fn top_level_if_present_is_a_noop_for_missing_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": { "build": "echo built" },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    pacquet.with_arg("--if-present").with_arg("nonexistent").assert().success();

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

#[test]
fn run_preserves_parent_tmpdir() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let alternate_tmpdir = workspace.join("project-tmp");
    fs::create_dir(&alternate_tmpdir).expect("create alternate temp dir");
    fs::write(
        workspace.join("show-tmp.js"),
        "require('fs').writeFileSync('tmpdir.json', JSON.stringify({ \
env: process.env.TMPDIR, os: require('os').tmpdir() }))",
    )
    .expect("write show-tmp.js");
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "test",
            "version": "0.0.0",
            "scripts": { "show-tmp": "node show-tmp.js" },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet
        .with_env("TMPDIR", &alternate_tmpdir)
        .with_arg("run")
        .with_arg("show-tmp")
        .assert()
        .success();

    let recorded: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(workspace.join("tmpdir.json")).expect("read tmpdir.json"),
    )
    .expect("parse tmpdir.json");
    let expected_tmpdir = alternate_tmpdir.to_string_lossy();
    assert_eq!(recorded["env"], expected_tmpdir.as_ref());
    if cfg!(not(windows)) {
        assert_eq!(recorded["os"], expected_tmpdir.as_ref());
    }

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

/// Running a script from a workspace member resolves binaries from the
/// workspace root's `node_modules/.bin` — pnpm puts it on PATH via
/// `extraBinPaths`, so root-level dev tools are callable from every
/// workspace project.
#[cfg(unix)]
#[test]
fn run_finds_workspace_root_bin_on_path() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - project\n")
        .expect("write pnpm-workspace.yaml");
    let bin_dir = workspace.join("node_modules").join(".bin");
    fs::create_dir_all(&bin_dir).expect("create workspace-root node_modules/.bin");
    write_executable(&bin_dir.join("root-tool"), "#!/bin/sh\ntouch root-tool-ran.txt\n");
    let project = workspace.join("project");
    fs::create_dir_all(&project).expect("create project dir");
    let manifest = json!({
        "name": "project",
        "version": "0.0.0",
        "scripts": { "build": "root-tool" },
    })
    .to_string();
    fs::write(project.join("package.json"), manifest).expect("write package.json");

    std::process::Command::cargo_bin("pnpm")
        .expect("find pacquet binary")
        .with_current_dir(&project)
        .with_arg("run")
        .with_arg("build")
        .assert()
        .success();
    assert!(
        project.join("root-tool-ran.txt").exists(),
        "the workspace root's node_modules/.bin should be on the script's PATH",
    );

    drop(root);
}

#[cfg(unix)]
#[test]
fn top_level_fallback_runs_script_before_local_bin() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let bin_dir = workspace.join("node_modules").join(".bin");
    fs::create_dir_all(&bin_dir).expect("create node_modules/.bin");
    let marker = workspace.join("source.txt");
    write_executable(
        &bin_dir.join("commitlint"),
        &format!("#!/bin/sh\nprintf bin > \"{}\"\n", marker.display()),
    );
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": {
            "commitlint": format!(r#"printf script > "{}""#, marker.display()),
        },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    pacquet.with_arg("commitlint").assert().success();
    assert_eq!(fs::read_to_string(&marker).expect("read marker"), "script");

    drop(root);
}

#[test]
fn top_level_fallback_runs_package_yaml_script_with_dir() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let fixtures = workspace.join("fixtures");
    fs::create_dir_all(&fixtures).expect("create fixtures dir");
    let marker = workspace.join("prepared.txt");
    fs::write(
        fixtures.join("package.yaml"),
        r#"name: fixtures
version: 0.0.0
scripts:
  prepareFixtures: node -e "require('fs').writeFileSync(process.env.MARKER_PATH, 'prepared')"
"#,
    )
    .expect("write package.yaml");

    pacquet
        .with_env("MARKER_PATH", marker.to_string_lossy().as_ref())
        .with_arg("--dir")
        .with_arg(&fixtures)
        .with_arg("prepareFixtures")
        .assert()
        .success();
    assert_eq!(fs::read_to_string(&marker).expect("read marker"), "prepared");

    drop(root);
}

/// npm's `--prefix` is accepted as a spelling of `--dir`
/// (<https://github.com/pnpm/pnpm/issues/13583>) — ahead of the
/// subcommand, where pnpm's own options live. Past the script name the
/// same token is the script's, so the script records what it received.
#[test]
fn prefix_selects_the_dir_before_the_subcommand_and_is_the_script_s_after_it() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let project = workspace.join("project");
    fs::create_dir_all(&project).expect("create project dir");
    let marker = workspace.join("ran.txt");
    fs::write(
        project.join("package.json"),
        json!({
            "name": "project",
            "version": "0.0.0",
            // The `--` keeps node from claiming a forwarded `--prefix` as
            // one of its own options.
            "scripts": {
                "test": r#"node -e "require('fs').writeFileSync(process.env.MARKER_PATH, process.argv.slice(1).join(' '))" --"#,
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet
        .with_env("MARKER_PATH", marker.to_string_lossy().as_ref())
        .with_arg("--prefix")
        .with_arg(&project)
        .with_arg("run")
        .with_arg("test")
        .with_arg("--prefix")
        .with_arg("forwarded")
        .assert()
        .success();
    assert_eq!(fs::read_to_string(&marker).expect("read marker"), "--prefix forwarded");

    drop(root);
}

#[cfg(unix)]
#[test]
fn top_level_fallback_runs_local_bin_when_script_is_missing() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let bin_dir = workspace.join("node_modules").join(".bin");
    fs::create_dir_all(&bin_dir).expect("create node_modules/.bin");
    let marker = workspace.join("args.txt");
    write_executable(
        &bin_dir.join("commitlint"),
        &format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\n", marker.display()),
    );
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": {},
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    pacquet
        .with_args(["commitlint", "--edit", "--config=commitlint.config.cjs"])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(&marker).expect("read marker"),
        "--edit\n--config=commitlint.config.cjs\n",
    );

    drop(root);
}

#[cfg(unix)]
#[test]
fn top_level_fallback_runs_local_bin_without_package_json() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let bin_dir = workspace.join("node_modules").join(".bin");
    fs::create_dir_all(&bin_dir).expect("create node_modules/.bin");
    let marker = workspace.join("args.txt");
    write_executable(
        &bin_dir.join("commitlint"),
        &format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\n", marker.display()),
    );

    pacquet.with_args(["commitlint", "--edit", "COMMIT_EDITMSG"]).assert().success();
    assert_eq!(fs::read_to_string(&marker).expect("read marker"), "--edit\nCOMMIT_EDITMSG\n");

    drop(root);
}

#[cfg(unix)]
#[test]
fn top_level_fallback_forwards_dotted_config_args_to_local_bin() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let bin_dir = workspace.join("node_modules").join(".bin");
    fs::create_dir_all(&bin_dir).expect("create node_modules/.bin");
    let marker = workspace.join("args.txt");
    write_executable(
        &bin_dir.join("commitlint"),
        &format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\n", marker.display()),
    );

    pacquet.with_args(["commitlint", "--config.foo=bar"]).assert().success();
    assert_eq!(fs::read_to_string(&marker).expect("read marker"), "--config.foo=bar\n");

    drop(root);
}

/// A mistyped top-level command falling back to `run` in a directory
/// without a manifest must surface the fallback's own missing-command
/// error, and the verify-deps-before-run gate (here on its `install`
/// action) must skip the directory rather than spawn an install that
/// has no manifest to work with. Mirrors the TypeScript regression
/// tests in `pnpm11/deps/status/test/checkDepsStatus.test.ts`
/// ("missing workspace state").
#[test]
fn top_level_fallback_without_manifest_does_not_attempt_an_install() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();

    let output = pacquet
        .with_env("pnpm_config_verify_deps_before_run", "install")
        .with_args(["witch-definitely-not-a-binary", "10", "login"])
        .output()
        .expect("spawn pacquet");
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDERR:\n{stderr}\n");
    assert!(!output.status.success(), "a mistyped command must fail");
    assert!(
        stderr.contains("witch-definitely-not-a-binary") && stderr.contains("not found"),
        "the failure must name the missing command, not come from a spawned install:\n{stderr}",
    );
    // `package.json` is included because an install in a manifest-less
    // directory would scaffold one before doing anything else.
    for side_effect in ["node_modules", "pnpm-lock.yaml", "package.json"] {
        assert!(
            !workspace.join(side_effect).exists(),
            "no install may run in a directory without a manifest, but {side_effect} appeared",
        );
    }

    drop(root);
}

/// With a non-silent reporter (the default, or e.g. `--reporter=ndjson`),
/// `pacquet run` echoes `$ <script>` to stderr before spawning the script —
/// matching pnpm's `runLifecycleHook.ts:110`
/// (`process.stderr.write(chalk.dim($ ${...})...)`). Only `--reporter=silent`
/// suppresses it.
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
    // `server.js` is probed in the same directory where the script
    // runs, which `CommandTempCwd` sets to the workspace for this case.
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

#[cfg(unix)]
#[test]
fn run_start_fallback_uses_dir_for_server_js_probe() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let project = workspace.join("project");
    fs::create_dir_all(&project).expect("create project");
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
    })
    .to_string();
    fs::write(project.join("package.json"), manifest).expect("write package.json");
    fs::write(project.join("server.js"), "// placeholder").expect("write server.js");

    let shim_dir = workspace.join("shim");
    fs::create_dir_all(&shim_dir).expect("create shim dir");
    let marker = workspace.join("node-cwd-and-args.txt");
    write_executable(
        &shim_dir.join("node"),
        &format!("#!/bin/sh\nprintf '%s\\n%s' \"$(pwd)\" \"$*\" > \"{}\"\n", marker.display()),
    );

    let existing_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", shim_dir.display(), existing_path);
    std::process::Command::cargo_bin("pnpm")
        .expect("find pacquet binary")
        .with_current_dir(&workspace)
        .with_env("PATH", new_path)
        .with_arg("--dir")
        .with_arg(&project)
        .with_arg("start")
        .assert()
        .success();

    let written = fs::read_to_string(&marker).expect("read marker");
    let project = fs::canonicalize(project).expect("canonicalize project");
    assert_eq!(written, format!("{}\nserver.js", project.display()));

    drop(root);
}

/// `pnpm test` / `start` / `stop` name no command in pnpm — they reach
/// `run` through the `pnpm <script>` fallback, so every token after the
/// command name is the script's, a `--` separator and anything shaped
/// like a pnpm flag included.
#[cfg(unix)]
#[test]
fn script_shortcuts_forward_every_argument_to_the_script() {
    for (command, arguments, expected) in [
        ("test", &["--flag", "value"][..], "--flag value"),
        ("test", &["--", "--flag"][..], "-- --flag"),
        // `--if-present` and `-s` are pnpm's own flags on other commands;
        // for a shortcut they belong to the script.
        ("start", &["--if-present"][..], "--if-present"),
        ("stop", &["-s"][..], "-s"),
        ("stop", &["--", "--flag", "x"][..], "-- --flag x"),
    ] {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        let marker = workspace.join("args.txt");
        write_executable(
            &workspace.join("record-args"),
            &format!("#!/bin/sh\nprintf '%s' \"$*\" > \"{}\"\n", marker.display()),
        );
        let manifest = json!({
            "name": "test",
            "version": "0.0.0",
            "scripts": { command: "./record-args" },
        })
        .to_string();
        fs::write(workspace.join("package.json"), manifest).expect("write package.json");

        pacquet.with_arg(command).with_args(arguments).assert().success();

        let written = fs::read_to_string(&marker).expect("read marker");
        assert_eq!(written, expected, "command: {command} {arguments:?}");

        drop(root);
    }
}

/// A `/pattern/` positional selects every matching script rather than
/// naming one, through both `pnpm run <selector>` and the bare
/// `pnpm <selector>` fallback.
#[cfg(unix)]
#[test]
fn run_executes_every_script_matching_a_regexp_selector() {
    for prefix in [&["run"][..], &[][..]] {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        let manifest = json!({
            "name": "test",
            "version": "0.0.0",
            "scripts": {
                "typecheck:one": format!(r#"touch "{}""#, workspace.join("one.txt").display()),
                "typecheck:two": format!(r#"touch "{}""#, workspace.join("two.txt").display()),
                "build": format!(r#"touch "{}""#, workspace.join("build.txt").display()),
            },
        })
        .to_string();
        fs::write(workspace.join("package.json"), manifest).expect("write package.json");

        pacquet.with_args(prefix).with_arg("/^typecheck:.+/").assert().success();

        assert!(workspace.join("one.txt").exists(), "prefix: {prefix:?}");
        assert!(workspace.join("two.txt").exists(), "prefix: {prefix:?}");
        assert!(!workspace.join("build.txt").exists(), "prefix: {prefix:?}");

        drop(root);
    }
}

#[test]
fn regexp_selected_scripts_run_concurrently_by_default() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("track-concurrency.js"),
        r"const fs = require('fs')
const [self, other] = process.argv.slice(2)
const marker = `active-${self}`
fs.writeFileSync(marker, '')
const started = Date.now()
const check = setInterval(() => {
  if (fs.existsSync(`active-${other}`)) {
    fs.writeFileSync('saw-parallel', '')
    finish()
  } else if (Date.now() - started > 1000) {
    finish()
  }
}, 10)
function finish () {
  clearInterval(check)
  fs.rmSync(marker, { force: true })
  console.log(self)
}
",
    )
    .expect("write concurrency probe");
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "test",
            "version": "0.0.0",
            "scripts": {
                "dev:one": "node track-concurrency.js one two",
                "dev:two": "node track-concurrency.js two one",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    let output = pacquet
        .with_args(["--workspace-concurrency=2", "run", "/^dev:/"])
        .assert()
        .success()
        .get_output()
        .clone();

    assert!(workspace.join("saw-parallel").exists(), "the selected scripts should overlap");
    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("STDOUT:\n{stdout}\n");
    assert!(stdout.contains("dev:one: one"));
    assert!(stdout.contains("dev:two: two"));

    drop(root);
}

#[test]
fn regexp_selected_scripts_cancel_siblings_after_failure() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "test",
            "version": "0.0.0",
            "scripts": {
                "dev:slow": r#"node -e "require('fs').writeFileSync('slow-started', ''); setTimeout(() => {}, 5000)""#,
                "dev:fail": r#"node -e "const fs = require('fs'); const wait = () => fs.existsSync('slow-started') ? process.exit(1) : setTimeout(wait, 10); wait()""#,
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    let start = Instant::now();
    pacquet.with_args(["--workspace-concurrency=2", "run", "/^dev:/"]).assert().failure();
    assert!(
        start.elapsed() < Duration::from_secs(4),
        "a failed script should cancel its in-flight sibling",
    );

    drop(root);
}

/// Flags on a selector say nothing about which scripts to pick, so pnpm
/// rejects them instead of honouring a subset.
#[cfg(unix)]
#[test]
fn run_rejects_regexp_flags_in_a_selector() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": { "build": "true" },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    let output = pacquet.with_args(["run", "/^BUILD/i"]).assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_UNSUPPORTED_SCRIPT_COMMAND_FORMAT"),
        "should reject the flags:\n{stderr}",
    );

    drop(root);
}

/// With `preferSymlinkedExecutables`, symlinked bins have no shim to
/// carry a `NODE_PATH` block, so the config exports one pointing at
/// the virtual store's hidden `node_modules` — pnpm's
/// `pnpm run with preferSymlinkedExecutables true` test.
#[test]
#[cfg_attr(target_os = "windows", ignore = "preferSymlinkedExecutables is inert on Windows")]
fn run_exports_node_path_when_prefer_symlinked_executables() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker_path = workspace.join("node-path.txt");
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": {
            "build": format!(r#"sh -c 'printf %s "$NODE_PATH" > "{}"'"#, marker_path.display()),
        },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "preferSymlinkedExecutables: true\n")
        .expect("write pnpm-workspace.yaml");

    pacquet.with_args(["run", "build"]).assert().success();
    let node_path = fs::read_to_string(&marker_path).expect("read marker");
    assert!(
        node_path.contains("node_modules/.pnpm/node_modules"),
        "NODE_PATH must point at the virtual store's hidden node_modules: {node_path:?}",
    );

    drop(root);
}

/// An explicit `virtualStoreDir` redirects the exported `NODE_PATH` —
/// pnpm's `pnpm run with preferSymlinkedExecutables and custom
/// virtualStoreDir` test.
#[test]
#[cfg_attr(target_os = "windows", ignore = "preferSymlinkedExecutables is inert on Windows")]
fn run_exports_node_path_from_a_custom_virtual_store_dir() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker_path = workspace.join("node-path.txt");
    let virtual_store_dir = workspace.join("foo/bar");
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": {
            "build": format!(r#"sh -c 'printf %s "$NODE_PATH" > "{}"'"#, marker_path.display()),
        },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "virtualStoreDir: {}\npreferSymlinkedExecutables: true\n",
            virtual_store_dir.display(),
        ),
    )
    .expect("write pnpm-workspace.yaml");

    pacquet.with_args(["run", "build"]).assert().success();
    let node_path = fs::read_to_string(&marker_path).expect("read marker");
    let expected = virtual_store_dir.join("node_modules");
    assert!(
        node_path.contains(&expected.display().to_string()),
        "NODE_PATH must point inside the custom virtual store: {node_path:?}",
    );

    drop(root);
}

/// `shellEmulator` runs scripts in pacquet's own shell instead of the
/// platform's, which is what makes a script written for `sh` portable to
/// Windows. The tests prove the emulator took over by pointing
/// `scriptShell` at a path that could never be spawned: the script still
/// runs, and without the setting the same configuration fails.
mod shell_emulator {
    use assert_cmd::prelude::*;
    use command_extra::CommandExtra;
    use pnpm_testing_utils::bin::CommandTempCwd;
    use serde_json::json;
    use std::{fs, path::Path};

    fn write_project(workspace: &Path, scripts: &serde_json::Value, shell_emulator: bool) {
        let manifest =
            json!({ "name": "test", "version": "0.0.0", "scripts": scripts }).to_string();
        fs::write(workspace.join("package.json"), manifest).expect("write package.json");
        let unspawnable_shell = workspace.join("no-such-shell");
        fs::write(
            workspace.join("pnpm-workspace.yaml"),
            format!(
                "scriptShell: {}\nshellEmulator: {shell_emulator}\n",
                unspawnable_shell.display(),
            ),
        )
        .expect("write pnpm-workspace.yaml");
    }

    #[test]
    fn runs_the_script_without_the_configured_shell() {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        write_project(&workspace, &json!({ "build": "echo emulated > marker.txt" }), true);

        pacquet.with_args(["run", "build"]).assert().success();

        let marker =
            fs::read_to_string(workspace.join("marker.txt")).expect("read the script's output");
        assert_eq!(marker.trim(), "emulated");

        drop(root);
    }

    #[test]
    fn without_the_setting_the_same_project_cannot_spawn_its_shell() {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        write_project(&workspace, &json!({ "build": "echo emulated > marker.txt" }), false);

        pacquet.with_args(["run", "build"]).assert().failure();
        assert!(!workspace.join("marker.txt").exists(), "the script must not have run");

        drop(root);
    }

    #[test]
    fn propagates_a_failing_scripts_exit_code() {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        write_project(&workspace, &json!({ "fail": "exit 5" }), true);

        let output = pacquet.with_args(["run", "fail"]).output().expect("spawn pacquet run");
        assert_eq!(output.status.code(), Some(5), "the script's exit code must propagate");

        drop(root);
    }
}
