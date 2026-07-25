//! Where pnpm stops parsing argv and starts forwarding it.
//!
//! Every pass that rewrites argv before clap sees it — [`ConfigOverrides::extract`]
//! and [`expand_universal_shorthands`] — has to agree on this, or a token
//! meant for a script gets claimed or rewritten on the way through
//! (pnpm/pnpm#13302).
//!
//! [`ConfigOverrides::extract`]: crate::config_overrides::ConfigOverrides::extract
//! [`expand_universal_shorthands`]: crate::shorthands::expand_universal_shorthands

use crate::{cli_args::CliArgs, flag_relocation::ArgTable};
use clap::CommandFactory;
use std::ffi::OsString;

/// Commands whose own arguments are a foreign command line.
///
/// pnpm's `SPECIALLY_ESCAPED_CMDS` is `run` / `dlx` / `with`; `exec` joins
/// them here because its argv shape reaches the same place. `with` does
/// *not*: [`crate::with_current::rewrite`] runs after
/// [`ConfigOverrides::extract`] and splices the inner command line into
/// pnpm's own argv, so at this point those tokens are still pnpm's to
/// claim — `pnpm with current run --config.foo=bar build` must have the
/// override stripped here, or clap sees it after the rewrite and takes it
/// for the script name.
///
/// [`ConfigOverrides::extract`]: crate::config_overrides::ConfigOverrides::extract
const COMMANDS_TAKING_A_FOREIGN_COMMAND_LINE: [&str; 3] = ["run", "exec", "dlx"];

/// Commands that prefix another command rather than taking arguments of
/// their own, so the command to classify is the positional after them.
const COMMAND_PREFIXES: [&str; 3] = ["recursive", "multi", "m"];

/// The first index of `argv` that must reach the child untouched, or `None`
/// when pnpm owns every token.
///
/// Three ways to reach it, all of which pnpm honors:
///
/// - an explicit `--`;
/// - the `pnpm <script>` fallback, where the first positional names no
///   known command;
/// - a command taking a foreign command line, where the boundary is the
///   token after the script or command name it is given.
pub(crate) fn passthrough_from(argv: &[OsString]) -> Option<usize> {
    // Scanned independently of the command: a separator ends parsing even
    // for a command that would otherwise own the rest of argv, as in
    // `pnpm install -- --config.foo=bar`.
    let separator = argv.iter().position(|arg| arg == "--").map(|index| index + 1);
    match (separator, command_boundary(argv)) {
        (Some(separator), Some(command)) => Some(separator.min(command)),
        (separator, command) => separator.or(command),
    }
}

/// The boundary implied by the command alone, ignoring any `--`.
fn command_boundary(argv: &[OsString]) -> Option<usize> {
    let command = CliArgs::command();
    // Both halves of the union: a global lives on the top-level command,
    // a command's own options on its subcommand.
    let mut arity = ArgTable::top_level(&command);
    arity.absorb_subcommands(&command);
    let mut index = 1;
    let mut prefix_allowed = true;
    while index < argv.len() {
        // Non-UTF-8 cannot be classified, so treat it as the child's.
        let Some(arg) = argv[index].to_str() else {
            return Some(index);
        };
        if arg == "--" {
            // The separator governs from here; see `passthrough_from`.
            return None;
        }
        if let Some(width) = option_width(arg, &arity) {
            index += width;
            continue;
        }
        // A positional.
        if prefix_allowed && COMMAND_PREFIXES.contains(&arg) {
            prefix_allowed = false;
            index += 1;
            continue;
        }
        if takes_a_foreign_command_line(arg) {
            return Some(next_positional(argv, index + 1, &arity)? + 1);
        }
        if !is_known_top_level_command(arg) {
            return Some(index + 1);
        }
        // A known command that parses its own arguments: pnpm owns the rest.
        return None;
    }
    None
}

/// The index of the first positional at or after `from`, or `None` when the
/// command was given no positional at all.
fn next_positional(argv: &[OsString], from: usize, arity: &ArgTable) -> Option<usize> {
    let mut index = from;
    while index < argv.len() {
        let Some(arg) = argv[index].to_str() else {
            return Some(index);
        };
        if arg == "--" {
            // The separator already ends parsing; nothing after it needs a
            // script name to anchor on.
            return Some(index);
        }
        match option_width(arg, arity) {
            Some(width) => index += width,
            None => return Some(index),
        }
    }
    None
}

/// The number of argv slots `arg` occupies when it is an option, or `None`
/// when it is a positional.
///
/// Arity comes from clap rather than a hand-listed set of option names: a
/// value-taking option missing from such a list makes its *value* look like
/// a positional, which lands the boundary on the wrong token
/// (`pnpm dlx --package cowsay --silent cowsay` would forward pnpm's own
/// `--silent`). The union across subcommands is what the pre-clap passes
/// have to work with, since the command is not yet known.
fn option_width(arg: &str, arity: &ArgTable) -> Option<usize> {
    let rest = arg.strip_prefix('-').filter(|rest| !rest.is_empty())?;
    if let Some(long) = rest.strip_prefix('-') {
        // `--config.<key>=<value>` is always self-contained.
        if long.starts_with("config.") {
            return Some(1);
        }
        let (name, has_inline_value) =
            long.split_once('=').map_or((long, false), |(name, _)| (name, true));
        let consumes_value = arity.long_consumes_value(name).unwrap_or(false);
        return Some(if consumes_value && !has_inline_value { 2 } else { 1 });
    }
    let short = rest.chars().next().expect("checked non-empty");
    let is_bare_short = rest.chars().count() == 1;
    let consumes_value = arity.short_consumes_value(short).unwrap_or(false);
    Some(if consumes_value && is_bare_short { 2 } else { 1 })
}

fn takes_a_foreign_command_line(name: &str) -> bool {
    resolves_to_any(name, &COMMANDS_TAKING_A_FOREIGN_COMMAND_LINE)
}

pub(crate) fn is_known_top_level_command(name: &str) -> bool {
    CliArgs::command().get_subcommands().any(|command| resolves_to(command, name))
}

/// Whether `name` is one of `canonical_names`, or an alias of one.
fn resolves_to_any(name: &str, canonical_names: &[&str]) -> bool {
    CliArgs::command()
        .get_subcommands()
        .any(|command| canonical_names.contains(&command.get_name()) && resolves_to(command, name))
}

fn resolves_to(command: &clap::Command, name: &str) -> bool {
    command.get_name() == name || command.get_all_aliases().any(|alias| alias == name)
}

#[cfg(test)]
mod tests;
