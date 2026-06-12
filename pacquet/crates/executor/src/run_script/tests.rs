use super::{RunScript, build_command, posix_quote, run_script};
use crate::extend_path::ScriptsPrependNodePath;
use std::{collections::HashMap, fs, path::Path};
use tempfile::tempdir;

#[test]
fn posix_quote_leaves_safe_strings_unquoted() {
    assert_eq!(posix_quote("hello-world"), "hello-world");
    assert_eq!(posix_quote("a_b@1.0.0/path:to,thing"), "a_b@1.0.0/path:to,thing");
}

#[test]
fn posix_quote_wraps_unsafe_strings() {
    assert_eq!(posix_quote(""), "''");
    assert_eq!(posix_quote("a b"), "'a b'");
    assert_eq!(posix_quote("two words"), "'two words'");
}

#[test]
fn posix_quote_escapes_embedded_single_quotes() {
    // The `shlex` escape for an embedded quote is `'"'"'`.
    assert_eq!(posix_quote("it's"), r#"'it'"'"'s'"#);
}

#[test]
fn build_command_without_args_returns_script_unchanged() {
    assert_eq!(build_command("tsc --build", &[]), "tsc --build");
}

#[test]
#[cfg_attr(target_os = "windows", ignore = "asserts POSIX quoting; Windows uses JSON.stringify")]
fn build_command_appends_quoted_args() {
    let args = ["plain".to_string(), "needs quoting".to_string()];
    assert_eq!(build_command("echo", &args), "echo plain 'needs quoting'");
}

fn manifest() -> serde_json::Value {
    serde_json::json!({ "name": "t", "version": "1.0.0" })
}

fn run(pkg_root: &Path, stage: &str, script: &str, args: &[String]) -> std::process::ExitStatus {
    let extra_env = HashMap::new();
    run_script(&RunScript {
        manifest: &manifest(),
        stage,
        script,
        args,
        pkg_root,
        init_cwd: pkg_root,
        extra_bin_paths: &[],
        script_shell: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        node_execpath: None,
        npm_execpath: None,
        user_agent: None,
        extra_env: &extra_env,
        silent: true,
    })
    .expect("run the script")
}

#[test]
#[cfg_attr(target_os = "windows", ignore = "uses a POSIX shell script body")]
fn run_script_stamps_npm_lifecycle_event() {
    let dir = tempdir().expect("temp dir");
    let marker = dir.path().join("stage.txt");
    let script = format!("printf %s \"$npm_lifecycle_event\" > \"{}\"", marker.display());

    let status = run(dir.path(), "build", &script, &[]);
    assert!(status.success(), "the script should exit cleanly");
    let written = fs::read_to_string(&marker).expect("read marker");
    assert_eq!(written, "build");
}

#[test]
#[cfg_attr(target_os = "windows", ignore = "uses a POSIX shell script body")]
fn run_script_prepends_node_modules_bin_to_path() {
    let dir = tempdir().expect("temp dir");
    let marker = dir.path().join("path.txt");
    let script = format!("printf %s \"$PATH\" > \"{}\"", marker.display());

    run(dir.path(), "build", &script, &[]);
    let written = fs::read_to_string(&marker).expect("read marker");
    let expected_bin = dir.path().join("node_modules").join(".bin");
    eprintln!("PATH:\n{written}\n");
    assert!(
        written.split(':').any(|entry| Path::new(entry) == expected_bin),
        "PATH should contain the project's node_modules/.bin",
    );
}

#[test]
#[cfg_attr(target_os = "windows", ignore = "uses a POSIX shell script body")]
fn run_script_returns_the_scripts_exit_status() {
    let dir = tempdir().expect("temp dir");
    let status = run(dir.path(), "build", "exit 7", &[]);
    assert_eq!(status.code(), Some(7));
}
