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
        let output = pacquet()
            .args(["completion", shell])
            .output()
            .expect("run pacquet completion");
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
        let output = pacquet()
            .args(["completion", shell])
            .output()
            .expect("run pacquet completion");
        let script = stdout(output);
        assert!(
            !script.contains(leaked_marker),
            "{shell} script should not contain hidden extra argument marker {leaked_marker:?}: {script}",
        );
    }
}

#[test]
fn completion_scripts_preserve_current_token_for_fish_and_pwsh() {
    let fish = stdout(
        pacquet()
            .args(["completion", "fish"])
            .output()
            .expect("run pacquet completion"),
    );
    assert!(fish.contains("commandline -ct"), "{fish}");

    let pwsh = stdout(
        pacquet()
            .args(["completion", "pwsh"])
            .output()
            .expect("run pacquet completion"),
    );
    assert!(pwsh.contains("$wordToComplete"), "{pwsh}");
}

#[test]
fn completion_server_lists_top_level_commands() {
    let output = pacquet()
        .args(["completion-server", "--", "pnpm", ""])
        .output()
        .expect("run pnpm completion-server");
    let reply = stdout(output);

    assert!(
        reply
            .lines()
            .any(|line| line == "install"),
        "{reply}",
    );
    assert!(
        reply
            .lines()
            .any(|line| line == "completion"),
        "{reply}",
    );
    assert!(reply.lines().any(|line| line == "add"), "{reply}");
}

#[test]
fn completion_server_answers_the_pn_alias_like_pnpm() {
    let reply = |binary: &str| {
        let output = pacquet()
            .args(["completion-server", "--", binary, ""])
            .output()
            .expect("run pnpm completion-server");
        stdout(output)
    };

    let pn = reply("pn");
    assert_eq!(pn, reply("pnpm"));
    assert!(pn.lines().any(|line| line == "install"), "{pn}");
}

#[test]
fn completion_server_lists_options_for_current_command() {
    let output = pacquet()
        .args(["completion-server", "--", "pnpm", "install", "--"])
        .output()
        .expect("run pnpm completion-server");
    let reply = stdout(output);

    assert!(
        reply
            .lines()
            .any(|line| line == "--filter"),
        "{reply}",
    );
    assert!(
        reply
            .lines()
            .any(|line| line == "--reporter"),
        "{reply}",
    );
    assert!(
        reply
            .lines()
            .any(|line| line == "--frozen-lockfile"),
        "{reply}",
    );
}

#[test]
fn completion_server_lists_option_values() {
    let output = pacquet()
        .args(["completion-server", "--", "pnpm", "--reporter", ""])
        .output()
        .expect("run pnpm completion-server");
    let reply = stdout(output);

    assert!(
        reply
            .lines()
            .any(|line| line == "default"),
        "{reply}",
    );
    assert!(
        reply
            .lines()
            .any(|line| line == "append-only"),
        "{reply}",
    );
    assert!(
        reply
            .lines()
            .any(|line| line == "ndjson"),
        "{reply}",
    );
    assert!(
        reply
            .lines()
            .any(|line| line == "silent"),
        "{reply}",
    );
}

#[test]
fn completion_server_lists_option_values_only_after_option_name() {
    let output = pacquet()
        .args(["completion-server", "--", "pnpm", "--reporter", "default", ""])
        .output()
        .expect("run pnpm completion-server");
    let reply = stdout(output);

    assert!(
        reply
            .lines()
            .any(|line| line == "install"),
        "{reply}",
    );
    assert!(
        !reply
            .lines()
            .any(|line| line == "append-only"),
        "{reply}",
    );
}

