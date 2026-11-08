pub use shells::{CompletionShell, SUPPORTED_SHELLS};

use crate::{
    cli_args::{cli_command::options::find_workspace_root_dir, prefix::find_npm_local_prefix},
    flag_relocation::short_cluster_consumes_value,
};
use clap::{Arg, Args, Command, CommandFactory};
use derive_more::{Display, Error};
use miette::{Diagnostic, IntoDiagnostic};
use options::{
    argument_takes_separate_value, attached_option_values, complete_argument_values,
    find_short_option_argument, option_value_is_allowed, separate_value_option, visible_options,
};
use pnpm_text_sanitize::sanitize_inline;
use std::{
    env,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Args)]
pub struct CompletionArgs {
    pub shell: Option<String>,

    #[clap(hide = true, trailing_var_arg = true, allow_hyphen_values = true)]
    pub extra: Vec<String>,
}

#[derive(Debug, Args)]
pub struct CompletionServerArgs {
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    pub words: Vec<String>,
}

#[derive(Debug, Display, Error, Diagnostic, PartialEq, Eq)]
#[non_exhaustive]
pub enum CompletionError {
    #[display("`pnpm completion` requires a shell name")]
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

    let Some(shell) = shell
        .map(str::trim)
        .filter(|shell| !shell.is_empty())
    else {
        return Err(CompletionError::MissingShellName);
    };

    CompletionShell::from_name(shell)
        .ok_or_else(|| CompletionError::UnsupportedShell { shell: shell.to_string() })
}

impl CompletionArgs {
    pub fn run(&self) -> miette::Result<()> {
        let shell = shell_from_args(self.shell.as_deref(), &self.extra)?;
        pnpm_diagnostics::stdout::check(generate_completion(shell, &mut std::io::stdout()));
        Ok(())
    }
}

impl CompletionServerArgs {
    pub fn run(&self) -> miette::Result<()> {
        let is_zsh = std::env::var_os("SHELL").is_some_and(|shell| shell == "zsh");
        for completion in complete_words(&self.words)? {
            if sanitize_inline(&completion) != completion {
                continue;
            }
            let completion = if is_zsh {
                completion.replace('\\', r"\\").replace(':', r"\:")
            } else {
                completion
            };
            pnpm_diagnostics::println!("{completion}");
        }
        Ok(())
    }
}

pub fn generate_completion(shell: CompletionShell, output: &mut dyn Write) -> std::io::Result<()> {
    output.write_all(shell.script().as_bytes())
}

pub fn complete_words(words: &[String]) -> miette::Result<Vec<String>> {
    let words = words_without_binary(words);
    let (before_current, current_word) = split_current_word(&words);
    if before_current
        .iter()
        .any(|word| word == "--")
    {
        return Ok(Vec::new());
    }

    let command = command_for_completion();
    let context = CompletionContext::new(&command, before_current);

    if let Some(values) = attached_option_values(&context, current_word)? {
        return Ok(values);
    }

    if current_word.starts_with('-') {
        return Ok(filter_by_prefix(visible_options(&context), current_word));
    }

    if let Some(argument) = context.pending_option {
        return complete_argument_values(&context, argument, current_word);
    }

    Ok(filter_by_prefix(context.positional_candidates()?, current_word))
}

struct CompletionContext<'a> {
    root: &'a Command,
    command: &'a Command,
    command_name: Option<&'a str>,
    has_positional: bool,
    directory: Option<&'a str>,
    pending_option: Option<&'a Arg>,
    workspace_root: bool,
}

impl<'a> CompletionContext<'a> {
    fn resolve_project_directory(&self) -> miette::Result<PathBuf> {
        let cwd = env::current_dir().into_diagnostic()?;
        let directory = match self.directory {
            Some(directory) => cwd.join(directory),
            None => find_npm_local_prefix(&cwd)?,
        };
        Ok(if self.workspace_root { find_workspace_root_dir(&directory)? } else { directory })
    }

    fn positional_candidates(&self) -> miette::Result<Vec<String>> {
        match self.command_name {
            Some("completion") => Ok(SUPPORTED_SHELLS
                .iter()
                .map(|shell| (*shell).to_string())
                .collect()),
            Some("run") => scripts::complete_scripts(self),
            _ => Ok(visible_subcommands(self.command)),
        }
    }

