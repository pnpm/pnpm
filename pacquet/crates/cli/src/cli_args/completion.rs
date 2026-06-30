use clap::{Arg, ArgAction, Args, Command, CommandFactory};
use derive_more::{Display, Error};
use miette::{Diagnostic, IntoDiagnostic};
use std::{io::Write, path::Path};

pub const SUPPORTED_SHELLS: &[&str] = &["bash", "fish", "pwsh", "zsh"];

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionShell {
    Bash,
    Fish,
    Pwsh,
    Zsh,
}

impl CompletionShell {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "bash" => Some(CompletionShell::Bash),
            "fish" => Some(CompletionShell::Fish),
            "pwsh" => Some(CompletionShell::Pwsh),
            "zsh" => Some(CompletionShell::Zsh),
            _ => None,
        }
    }

    fn script(self) -> &'static str {
        match self {
            CompletionShell::Bash => BASH_COMPLETION,
            CompletionShell::Fish => FISH_COMPLETION,
            CompletionShell::Pwsh => PWSH_COMPLETION,
            CompletionShell::Zsh => ZSH_COMPLETION,
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
    pub fn run(&self) -> miette::Result<()> {
        let shell = shell_from_args(self.shell.as_deref(), &self.extra)?;
        generate_completion(shell, &mut std::io::stdout())?;
        Ok(())
    }
}

impl CompletionServerArgs {
    pub fn run(&self) -> miette::Result<()> {
        for completion in complete_words(&self.words) {
            println!("{completion}");
        }
        Ok(())
    }
}

pub fn generate_completion(shell: CompletionShell, output: &mut dyn Write) -> miette::Result<()> {
    output.write_all(shell.script().as_bytes()).into_diagnostic()
}

pub fn complete_words(words: &[String]) -> Vec<String> {
    let words = words_without_binary(words);
    let (before_current, current_word) = split_current_word(&words);
    if before_current.iter().any(|word| word == "--") {
        return Vec::new();
    }

    let command = command_for_completion();
    let context = CompletionContext::new(&command, before_current);

    if let Some(values) = equals_option_values(&context, current_word) {
        return values;
    }

    if let Some(values) = option_values(&context, before_current) {
        return filter_by_prefix(values, current_word);
    }

    if current_word.starts_with('-') {
        return filter_by_prefix(visible_options(&context), current_word);
    }

    if context.command_name == Some("completion") {
        return filter_by_prefix(
            SUPPORTED_SHELLS.iter().map(|shell| (*shell).to_string()).collect(),
            current_word,
        );
    }

    filter_by_prefix(visible_subcommands(context.command), current_word)
}

struct CompletionContext<'a> {
    root: &'a Command,
    command: &'a Command,
    command_name: Option<&'a str>,
}

impl<'a> CompletionContext<'a> {
    fn new(root: &'a Command, words: &[String]) -> Self {
        let mut command = root;
        let mut command_name = None;
        let mut index = 0;

        while let Some(word) = words.get(index) {
            if let Some(subcommand) = command
                .get_subcommands()
                .find(|subcommand| !subcommand.is_hide_set() && command_matches(subcommand, word))
            {
                command = subcommand;
                command_name = Some(subcommand.get_name());
                index += 1;
                continue;
            }

            if word.starts_with('-') {
                if option_has_separate_value(word)
                    && find_option_argument_in_command(command, word)
                        .or_else(|| find_option_argument_in_command(root, word))
                        .is_some_and(argument_takes_value)
                {
                    index += 2;
                } else {
                    index += 1;
                }
                continue;
            }

            index += 1;
        }

        Self { root, command, command_name }
    }
}

fn command_for_completion() -> Command {
    super::CliArgs::command()
}

fn words_without_binary(words: &[String]) -> Vec<String> {
    let Some((first, rest)) = words.split_first() else {
        return Vec::new();
    };

    if Path::new(first)
        .file_stem()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "pacquet")
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
    command.get_name() == word || command.get_all_aliases().any(|alias| alias == word)
}