#[test]
fn completion_server_does_not_treat_option_values_as_commands() {
    let output = pacquet()
        .args(["completion-server", "--", "pnpm", "--filter", "install", ""])
        .output()
        .expect("run pnpm completion-server");
    let reply = stdout(output);

    assert!(reply.lines().any(|line| line == "add"), "{reply}");
    assert!(
        !reply
            .lines()
            .any(|line| line == "--frozen-lockfile"),
        "{reply}",
    );
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

    assert!(
        reply
            .lines()
            .any(|line| line == "prune"),
        "{reply}",
    );
    assert!(reply.lines().any(|line| line == "path"), "{reply}");
}

#[test]
fn completion_server_lists_ci_command() {
    let output = pacquet()
        .args(["completion-server", "--", "pnpm", "ci", "--"])
        .output()
        .expect("run pnpm completion-server");
    let reply = stdout(output);

    assert!(
        reply
            .lines()
            .any(|line| line == "--frozen-lockfile"),
        "{reply}",
    );
    assert!(
        reply
            .lines()
            .any(|line| line == "--dry-run"),
        "{reply}",
    );
    assert!(
        reply
            .lines()
            .any(|line| line == "--lockfile"),
        "{reply}",
    );
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
fn completion_server_completes_install_clean_alias() {
    let output = pacquet()
        .args(["completion-server", "--", "pnpm", "install-clean"])
        .output()
        .expect("run pnpm completion-server");
    let reply = stdout(output);

    assert_eq!(reply.lines().collect::<Vec<_>>(), ["install-clean"]);
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
    let output = pacquet()
        .arg("completion")
        .output()
        .expect("run pacquet completion");
    let err = stderr(output);
    assert!(err.contains("`pnpm completion` requires a shell name"), "{err}");
}

#[test]
fn completion_unsupported_shell_errors_like_pnpm() {
    let output = pacquet()
        .args(["completion", "elvish"])
        .output()
        .expect("run pacquet completion");
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

#[test]
fn completion_server_completes_project_scripts() {
    let project = TempDir::new().unwrap();
    std::fs::write(
        project.path().join("package.json"),
        r#"{"scripts":{"hello":"echo hi","build":"echo b","build:watch":"echo w"}}"#,
    )
    .unwrap();
    for shell in ["bash", "fish", "pwsh", "zsh"] {
        for (words, expected) in [
            (vec!["pnpm", "run", ""], "build\nbuild:watch\nhello\n"),
            (vec!["pnpm", "run", "bu"], "build\nbuild:watch\n"),
            (vec!["pnpm", "run", "build:"], "build:watch\n"),
            (vec!["pnpm", "--color", "run", "he"], "hello\n"),
            (vec!["pnpm", "run", "--color", "he"], "hello\n"),
            (vec!["pn", "run-script", "he"], "hello\n"),
            (vec!["pnpm", "--filter", "run", "run", "--if-present", "he"], "hello\n"),
            (vec!["pnpm", "run", "hello", ""], ""),
            (vec!["pnpm", "run", "--", ""], ""),
        ] {
            let output = pacquet()
                .current_dir(project.path())
                .env("SHELL", shell)
                .args(["completion-server", "--"])
                .args(&words)
                .output()
                .unwrap();
            let expected =
                if shell == "zsh" { expected.replace(':', r"\:") } else { expected.to_string() };
            assert_eq!(stdout(output), expected, "{shell}: {words:?}");
        }
    }
}

#[test]
fn completion_server_finds_scripts_from_project_subdirectories() {
    let project = TempDir::new().unwrap();
    let nested = project.path().join("src/nested");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(project.path().join("package.json"), r#"{"scripts":{"hello":"echo hi"}}"#)
        .unwrap();
    let output = pacquet()
        .current_dir(nested)
        .args(["completion-server", "--", "pnpm", "run", ""])
        .output()
        .unwrap();
    assert_eq!(stdout(output), "hello\n");
}

#[test]
fn completion_server_handles_projects_without_scripts() {
    for manifest in [None, Some("{}"), Some(r#"{"scripts":{}}"#)] {
        let project = TempDir::new().unwrap();
        if let Some(manifest) = manifest {
            std::fs::write(project.path().join("package.json"), manifest).unwrap();
        }
        let output = pacquet()
            .current_dir(project.path())
            .args(["completion-server", "--", "pnpm", "run", ""])
            .output()
            .unwrap();
        assert_eq!(stdout(output), "");
    }
}

#[test]
fn completion_server_respects_project_directory_options() {
    let project = TempDir::new().unwrap();
    let target = project.path().join("target-project");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(project.path().join("package.json"), r#"{"scripts":{"wrong":"echo wrong"}}"#)
        .unwrap();
    std::fs::write(target.join("package.json"), r#"{"scripts":{"hello":"echo hi"}}"#).unwrap();
    for words in [
        vec!["pnpm", "--dir", "target-project", "run", ""],
        vec!["pnpm", "run", "--dir=target-project", ""],
        vec!["pnpm", "-C", "target-project", "run-script", ""],
        vec!["pnpm", "-Ctarget-project", "run", ""],
        vec!["pnpm", "-rCtarget-project", "run", ""],
        vec!["pnpm", "-rC", "target-project", "run", ""],
        vec!["pnpm", "--prefix=target-project", "run", ""],
    ] {
        let output = pacquet()
            .current_dir(project.path())
            .args(["completion-server", "--"])
            .args(&words)
            .output()
            .unwrap();
        assert_eq!(stdout(output), "hello\n", "{words:?}");
    }
    let output = pacquet()
        .current_dir(project.path())
        .args(["completion-server", "--", "pnpm", "run", "--dir", ""])
        .output()
        .unwrap();
    assert_eq!(stdout(output), "");
}

#[test]
fn completion_server_reports_invalid_script_manifests() {
    let project = TempDir::new().unwrap();
    std::fs::write(project.path().join("package.json"), "{").unwrap();
    let output = pacquet()
        .current_dir(project.path())
        .args(["completion-server", "--", "pnpm", "run", ""])
        .output()
        .unwrap();
    let error = stderr(output);
    assert!(error.contains("package.json"), "{error}");
}

#[test]
fn completion_server_uses_the_nearest_project_manifest() {
    let project = TempDir::new().unwrap();
    let child = project.path().join("child");
    let nested = child.join("src");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(project.path().join("package.json"), r#"{"scripts":{"parent":"echo parent"}}"#)
        .unwrap();
    std::fs::write(child.join("package.yaml"), "scripts:\n  child: echo child\n").unwrap();
    let output = pacquet()
        .current_dir(nested)
        .args(["completion-server", "--", "pnpm", "run", ""])
        .output()
        .unwrap();
    assert_eq!(stdout(output), "child\n");
}

#[test]
fn completion_server_does_not_search_above_explicit_directories() {
    let project = TempDir::new().unwrap();
    std::fs::create_dir(project.path().join("subdir")).unwrap();
    std::fs::write(project.path().join("package.json"), r#"{"scripts":{"parent":"echo parent"}}"#)
        .unwrap();
    let output = pacquet()
        .current_dir(project.path())
        .args(["completion-server", "--", "pnpm", "--dir", "subdir", "run", ""])
        .output()
        .unwrap();
    assert_eq!(stdout(output), "");
}

#[test]
fn completion_server_respects_workspace_root_selection() {
    let project = TempDir::new().unwrap();
    let child = project.path().join("child");
    std::fs::create_dir(&child).unwrap();
    std::fs::write(project.path().join("pnpm-workspace.yaml"), "packages:\n  - child\n").unwrap();
    std::fs::write(project.path().join("package.json"), r#"{"scripts":{"root":"echo root"}}"#)
        .unwrap();
    std::fs::write(child.join("package.json"), r#"{"scripts":{"child":"echo child"}}"#).unwrap();
    for flags in [
        &["--workspace-root"][..],
        &["-w"],
        &["-rw"],
        &["-wC."],
        &["--dir", "--workspace-root"],
        &["-C", "--workspace-root"],
    ] {
        for after_command in [false, true] {
            let mut command = pacquet();
            command
                .current_dir(&child)
                .args(["completion-server", "--", "pnpm"]);
            if after_command {
                command.arg("run").args(flags);
            } else {
                command.args(flags).arg("run");
            }
            let output = command.arg("").output().unwrap();
            assert_eq!(stdout(output), "root\n", "{flags:?}, after_command={after_command}");
        }
    }
}

#[cfg(windows)]
#[test]
fn completion_powershell_preserves_literal_script_names() {
    let names = [
        "semi;colon",
        "pipe|name",
        "dollar$name",
        "space name",
        "quote'name",
        "tick`name",
        "$(Write-Output injected)",
    ];
    let project = project_with_scripts(&names);
    let mut script = stdout(
        pacquet()
            .args(["completion", "pwsh"])
            .output()
            .unwrap(),
    );
    script.push_str(include_str!("completion/powershell_safety.ps1"));
    let output = Command::new("pwsh")
        .current_dir(project.path())
        .env("PATH", prepend_binary_dir_to_path())
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .unwrap();
    let reply = stdout(output);
    let mut actual: Vec<_> = reply.lines().collect();
    actual.sort_unstable();
    let mut expected = names.to_vec();
    expected.sort_unstable();
    assert_eq!(actual, expected);
}

fn prepend_binary_dir_to_path() -> std::ffi::OsString {
    let binary = pacquet().get_program().to_owned();
    let directory = std::path::Path::new(&binary)
        .parent()
        .unwrap()
        .to_path_buf();
    let original = std::env::var_os("PATH").unwrap();
    std::env::join_paths(std::iter::once(directory).chain(std::env::split_paths(&original)))
        .unwrap()
}

#[cfg(unix)]
#[test]
fn completion_bash_preserves_literal_script_names() {
    let names = [
        "*literal",
        "semi;printf injected",
        "dollar$(printf injected)",
        "space name",
        "quote'name",
        "tick`name",
    ];
    let project = project_with_scripts(&names);
    std::fs::write(project.path().join("expanded-literal"), "").unwrap();
    let mut script = stdout(
        pacquet()
            .args(["completion", "bash"])
            .output()
            .unwrap(),
    );
    script.push_str(
        r#"
COMP_WORDS=(pnpm run "")
COMP_CWORD=2
COMP_LINE='pnpm run '
COMP_POINT=9
_pnpm_completion
eval "set -- ${COMPREPLY[*]}"
printf '%s\n' "$@"
"#,
    );
    let output = Command::new("bash")
        .current_dir(project.path())
        .env("PATH", prepend_binary_dir_to_path())
        .args(["--noprofile", "--norc", "-c", &script])
        .output()
        .unwrap();
    let reply = stdout(output);
    let actual: Vec<_> = reply.lines().collect();
    let mut expected = names.to_vec();
    expected.sort_unstable();
    assert_eq!(actual, expected);
}

#[test]
fn completion_server_accepts_attached_hyphen_prefixed_directories() {
    let project = TempDir::new().unwrap();
    let target = project.path().join("-target");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("package.json"), r#"{"scripts":{"hello":"echo hi"}}"#).unwrap();
    for option in ["--dir=-target", "--prefix=-target", "-C-target", "-rC-target"] {
        let output = pacquet()
            .current_dir(project.path())
            .args(["completion-server", "--", "pnpm", "run", option, ""])
            .output()
            .unwrap();
        assert_eq!(stdout(output), "hello\n", "{option}");
    }
}

fn project_with_scripts(names: &[&str]) -> TempDir {
    let project = TempDir::new().unwrap();
    let scripts: serde_json::Map<String, serde_json::Value> = names
        .iter()
        .map(|name| (name.to_string(), serde_json::Value::String("echo safe".to_string())))
        .collect();
    std::fs::write(
        project.path().join("package.json"),
        serde_json::to_vec(&serde_json::json!({"scripts": scripts})).unwrap(),
    )
    .unwrap();
    project
}
