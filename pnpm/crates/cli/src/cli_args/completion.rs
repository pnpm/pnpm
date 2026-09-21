pub use shells::{CompletionShell, SUPPORTED_SHELLS};

use crate::{
    cli_args::{cli_command::options::find_workspace_root_dir, prefix::find_npm_local_prefix},
    flag_relocation::short_cluster_consumes_value,
};
use clap::{Arg, ArgAction, Args, Command, CommandFactory};
use derive_more::{Display, Error};
use miette::{Diagnostic, IntoDiagnostic};
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
        generate_completion(shell, &mut std::io::stdout())?;
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
            println!("{completion}");
        }
        Ok(())
    }
}

pub fn generate_completion(shell: CompletionShell, output: &mut dyn Write) -> miette::Result<()> {
    output
        .write_all(shell.script().as_bytes())
        .into_diagnostic()
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

    if let Some(values) = equals_option_values(&context, current_word) {
        return Ok(values);
    }

    if let Some(values) = option_values(&context, before_current) {
        return Ok(filter_by_prefix(values, current_word));
    }

    if current_word.starts_with('-') {
        return Ok(filter_by_prefix(visible_options(&context), current_word));
    }

    if context.awaiting_option_value
        && before_current
            .last()
            .is_some_and(|word| matches!(word.as_str(), "--filter" | "-F"))
    {
        return Ok(filter_by_prefix(packages::complete_packages(&context)?, current_word));
    }

    Ok(filter_by_prefix(context.positional_candidates()?, current_word))
}