fn visible_subcommands(command: &Command) -> Vec<String> {
    command
        .get_subcommands()
        .filter(|subcommand| !subcommand.is_hide_set())
        .flat_map(|subcommand| {
            subcommand.get_name_and_visible_aliases().into_iter().map(String::from)
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
    candidates.into_iter().filter(|candidate| candidate.starts_with(prefix)).collect()
}

fn extend_visible_options(options: &mut Vec<String>, command: &Command) {
    for argument in command.get_arguments().filter(|argument| !argument.is_hide_set()) {
        if let Some(short) = argument.get_short() {
            options.push(format!("-{short}"));
        }
        if let Some(long) = argument.get_long() {
            options.push(format!("--{long}"));
        }
        if let Some(aliases) = argument.get_visible_aliases() {
            options.extend(aliases.into_iter().map(|alias| format!("--{alias}")));
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
    let option =
        words.last().filter(|word| word.starts_with('-') && option_has_separate_value(word))?;
    let argument = find_option_argument(context, option)?;
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

fn find_option_argument_in_command<'a>(command: &'a Command, option: &str) -> Option<&'a Arg> {
    command.get_arguments().find(|argument| argument_matches(argument, option))
}

fn argument_matches(argument: &Arg, option: &str) -> bool {
    if let Some(long) = option.strip_prefix("--") {
        let long = long.split_once('=').map_or(long, |(name, _)| name);
        return argument.get_long() == Some(long)
            || argument
                .get_all_aliases()
                .is_some_and(|aliases| aliases.into_iter().any(|alias| alias == long));
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

fn option_has_separate_value(option: &str) -> bool {
    !option.contains('=')
}

const BASH_COMPLETION: &str = r#"###-begin-pacquet-completion-###
_pacquet_completion() {
  local IFS=$'\n'
  COMPREPLY=($(COMP_LINE="$COMP_LINE" COMP_POINT="$COMP_POINT" SHELL=bash pacquet completion-server -- "${COMP_WORDS[@]}"))
}
complete -F _pacquet_completion pacquet
###-end-pacquet-completion-###
"#;

const FISH_COMPLETION: &str = r#"###-begin-pacquet-completion-###
function __pacquet_completion
  set -lx SHELL fish
  set -lx COMP_LINE (commandline -cp)
  set -lx COMP_POINT (string length -- $COMP_LINE)
  set -l tokens (commandline -opc)
  set -l current (commandline -ct)
  if test (count $tokens) -eq 0
    set -a tokens "$current"
  else if test "$tokens[-1]" != "$current"
    set -a tokens "$current"
  end
  pacquet completion-server -- $tokens
end
complete -c pacquet -f -a "(__pacquet_completion)"
###-end-pacquet-completion-###
"#;

const PWSH_COMPLETION: &str = r#"###-begin-pacquet-completion-###
Register-ArgumentCompleter -Native -CommandName pacquet -ScriptBlock {
  param($wordToComplete, $commandAst, $cursorPosition)
  $env:SHELL = "pwsh"
  $env:COMP_LINE = $commandAst.ToString()
  $env:COMP_POINT = $cursorPosition
  $elements = @($commandAst.CommandElements | ForEach-Object { $_.Extent.Text })
  if ($elements.Count -eq 0 -or $elements[-1] -ne $wordToComplete) {
    $elements += $wordToComplete
  }
  pacquet completion-server -- @elements
}
###-end-pacquet-completion-###
"#;

const ZSH_COMPLETION: &str = r#"#compdef pacquet
###-begin-pacquet-completion-###
_pacquet_completion() {
  local reply
  reply=("${(@f)$(COMP_CWORD=$((CURRENT-1)) COMP_LINE="$BUFFER" COMP_POINT="$CURSOR" SHELL=zsh pacquet completion-server -- "${words[@]}")}")
  _describe 'values' reply
}
compdef _pacquet_completion pacquet
###-end-pacquet-completion-###
"#;

#[cfg(test)]
mod tests;
