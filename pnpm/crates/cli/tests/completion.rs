use assert_cmd::prelude::*;
use std::process::Command;
use tempfile::TempDir;

fn pacquet() -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary")
}

fn stdout(output: std::process::Output) -> String {
    assert!(output.status.success(), "command failed: {output:?}");
    String::from_utf8(output.stdout).expect("stdout is utf8")
}

fn stderr(output: std::process::Output) -> String {
    assert!(!output.status.success(), "command succeeded unexpectedly: {output:?}");
    String::from_utf8(output.stderr).expect("stderr is utf8")
}

#[test]
fn completion_scripts_are_lightweight_shims_for_pnpm_supported_shells() {
    let cases = [
        ("bash", "_pnpm_completion"),
        ("fish", "complete -c pnpm"),
        ("pwsh", "Register-ArgumentCompleter -Native -CommandName pnpm"),
        ("zsh", "#compdef pnpm"),
    ];

    for (shell, marker) in cases {
        let output =
            pacquet().args(["completion", shell]).output().expect("run pacquet completion");
        let script = stdout(output);
        assert!(script.contains(marker), "{shell} script should contain {marker:?}: {script}");
        assert!(
            script.contains("pnpm completion-server"),
            "{shell} script should call completion-server: {script}",
        );
        assert!(script.lines().count() < 80, "{shell} script should be lightweight: {script}");
        assert!(
            !script.contains("Install packages"),
            "{shell} script should not inline command help: {script}",
        );
    }
}

#[test]
fn completion_scripts_do_not_expose_redundant_parameter_plumbing() {
    let cases = [("bash", "EXTRA"), ("zsh", "*::extra:_default")];

    for (shell, leaked_marker) in cases {
        let output =
            pacquet().args(["completion", shell]).output().expect("run pacquet completion");
        let script = stdout(output);
        assert!(
            !script.contains(leaked_marker),
            "{shell} script should not contain hidden extra argument marker {leaked_marker:?}: {script}",
        );
    }
}

#[test]
fn completion_scripts_preserve_current_token_for_fish_and_pwsh() {
    let fish =
        stdout(pacquet().args(["completion", "fish"]).output().expect("run pacquet completion"));
    assert!(fish.contains("commandline -ct"), "{fish}");

    let pwsh =
        stdout(pacquet().args(["completion", "pwsh"]).output().expect("run pacquet completion"));
    assert!(pwsh.contains("$wordToComplete"), "{pwsh}");
}

#[test]
fn completion_server_lists_top_level_commands() {
    let output = pacquet()
        .args(["completion-server", "--", "pnpm", ""])
        .output()
        .expect("run pnpm completion-server");
    let reply = stdout(output);

    assert!(reply.lines().any(|line| line == "install"), "{reply}");
    assert!(reply.lines().any(|line| line == "completion"), "{reply}");
    assert!(reply.lines().any(|line| line == "add"), "{reply}");
}

#[test]
fn completion_server_lists_options_for_current_command() {
    let output = pacquet()
        .args(["completion-server", "--", "pnpm", "install", "--"])
        .output()
        .expect("run pnpm completion-server");
    let reply = stdout(output);

    assert!(reply.lines().any(|line| line == "--filter"), "{reply}");
    assert!(reply.lines().any(|line| line == "--reporter"), "{reply}");
    assert!(reply.lines().any(|line| line == "--frozen-lockfile"), "{reply}");
}

#[test]
fn completion_server_lists_option_values() {
    let output = pacquet()
        .args(["completion-server", "--", "pnpm", "--reporter", ""])
        .output()
        .expect("run pnpm completion-server");
    let reply = stdout(output);

    assert!(reply.lines().any(|line| line == "default"), "{reply}");
    assert!(reply.lines().any(|line| line == "append-only"), "{reply}");
    assert!(reply.lines().any(|line| line == "ndjson"), "{reply}");
    assert!(reply.lines().any(|line| line == "silent"), "{reply}");
}

#[test]
fn completion_server_lists_option_values_only_after_option_name() {
    let output = pacquet()
        .args(["completion-server", "--", "pnpm", "--reporter", "default", ""])
        .output()
        .expect("run pnpm completion-server");
    let reply = stdout(output);

    assert!(reply.lines().any(|line| line == "install"), "{reply}");
    assert!(!reply.lines().any(|line| line == "append-only"), "{reply}");
}

#[test]
fn completion_server_does_not_treat_option_values_as_commands() {
    let output = pacquet()
        .args(["completion-server", "--", "pnpm", "--filter", "install", ""])
        .output()
        .expect("run pnpm completion-server");
    let reply = stdout(output);

    assert!(reply.lines().any(|line| line == "add"), "{reply}");
    assert!(!reply.lines().any(|line| line == "--frozen-lockfile"), "{reply}");
}

#[test]
fn completion_server_stops_after_double_dash_separator() {
    let output = pacquet()
        .args(["completion-server", "--", "pnpm", "--", "--rep"])
        .output()
        .expect("run pnpm completion-server");
    let reply = stdout(output);

    assert_eq!(reply, "");
}

#[test]
fn completion_server_lists_nested_subcommands() {
    let output = pacquet()
        .args(["completion-server", "--", "pnpm", "store", ""])
        .output()
        .expect("run pnpm completion-server");
    let reply = stdout(output);

    assert!(reply.lines().any(|line| line == "prune"), "{reply}");
    assert!(reply.lines().any(|line| line == "path"), "{reply}");
}

