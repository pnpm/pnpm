use miette::Diagnostic as _;

use crate::cli_args::CliArgs;
use clap::Parser as _;

use super::{CompletionError, CompletionShell, SUPPORTED_SHELLS, shell_from_args};

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn diagnostic_code(err: &CompletionError) -> Option<String> {
    err.code().map(|code| code.to_string())
}

#[test]
fn supported_shells_are_pnpm_compatible() {
    assert_eq!(SUPPORTED_SHELLS, &["bash", "fish", "pwsh", "zsh"]);
}

#[test]
fn missing_shell_errors_like_pnpm() {
    let err = shell_from_args(None, &[]).expect_err("missing shell rejected");
    assert_eq!(err.to_string(), "`pnpm completion` requires a shell name");
    assert_eq!(diagnostic_code(&err), Some("ERR_PNPM_MISSING_SHELL_NAME".to_string()));
}

#[test]
fn empty_shell_errors_like_pnpm() {
    let err = shell_from_args(Some(" \n"), &[]).expect_err("blank shell rejected");
    assert_eq!(err.to_string(), "`pnpm completion` requires a shell name");
    assert_eq!(diagnostic_code(&err), Some("ERR_PNPM_MISSING_SHELL_NAME".to_string()));
}

#[test]
fn unsupported_shell_lists_supported_shells() {
    let err = shell_from_args(Some("elvish"), &[]).expect_err("unsupported shell rejected");
    assert_eq!(err.to_string(), "'elvish' is not supported");
    assert_eq!(diagnostic_code(&err), Some("ERR_PNPM_UNSUPPORTED_SHELL".to_string()));
    assert_eq!(
        err.help().map(|help| help.to_string()),
        Some("Supported shells are: bash, fish, pwsh, zsh".to_string()),
    );
}

#[test]
fn redundant_parameters_are_rejected_before_shell_validation() {
    let extra = strings(&["fish", "pwsh", "zsh"]);
    let err = shell_from_args(Some("elvish"), &extra).expect_err("redundant params rejected");
    assert_eq!(err.to_string(), "The 3 parameters after shell is not necessary");
    assert_eq!(diagnostic_code(&err), Some("ERR_PNPM_REDUNDANT_PARAMETERS".to_string()));
}

#[test]
fn supported_shells_parse_after_trimming() {
    assert_eq!(shell_from_args(Some(" bash\n"), &[]).expect("bash"), CompletionShell::Bash);
    assert_eq!(shell_from_args(Some("fish"), &[]).expect("fish"), CompletionShell::Fish);
    assert_eq!(shell_from_args(Some("pwsh"), &[]).expect("pwsh"), CompletionShell::Pwsh);
    assert_eq!(shell_from_args(Some("zsh"), &[]).expect("zsh"), CompletionShell::Zsh);
}

#[test]
fn generated_scripts_call_completion_server() {
    let shells =
        [CompletionShell::Bash, CompletionShell::Fish, CompletionShell::Pwsh, CompletionShell::Zsh];

    for shell in shells {
        let mut output = Vec::new();
        super::generate_completion(shell, &mut output).expect("generate completion");
        let script = String::from_utf8(output).expect("script is utf8");
        assert!(script.contains("pnpm completion-server"), "{script}");
    }
}

#[test]
fn generated_scripts_register_pn_alias() {
    let cases = [
        (CompletionShell::Bash, &["complete -F _pnpm_completion pnpm pn"][..]),
        (
            CompletionShell::Fish,
            &[
                r#"complete -c pnpm -f -a "(__pnpm_completion)""#,
                r#"complete -c pn -f -a "(__pnpm_completion)""#,
            ],
        ),
        (
            CompletionShell::Pwsh,
            &["Register-ArgumentCompleter -Native -CommandName pnpm,pn -ScriptBlock"],
        ),
        (CompletionShell::Zsh, &["#compdef pnpm pn", "compdef _pnpm_completion pnpm pn"]),
    ];

    for (shell, snippets) in cases {
        let mut output = Vec::new();
        super::generate_completion(shell, &mut output).expect("generate completion");
        let script = String::from_utf8(output).expect("script is utf8");
        for snippet in snippets {
            assert!(
                script.contains(snippet),
                "{shell:?} script should contain {snippet:?}: {script}",
            );
        }
    }
}

#[test]
fn pn_is_stripped_like_the_other_pnpm_binary_names() {
    for binary in ["pnpm", "pn", "pacquet", "/usr/local/bin/pn", "pn.exe"] {
        assert_eq!(
            super::words_without_binary(&strings(&[binary, "add", ""])),
            strings(&["add", ""]),
            "{binary} should be dropped",
        );
    }

    assert_eq!(super::words_without_binary(&strings(&["npm", ""])), strings(&["npm", ""]));
}

#[test]
fn completion_server_treats_pn_as_the_pnpm_binary() {
    let pnpm = super::complete_words(&strings(&["pnpm", ""]));
    let pn = super::complete_words(&strings(&["pn", ""]));

    assert_eq!(pnpm, pn);
    assert!(pn.iter().any(|completion| completion == "install"), "{pn:?}");
}

#[test]
fn completion_can_run_before_async_runtime_setup() {
    let args = CliArgs::parse_from(["pnpm", "completion-server", "--", "pnpm", "completion", ""]);

    assert!(args.run_completion_if_requested().expect("completion dispatch succeeds"));
}
