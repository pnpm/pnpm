use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::fs;

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
/// spawned shell verbatim, joined by spaces. Mirrors `pnpm run
/// <script> -- <args>` minus the npm `--` separator (pacquet does
/// not require it).
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

/// Without `--if-present`, calling a script that does not exist
/// fails. Mirrors `pnpm run` behavior — the missing-script error
/// comes from `PackageManifest::script`, surfaced through the
/// `RunArgs::run` `wrap_err` chain.
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