    fn scan_option(&mut self, word: &'a str, next: Option<&'a str>) -> (usize, Option<&'a Arg>) {
        if let Some(rest) = word
            .strip_prefix('-')
            .filter(|rest| !rest.starts_with('-'))
        {
            return self.scan_short_options(rest, next);
        }
        self.workspace_root |= word == "--workspace-root";
        let pending_option = separate_value_option(self, word, next);
        let width = if pending_option.is_some() { 2 } else { 1 };
        if let Some(directory) = scripts::directory_option(word, next.filter(|_| width == 2)) {
            self.directory = Some(directory);
        }
        (width, pending_option)
    }

    fn scan_short_options(
        &mut self,
        rest: &'a str,
        next: Option<&'a str>,
    ) -> (usize, Option<&'a Arg>) {
        let mut remaining = rest;
        let mut pending_option = None;
        let consumes_value = short_cluster_consumes_value(rest, |short| {
            remaining = remaining.strip_prefix(short).expect("scanner visits each short in order");
            let argument = find_short_option_argument(self, short)?;
            self.workspace_root |= argument.get_id() == "workspace_root";
            let accepts_next = option_value_is_allowed(argument, next);
            if argument.get_id() == "dir" {
                self.directory = short_option_value(remaining, next, accepts_next);
            }
            let takes_separate = argument_takes_separate_value(argument);
            pending_option = (takes_separate && accepts_next).then_some(argument);
            Some(takes_separate)
        });
        if consumes_value && pending_option.is_some() { (2, pending_option) } else { (1, None) }
    }

    fn new(root: &'a Command, words: &'a [String]) -> Self {
        let mut context = Self {
            root,
            command: root,
            command_name: None,
            has_positional: false,
            directory: None,
            pending_option: None,
            workspace_root: false,
        };
        let mut index = 0;
        while let Some(word) = words.get(index) {
            if let Some(subcommand) = context.command
                .get_subcommands()
                .find(|subcommand| !subcommand.is_hide_set() && command_matches(subcommand, word))
            {
                context.command = subcommand;
                context.command_name = Some(subcommand.get_name());
                index += 1;
                continue;
            }
            if word.starts_with('-') {
                let (width, pending_option) =
                    context.scan_option(word, words.get(index + 1).map(String::as_str));
                context.pending_option =
                    (index + 1 == words.len()).then_some(pending_option).flatten();
                index += width;
                continue;
            }
            context.has_positional = true;
            index += 1;
        }
        context
    }
}

fn short_option_value<'a>(
    attached: &'a str,
    next: Option<&'a str>,
    accepts_next: bool,
) -> Option<&'a str> {
    if !attached.is_empty() {
        return Some(attached.strip_prefix('=').unwrap_or(attached));
    }
    if accepts_next { next } else { None }
}

fn command_for_completion() -> Command {
    let mut command = super::CliArgs::command();
    command.build();
    command
}

fn words_without_binary(words: &[String]) -> Vec<String> {
    let Some((first, rest)) = words.split_first() else {
        return Vec::new();
    };

    if Path::new(first)
        .file_stem()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, "pnpm" | "pn" | "pacquet"))
    {
        rest.to_vec()
    } else {
        words.to_vec()
    }
}

fn split_current_word(words: &[String]) -> (&[String], &str) {
    match words.split_last() {
        Some((current, before_current)) => (before_current, current.as_str()),
        None => (&[], ""),
    }
}

fn command_matches(command: &Command, word: &str) -> bool {
    command.get_name() == word
        || command
            .get_all_aliases()
            .any(|alias| alias == word)
}

fn visible_subcommands(command: &Command) -> Vec<String> {
    command
        .get_subcommands()
        .filter(|subcommand| !subcommand.is_hide_set())
        .flat_map(|subcommand| {
            subcommand
                .get_name_and_visible_aliases()
                .into_iter()
                .map(String::from)
        })
        .collect()
}

fn filter_by_prefix(candidates: Vec<String>, prefix: &str) -> Vec<String> {
    candidates
        .into_iter()
        .filter(|candidate| candidate.starts_with(prefix))
        .collect()
}

mod options;
mod packages;
mod paths;
mod scripts;
mod shells;

#[cfg(test)]
mod tests;