struct CompletionContext<'a> {
    root: &'a Command,
    command: &'a Command,
    command_name: Option<&'a str>,
    has_positional: bool,
    directory: Option<&'a str>,
    awaiting_option_value: bool,
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

    fn scan_option(&mut self, word: &'a str, next: Option<&'a str>) -> usize {
        if let Some(rest) = word
            .strip_prefix('-')
            .filter(|rest| !rest.starts_with('-'))
        {
            return self.scan_short_options(rest, next);
        }
        self.workspace_root |= word == "--workspace-root";
        let width = option_word_width(self, word, next);
        if let Some(directory) = scripts::directory_option(word, next.filter(|_| width == 2)) {
            self.directory = Some(directory);
        }
        width
    }

    fn scan_short_options(&mut self, rest: &'a str, next: Option<&'a str>) -> usize {
        let mut remaining = rest;
        let mut accepts_next = false;
        let consumes_value = short_cluster_consumes_value(rest, |short| {
            remaining = remaining.strip_prefix(short).expect("scanner visits each short in order");
            let argument = find_short_option_argument(self.command, short)
                .or_else(|| find_short_option_argument(self.root, short))?;
            self.workspace_root |= argument.get_id() == "workspace_root";
            accepts_next = option_value_is_allowed(argument, next);
            if argument.get_id() == "dir" {
                self.directory = short_option_value(remaining, next, accepts_next);
            }
            Some(argument_takes_separate_value(argument))
        });
        if consumes_value && accepts_next { 2 } else { 1 }
    }

    fn new(root: &'a Command, words: &'a [String]) -> Self {
        let mut context = Self {
            root,
            command: root,
            command_name: None,
            has_positional: false,
            directory: None,
            awaiting_option_value: false,
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
                let width = context.scan_option(word, words.get(index + 1).map(String::as_str));
                context.awaiting_option_value = width == 2 && index + 1 == words.len();
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

fn option_word_width(context: &CompletionContext<'_>, word: &str, next: Option<&str>) -> usize {
    let takes_separate_value = option_has_separate_value(word)
        && find_option_argument(context, word)
            .is_some_and(|argument| {
                argument_takes_separate_value(argument) && option_value_is_allowed(argument, next)
            });
    if takes_separate_value { 2 } else { 1 }
}

fn option_value_is_allowed(argument: &Arg, next: Option<&str>) -> bool {
    next.is_none_or(|value| {
        !value.starts_with('-') || value == "-" || argument.is_allow_hyphen_values_set()
    })
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

fn visible_options(context: &CompletionContext<'_>) -> Vec<String> {
    let mut options = Vec::new();
    extend_visible_options(&mut options, context.root);
    if context.command_name.is_some() {
        extend_visible_options(&mut options, context.command);
    }
    options.sort();
    options.dedup();
    options
}

fn filter_by_prefix(candidates: Vec<String>, prefix: &str) -> Vec<String> {
    candidates
        .into_iter()
        .filter(|candidate| candidate.starts_with(prefix))
        .collect()
}

fn extend_visible_options(options: &mut Vec<String>, command: &Command) {
    for argument in command
        .get_arguments()
        .filter(|argument| !argument.is_hide_set())
    {
        if let Some(short) = argument.get_short() {
            options.push(format!("-{short}"));
        }
        if let Some(long) = argument.get_long() {
            options.push(format!("--{long}"));
        }
        if let Some(aliases) = argument.get_visible_aliases() {
            options.extend(
                aliases
                    .into_iter()
                    .map(|alias| format!("--{alias}")),
            );
        }
    }
}

fn equals_option_values(
    context: &CompletionContext<'_>,
    current_word: &str,
) -> Option<Vec<String>> {
    let (option, value_prefix) = current_word.split_once('=')?;
    if !option.starts_with('-') {
        return None;
    }

    let argument = find_option_argument(context, option)?;
    let mut values: Vec<_> = visible_possible_values(argument)
        .into_iter()
        .filter(|value| value.starts_with(value_prefix))
        .map(|value| format!("{option}={value}"))
        .collect();

    values.sort();
    values.dedup();
    Some(values)
}

fn option_values(context: &CompletionContext<'_>, words: &[String]) -> Option<Vec<String>> {
    let option = words
        .last()
        .filter(|word| word.starts_with('-') && option_has_separate_value(word))?;
    let argument = find_option_argument(context, option)?;
    if !argument_takes_separate_value(argument) {
        return None;
    }
    let mut values = visible_possible_values(argument);

    if values.is_empty() {
        return None;
    }

    values.sort();
    values.dedup();
    Some(values)
}

fn visible_possible_values(argument: &Arg) -> Vec<String> {
    argument
        .get_possible_values()
        .into_iter()
        .filter(|value| !value.is_hide_set())
        .map(|value| value.get_name().to_string())
        .collect()
}

fn find_option_argument<'a>(context: &'a CompletionContext<'_>, option: &str) -> Option<&'a Arg> {
    find_option_argument_in_command(context.command, option)
        .or_else(|| find_option_argument_in_command(context.root, option))
}

fn find_short_option_argument(command: &Command, short: char) -> Option<&Arg> {
    command
        .get_arguments()
        .find(|argument| argument.get_short() == Some(short))
}

fn find_option_argument_in_command<'a>(command: &'a Command, option: &str) -> Option<&'a Arg> {
    command.get_arguments().find(|argument| argument_matches(argument, option))
}

fn argument_matches(argument: &Arg, option: &str) -> bool {
    if let Some(long) = option.strip_prefix("--") {
        let long = long.split_once('=').map_or(long, |(name, _)| name);
        return argument.get_long() == Some(long)
            || argument
                .get_all_aliases()
                .is_some_and(|aliases| {
                    aliases
                        .into_iter()
                        .any(|alias| alias == long)
                });
    }

    if let Some(short) = option.strip_prefix('-') {
        return short.len() == 1
            && argument
                .get_short()
                .is_some_and(|argument_short| short.starts_with(argument_short));
    }

    false
}

fn argument_takes_value(argument: &Arg) -> bool {
    argument.get_num_args().is_some_and(|range| range.takes_values())
        || matches!(argument.get_action(), ArgAction::Set | ArgAction::Append)
}

fn argument_takes_separate_value(argument: &Arg) -> bool {
    argument_takes_value(argument) && !argument.is_require_equals_set()
}

fn option_has_separate_value(option: &str) -> bool {
    !option.contains('=')
}

mod packages;
mod scripts;
mod shells;

#[cfg(test)]
mod tests;
