#![cfg(unix)]

use super::{RunScript, run_script};
use crate::extend_path::ScriptsPrependNodePath;
use std::{collections::HashMap, fs, path::PathBuf};
use tempfile::tempdir;

fn empty_env() -> HashMap<String, String> {
    HashMap::new()
}

/// A foreground script runs with `node_modules/.bin` prepended to PATH
/// and the `npm_*` env stamped — the very setup the pre-rewrite
/// `pacquet run` (`sh -c <script>` with the bare inherited env) lacked.
#[test]
fn foreground_script_sees_bin_path_and_npm_env() {
    let dir = tempdir().expect("create temp dir");
    let pkg_root = dir.path();
    let dump = pkg_root.join("dump.txt");
    let manifest = serde_json::json!({
        "name": "runner",
        "version": "2.0.0",
        "scripts": { "show": "true" },
    });
    fs::write(pkg_root.join("package.json"), manifest.to_string()).expect("write manifest");

    let script = format!(
        "printf 'event=%s\\nname=%s\\nver=%s\\npath=%s\\n' \"$npm_lifecycle_event\" \"$npm_package_name\" \"$npm_package_version\" \"$PATH\" > {}",
        dump.display(),
    );
    let extra_env = empty_env();
    let extra_bin_paths: Vec<PathBuf> = vec![];
    let status = run_script(RunScript {
        pkg_root,
        stage: "show",
        script: &script,
        args: &[],
        manifest: &manifest,
        init_cwd: pkg_root,
        extra_bin_paths: &extra_bin_paths,
        extra_env: &extra_env,
        node_execpath: None,
        npm_execpath: None,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        print_command: false,
    })
    .expect("run the script");
    assert!(status.success(), "script should exit cleanly: {status:?}");

    let dumped = fs::read_to_string(&dump).expect("read dump");
    assert!(dumped.contains("event=show\n"), "npm_lifecycle_event stamped:\n{dumped}");
    assert!(dumped.contains("name=runner\n"), "npm_package_name stamped:\n{dumped}");
    assert!(dumped.contains("ver=2.0.0\n"), "npm_package_version stamped:\n{dumped}");
    let bin = pkg_root.join("node_modules").join(".bin");
    assert!(
        dumped.contains(&bin.to_string_lossy().into_owned()),
        "PATH must contain {}:\n{dumped}",
        bin.display(),
    );
}

/// The child's non-zero exit code is surfaced (not an error), so the CLI
/// can exit the process with the failing script's code like pnpm does.
#[test]
fn foreground_script_returns_child_exit_code() {
    let dir = tempdir().expect("create temp dir");
    let pkg_root = dir.path();
    let manifest = serde_json::json!({ "name": "x", "version": "1.0.0" });
    fs::write(pkg_root.join("package.json"), manifest.to_string()).expect("write manifest");

    let extra_env = empty_env();
    let extra_bin_paths: Vec<PathBuf> = vec![];
    let status = run_script(RunScript {
        pkg_root,
        stage: "boom",
        script: "exit 7",
        args: &[],
        manifest: &manifest,
        init_cwd: pkg_root,
        extra_bin_paths: &extra_bin_paths,
        extra_env: &extra_env,
        node_execpath: None,
        npm_execpath: None,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        print_command: false,
    })
    .expect("spawn/wait succeed even though the script fails");
    assert_eq!(status.code(), Some(7), "child exit code is surfaced");
}

/// CLI args are appended to the script body and shell-quoted, so an
/// argument with spaces stays a single token.
#[test]
fn foreground_script_appends_quoted_args() {
    let dir = tempdir().expect("create temp dir");
    let pkg_root = dir.path();
    let dump = pkg_root.join("args.txt");
    let manifest = serde_json::json!({ "name": "x", "version": "1.0.0" });
    fs::write(pkg_root.join("package.json"), manifest.to_string()).expect("write manifest");

    // A helper that wraps each positional arg in brackets. The runner
    // appends the CLI args to the script body, so they arrive as `$@`
    // of this helper; a single quoted "a b" must show up as one
    // `[a b]` token, proving the quoting survived the shell split.
    let helper = pkg_root.join("args.sh");
    fs::write(&helper, format!("printf '[%s]' \"$@\" > {}\n", dump.display()))
        .expect("write helper");
    let script = format!("sh {}", helper.display());
    let args = vec!["a b".to_string()];
    let extra_env = empty_env();
    let extra_bin_paths: Vec<PathBuf> = vec![];
    let status = run_script(RunScript {
        pkg_root,
        stage: "echo",
        script: &script,
        args: &args,
        manifest: &manifest,
        init_cwd: pkg_root,
        extra_bin_paths: &extra_bin_paths,
        extra_env: &extra_env,
        node_execpath: None,
        npm_execpath: None,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        print_command: false,
    })
    .expect("run the script");
    assert!(status.success(), "script should exit cleanly: {status:?}");
    let dumped = fs::read_to_string(&dump).expect("read dump");
    assert_eq!(dumped, "[a b]", "the space-containing arg stays one token");
}

#[test]
fn quote_arg_matches_shlex_rules() {
    assert_eq!(super::quote_arg("safe-token_1.2"), "safe-token_1.2");
    assert_eq!(super::quote_arg(""), "''");
    assert_eq!(super::quote_arg("a b"), "'a b'");
    assert_eq!(super::quote_arg("it's"), "'it'\\''s'");
}
