use super::{
    CompletionContext, filter_by_prefix, packages,
    paths::{self, PathCompletionKind},
};
use clap::{Arg, ArgAction, Command, ValueHint};
use miette::IntoDiagnostic;
use std::env;

pub(super) fn visible_options(context: &CompletionContext<'_>) -> Vec<String> {
    let mut options = Vec::new();
    extend_visible_options(&mut options, context.root);
    if context.command_name.is_some() {
        extend_visible_options(&mut options, context.command);
    }
    options.sort();
    options.dedup();
    options
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

pub(super) fn attached_option_values(
    context: &CompletionContext<'_>,
    current_word: &str,
) -> miette::Result<Option<Vec<String>>> {
    let Some((option_prefix, argument, value_prefix)) =
        attached_option_completion(context, current_word)
    else {
        return Ok(None);
    };
    let values = complete_argument_values(context, argument, value_prefix)?
        .into_iter()
        .map(|value| format!("{option_prefix}{value}"))
        .collect();
    Ok(Some(values))
}

fn attached_option_completion<'a>(
    context: &'a CompletionContext<'_>,
    current_word: &'a str,
) -> Option<(String, &'a Arg, &'a str)> {
    if current_word.starts_with("--") {
        let (option, value_prefix) = current_word.split_once('=')?;
        let argument = find_option_argument(context, option)?;
        return argument_takes_value(argument)
            .then(|| (format!("{option}="), argument, value_prefix));
    }
    let rest = current_word.strip_prefix('-')?;
    for (offset, short) in rest.char_indices() {
        let argument = find_short_option_argument(context, short)?;
        if !argument_takes_separate_value(argument) {
            continue;
        }
        let attached = &rest[offset + short.len_utf8()..];
        if attached.is_empty() {
            return None;
        }
        let option_end = 1 + offset + short.len_utf8();
        let (prefix, value_prefix) = match attached.strip_prefix('=') {
            Some(value_prefix) => (&current_word[..=option_end], value_prefix),
            None => (&current_word[..option_end], attached),
        };
        return Some((prefix.to_string(), argument, value_prefix));
    }
    None
}

pub(super) fn complete_argument_values(
    context: &CompletionContext<'_>,
    argument: &Arg,
    value_prefix: &str,
) -> miette::Result<Vec<String>> {
    if matches!(argument.get_id().as_str(), "filter" | "filter_prod") {
        return Ok(filter_by_prefix(packages::complete_packages(context)?, value_prefix));
    }
    match argument.get_value_hint() {
        ValueHint::DirPath => {
            let cwd = env::current_dir().into_diagnostic()?;
            Ok(paths::complete_paths(&cwd, value_prefix, PathCompletionKind::Directories))
        }
        ValueHint::FilePath | ValueHint::AnyPath => {
            let cwd = env::current_dir().into_diagnostic()?;
            Ok(paths::complete_paths(&cwd, value_prefix, PathCompletionKind::FilesAndDirectories))
        }
        _ => {
            let mut values = filter_by_prefix(visible_possible_values(argument), value_prefix);
            values.sort();
            values.dedup();
            Ok(values)
        }
    }
}

fn visible_possible_values(argument: &Arg) -> Vec<String> {
    argument
        .get_possible_values()
        .into_iter()
        .filter(|value| !value.is_hide_set())
        .map(|value| value.get_name().to_string())
        .collect()
}

pub(super) fn separate_value_option<'a>(
    context: &CompletionContext<'a>,
    word: &str,
    next: Option<&str>,
) -> Option<&'a Arg> {
    if word.contains('=') {
        return None;
    }
    find_option_argument(context, word)
        .filter(|argument| {
            argument_takes_separate_value(argument) && option_value_is_allowed(argument, next)
        })
}

pub(super) fn option_value_is_allowed(argument: &Arg, next: Option<&str>) -> bool {
    next.is_none_or(|value| {
        !value.starts_with('-') || value == "-" || argument.is_allow_hyphen_values_set()
    })
}

pub(super) fn find_option_argument<'a>(
    context: &CompletionContext<'a>,
    option: &str,
) -> Option<&'a Arg> {
    find_option_argument_in_command(context.command, option)
        .or_else(|| find_option_argument_in_command(context.root, option))
}

pub(super) fn find_short_option_argument<'a>(
    context: &CompletionContext<'a>,
    short: char,
) -> Option<&'a Arg> {
    find_short_in_command(context.command, short)
        .or_else(|| find_short_in_command(context.root, short))
}

fn find_short_in_command(command: &Command, short: char) -> Option<&Arg> {
    command
        .get_arguments()
        .find(|argument| argument.get_short() == Some(short))
}

fn find_option_argument_in_command<'a>(command: &'a Command, option: &str) -> Option<&'a Arg> {
    command.get_arguments().find(|argument| argument_matches(argument, option))
}

fn argument_matches(argument: &Arg, option: &str) -> bool {
    if let Some(long) = option.strip_prefix("--") {
        let long = match long.split_once('=') {
            Some((name, _)) => name,
            None => long,
        };
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

pub(super) fn argument_takes_separate_value(argument: &Arg) -> bool {
    argument_takes_value(argument) && !argument.is_require_equals_set()
}
