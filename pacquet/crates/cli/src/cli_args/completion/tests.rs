use miette::Diagnostic as _;

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
    assert_eq!(err.to_string(), "`pacquet completion` requires a shell name");
    assert_eq!(diagnostic_code(&err), Some("ERR_PNPM_MISSING_SHELL_NAME".to_string()));
}

#[test]
fn empty_shell_errors_like_pnpm() {
    let err = shell_from_args(Some(" \n"), &[]).expect_err("blank shell rejected");
    assert_eq!(err.to_string(), "`pacquet completion` requires a shell name");
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
        assert!(script.contains("pacquet completion-server"), "{script}");
    }
}