#[test]
fn completion_server_lists_ci_command() {
    let output = pacquet()
        .args(["completion-server", "--", "pnpm", "ci", "--"])
        .output()
        .expect("run pnpm completion-server");
    let reply = stdout(output);

    assert!(reply.lines().any(|line| line == "--frozen-lockfile"), "{reply}");
    assert!(reply.lines().any(|line| line == "--dry-run"), "{reply}");
    assert!(reply.lines().any(|line| line == "--lockfile"), "{reply}");
}

#[test]
fn completion_server_completes_ci_aliases() {
    let output = pacquet()
        .args(["completion-server", "--", "pnpm", "ci"])
        .output()
        .expect("run pnpm completion-server");
    let reply = stdout(output);

    assert_eq!(reply.lines().collect::<Vec<_>>(), ["ci"]);
}

#[test]
fn completion_server_completes_clean_install_alias() {
    let output = pacquet()
        .args(["completion-server", "--", "pnpm", "clean-install"])
        .output()
        .expect("run pnpm completion-server");
    let reply = stdout(output);

    assert_eq!(reply.lines().collect::<Vec<_>>(), ["clean-install"]);
}

#[test]
fn completion_server_completes_ic_alias() {
    let output = pacquet()
        .args(["completion-server", "--", "pnpm", "ic"])
        .output()
        .expect("run pnpm completion-server");
    let reply = stdout(output);

    assert_eq!(reply.lines().collect::<Vec<_>>(), ["ic"]);
}

#[test]
fn completion_server_filters_command_prefixes() {
    let output = pacquet()
        .args(["completion-server", "--", "pnpm", "inst"])
        .output()
        .expect("run pnpm completion-server");
    let reply = stdout(output);

    assert_eq!(reply.lines().collect::<Vec<_>>(), ["install", "install-test", "install-clean"]);
}

#[test]
fn completion_server_filters_option_prefixes() {
    let output = pacquet()
        .args(["completion-server", "--", "pnpm", "--rep"])
        .output()
        .expect("run pnpm completion-server");
    let reply = stdout(output);

    assert_eq!(reply.lines().collect::<Vec<_>>(), ["--reporter"]);
}

#[test]
fn completion_server_filters_option_value_prefixes() {
    let output = pacquet()
        .args(["completion-server", "--", "pnpm", "--reporter", "a"])
        .output()
        .expect("run pnpm completion-server");
    let reply = stdout(output);

    assert_eq!(reply.lines().collect::<Vec<_>>(), ["append-only"]);
}

#[test]
fn completion_server_completes_equals_option_values() {
    let output = pacquet()
        .args(["completion-server", "--", "pnpm", "--reporter=de"])
        .output()
        .expect("run pnpm completion-server");
    let reply = stdout(output);

    assert_eq!(reply.lines().collect::<Vec<_>>(), ["--reporter=default"]);
}

#[test]
fn completion_server_lists_completion_shells() {
    let output = pacquet()
        .args(["completion-server", "--", "pnpm", "completion", ""])
        .output()
        .expect("run pnpm completion-server");
    let reply = stdout(output);

    assert_eq!(reply.lines().collect::<Vec<_>>(), ["bash", "fish", "pwsh", "zsh"]);
}

#[test]
fn completion_server_does_not_require_a_project_or_existing_dir_argument() {
    let root = TempDir::new().expect("temp dir");
    let missing_dir = root.path().join("missing");
    let output = pacquet()
        .args(["--dir"])
        .arg(&missing_dir)
        .args(["completion-server", "--", "pnpm", "completion", ""])
        .output()
        .expect("run pnpm completion-server");
    let reply = stdout(output);

    assert_eq!(reply.lines().collect::<Vec<_>>(), ["bash", "fish", "pwsh", "zsh"]);
}

#[test]
fn completion_missing_shell_errors_like_pnpm() {
    let output = pacquet().arg("completion").output().expect("run pacquet completion");
    let err = stderr(output);
    assert!(err.contains("`pnpm completion` requires a shell name"), "{err}");
}

#[test]
fn completion_unsupported_shell_errors_like_pnpm() {
    let output = pacquet().args(["completion", "elvish"]).output().expect("run pacquet completion");
    let err = stderr(output);
    assert!(err.contains("'elvish' is not supported"), "{err}");
    assert!(err.contains("Supported shells are: bash, fish, pwsh, zsh"), "{err}");
}

#[test]
fn completion_redundant_parameters_error_like_pnpm() {
    let output = pacquet()
        .args(["completion", "bash", "fish", "pwsh"])
        .output()
        .expect("run pacquet completion");
    let err = stderr(output);
    assert!(err.contains("The 2 parameters after shell is not necessary"), "{err}");
}

#[test]
fn completion_does_not_require_a_project_or_existing_dir_argument() {
    let root = TempDir::new().expect("temp dir");
    let missing_dir = root.path().join("missing");
    let output = pacquet()
        .args(["--dir"])
        .arg(&missing_dir)
        .args(["completion", "zsh"])
        .output()
        .expect("run pacquet completion");
    let script = stdout(output);
    assert!(script.contains("#compdef pnpm"), "{script}");
}
