use super::{EmulatedOutput, ShellEmulatorError, execute_emulated};
use pnpm_reporter::LifecycleStdio;
use std::{collections::HashMap, path::Path, sync::Mutex};
use tempfile::tempdir;

/// Run `script` in `cwd`, returning its exit code and the lines it wrote,
/// each tagged with the stream it came from.
fn run(
    script: &str,
    cwd: &Path,
    env: &HashMap<String, String>,
) -> (i32, Vec<(LifecycleStdio, String)>) {
    let lines = Mutex::new(Vec::new());
    let sink = |stdio, line| lines.lock().expect("the sink is never poisoned").push((stdio, line));
    let code = execute_emulated(script, cwd, env, EmulatedOutput::Lines(&sink))
        .expect("run the script under the emulator");
    (code, lines.into_inner().expect("the sink is never poisoned"))
}

/// The lines `stdio` carried, in the order the sink saw them.
fn lines_from(lines: &[(LifecycleStdio, String)], stdio: LifecycleStdio) -> Vec<&str> {
    lines
        .iter()
        .filter(|(line_stdio, _)| *line_stdio == stdio)
        .map(|(_, line)| line.as_str())
        .collect()
}

#[test]
fn captures_stdout_line_by_line() {
    let dir = tempdir().expect("create a temp dir");
    let (code, lines) = run("echo first && echo second", dir.path(), &HashMap::new());
    dbg!(&lines);
    assert_eq!(code, 0);
    assert_eq!(
        lines,
        vec![
            (LifecycleStdio::Stdout, "first".to_string()),
            (LifecycleStdio::Stdout, "second".to_string()),
        ],
    );
}

/// Each stream keeps its own order, but the two are pumped
/// independently, so which of them reaches the sink first is a race and
/// is deliberately not asserted.
#[test]
fn separates_stderr_from_stdout() {
    let dir = tempdir().expect("create a temp dir");
    let (code, lines) = run("echo out && echo err 1>&2", dir.path(), &HashMap::new());
    dbg!(&lines);
    assert_eq!(code, 0);
    assert_eq!(lines_from(&lines, LifecycleStdio::Stdout), vec!["out"]);
    assert_eq!(lines_from(&lines, LifecycleStdio::Stderr), vec!["err"]);
}

/// A script's last line often arrives without a trailing newline; it must
/// still reach the sink rather than being dropped at EOF.
#[test]
fn emits_a_final_line_without_a_newline() {
    let dir = tempdir().expect("create a temp dir");
    std::fs::write(dir.path().join("unterminated.txt"), "trailing")
        .expect("write a file with no trailing newline");
    let (code, lines) = run("cat unterminated.txt", dir.path(), &HashMap::new());
    dbg!(&lines);
    assert_eq!(code, 0);
    assert_eq!(lines, vec![(LifecycleStdio::Stdout, "trailing".to_string())]);
}

#[test]
fn reports_the_scripts_exit_code() {
    let dir = tempdir().expect("create a temp dir");
    let (code, _) = run("exit 3", dir.path(), &HashMap::new());
    assert_eq!(code, 3);
}

/// The emulator resolves variables against the environment the caller
/// built for the script, not against pacquet's own environment.
#[test]
fn expands_variables_from_the_supplied_env() {
    let dir = tempdir().expect("create a temp dir");
    let env = HashMap::from([("npm_package_name".to_string(), "my-pkg".to_string())]);
    let (code, lines) = run("echo $npm_package_name", dir.path(), &env);
    dbg!(&lines);
    assert_eq!(code, 0);
    assert_eq!(lines, vec![(LifecycleStdio::Stdout, "my-pkg".to_string())]);
}

#[test]
fn runs_in_the_given_directory() {
    let dir = tempdir().expect("create a temp dir");
    let (code, _) = run("echo hello > written.txt", dir.path(), &HashMap::new());
    assert_eq!(code, 0);
    let written =
        std::fs::read_to_string(dir.path().join("written.txt")).expect("read the written file");
    assert_eq!(written.trim(), "hello");
}

#[test]
fn rejects_a_script_the_shell_cannot_parse() {
    let dir = tempdir().expect("create a temp dir");
    let error = execute_emulated(
        "echo 'unterminated",
        dir.path(),
        &HashMap::new(),
        EmulatedOutput::Inherit,
    )
    .expect_err("an unparsable script is an error, not an exit code");
    dbg!(&error);
    assert!(matches!(error, ShellEmulatorError::Parse { .. }));
}
