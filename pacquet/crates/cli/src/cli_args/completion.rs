use clap::{Arg, Args, Command, CommandFactory};
use derive_more::{Display, Error};
use miette::Diagnostic;
use std::io::Write;

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

impl CompletionArgs {
    pub fn run(self) -> miette::Result<()> {
        let shell = shell_from_args(self.shell.as_deref(), &self.extra)?;
        generate_completion(shell, &mut std::io::stdout());
        Ok(())
    }
}

pub fn generate_completion(shell: CompletionShell, output: &mut dyn Write) {
    let mut command = command_for_completion_generation();
    clap_complete::generate(shell.to_clap_shell(), &mut command, "pacquet", output);
}

fn command_for_completion_generation() -> Command {
    super::CliArgs::command().mut_subcommands(|subcommand| {
        if subcommand.get_name() == "completion" {
            Command::new("completion")
                .about("Print shell completion code to stdout.")
                .arg(Arg::new("shell").value_name("SHELL"))
        } else {
            subcommand
        }
    })
}

#[cfg(test)]
mod tests;
