#![cfg_attr(
    not(test),
    expect(dead_code, reason = "Task 1 adds the parser before Task 2 registers the command.")
)]

use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;

pub const SUPPORTED_SHELLS: &[&str] = &["bash", "fish", "pwsh", "zsh"];

#[derive(Debug, Args)]
pub struct CompletionArgs {
    pub shell: Option<String>,

    #[clap(hide = true, trailing_var_arg = true, allow_hyphen_values = true)]
    pub extra: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionShell {
    Bash,
    Fish,
    Pwsh,
    Zsh,
}

impl CompletionShell {
    pub fn to_clap_shell(self) -> clap_complete::Shell {
        match self {
            CompletionShell::Bash => clap_complete::Shell::Bash,
            CompletionShell::Fish => clap_complete::Shell::Fish,
            CompletionShell::Pwsh => clap_complete::Shell::PowerShell,
            CompletionShell::Zsh => clap_complete::Shell::Zsh,
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "bash" => Some(CompletionShell::Bash),
            "fish" => Some(CompletionShell::Fish),
            "pwsh" => Some(CompletionShell::Pwsh),
            "zsh" => Some(CompletionShell::Zsh),
            _ => None,
        }
    }
}

#[derive(Debug, Display, Error, Diagnostic, PartialEq, Eq)]
#[non_exhaustive]
pub enum CompletionError {
    #[display("`pacquet completion` requires a shell name")]
    #[diagnostic(code(ERR_PNPM_MISSING_SHELL_NAME))]
    MissingShellName,

    #[display("'{shell}' is not supported")]
    #[diagnostic(code(ERR_PNPM_UNSUPPORTED_SHELL), help("Supported shells are: {}", SUPPORTED_SHELLS.join(", ")))]
    UnsupportedShell { shell: String },

    #[display("The {count} parameters after shell is not necessary")]
    #[diagnostic(code(ERR_PNPM_REDUNDANT_PARAMETERS))]
    RedundantParameters { count: usize },
}

pub fn shell_from_args(
    shell: Option<&str>,
    extra: &[String],
) -> Result<CompletionShell, CompletionError> {
    if !extra.is_empty() {
        return Err(CompletionError::RedundantParameters { count: extra.len() });
    }

    let Some(shell) = shell.map(str::trim).filter(|shell| !shell.is_empty()) else {
        return Err(CompletionError::MissingShellName);
    };

    CompletionShell::from_name(shell)
        .ok_or_else(|| CompletionError::UnsupportedShell { shell: shell.to_string() })
}

#[cfg(test)]
mod tests {
    use miette::Diagnostic as _;

    use super::{CompletionShell, shell_from_args};

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn missing_shell_errors_like_pnpm() {
        let err = shell_from_args(None, &[]).expect_err("missing shell rejected");
        assert_eq!(err.to_string(), "`pacquet completion` requires a shell name");
    }

    #[test]
    fn empty_shell_errors_like_pnpm() {
        let err = shell_from_args(Some(" \n"), &[]).expect_err("blank shell rejected");
        assert_eq!(err.to_string(), "`pacquet completion` requires a shell name");
    }

    #[test]
    fn unsupported_shell_lists_supported_shells() {
        let err = shell_from_args(Some("elvish"), &[]).expect_err("unsupported shell rejected");
        assert_eq!(err.to_string(), "'elvish' is not supported");
        assert_eq!(
            err.help().map(|help| help.to_string()),
            Some("Supported shells are: bash, fish, pwsh, zsh".to_string())
        );
    }

    #[test]
    fn redundant_parameters_are_rejected_before_shell_validation() {
        let extra = strings(&["fish", "pwsh", "zsh"]);
        let err = shell_from_args(None, &extra).expect_err("redundant params rejected");
        assert_eq!(err.to_string(), "The 3 parameters after shell is not necessary");
    }

    #[test]
    fn supported_shells_parse_after_trimming() {
        assert_eq!(shell_from_args(Some(" bash\n"), &[]).expect("bash"), CompletionShell::Bash);
        assert_eq!(shell_from_args(Some("fish"), &[]).expect("fish"), CompletionShell::Fish);
        assert_eq!(shell_from_args(Some("pwsh"), &[]).expect("pwsh"), CompletionShell::Pwsh);
        assert_eq!(shell_from_args(Some("zsh"), &[]).expect("zsh"), CompletionShell::Zsh);
    }

    #[test]
    fn pwsh_maps_to_powershell_generator() {
        assert_eq!(CompletionShell::Pwsh.to_clap_shell(), clap_complete::Shell::PowerShell);
    }
}
