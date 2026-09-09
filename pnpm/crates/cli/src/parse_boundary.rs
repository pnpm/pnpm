//! Where pnpm stops parsing argv and starts forwarding it.
//!
//! Every pass that rewrites argv before clap sees it — [`ConfigOverrides::extract`]
//! and [`expand_universal_shorthands`] — has to agree on this, or a token
//! meant for a script gets claimed or rewritten on the way through
//! (pnpm/pnpm#13302).
//!
//! [`ConfigOverrides::extract`]: crate::config_overrides::ConfigOverrides::extract
//! [`expand_universal_shorthands`]: crate::shorthands::expand_universal_shorthands

use crate::{cli_args::grammar, flag_relocation::ArgTable};
use clap::Command;
use std::{collections::HashSet, ffi::OsString};

/// Commands whose own arguments are unconditionally a foreign command line.
///
/// pnpm's `SPECIALLY_ESCAPED_CMDS` is `run` / `dlx` / `with`; `exec` joins
/// them here because its argv shape reaches the same place. `with` is
/// handled separately in [`command_boundary`], because only some of its
/// invocations forward their arguments to another process.
const COMMANDS_TAKING_A_FOREIGN_COMMAND_LINE: [&str; 3] = ["run", "exec", "dlx"];

/// Commands that stand for one named script, whose own arguments are that
/// script's command line.
///
/// pnpm has no command by these names: they reach `run` through the
/// `pnpm <script>` fallback, so the boundary is the token right after the
/// command name — there is no script name to skip, unlike `run`.
const SCRIPT_SHORTCUTS: [&str; 3] = ["test", "start", "stop"];

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
    match (separator, command_boundary(argv).map(|boundary| boundary.index)) {
        (Some(separator), Some(command)) => Some(separator.min(command)),
        (separator, command) => separator.or(command),
    }
}

/// Where a command's forwarded arguments begin, and whether that command
/// is a script shortcut — the one shape whose arguments can *open* on a
/// `--`, since no script name precedes them.
#[derive(Clone, Copy)]
pub(crate) struct CommandBoundary {
    pub(crate) index: usize,
    pub(crate) is_script_shortcut: bool,
}

/// The boundary implied by the command alone, ignoring any `--`.
pub(crate) fn command_boundary(argv: &[OsString]) -> Option<CommandBoundary> {
    let command = grammar();
    let arity = union_arity();
    let mut index = 1;
    let mut prefix_allowed = true;
    while index < argv.len() {
        // Non-UTF-8 cannot be classified, so treat it as the child's.
        let Some(arg) = argv[index].to_str() else {
            return Some(CommandBoundary { index, is_script_shortcut: false });
        };
        if arg == "--" {
            // The separator governs from here; see `passthrough_from`.
            return None;
        }
        if let Some(width) = option_width(arg, next_token(argv, index), arity) {
            index += width;
            continue;
        }
        // A positional.
        if is_command_prefix(arg, prefix_allowed) {
            prefix_allowed = false;
            index += 1;
            continue;
        }
        match subcommand_scan(argv, index, (command, arg), arity)? {
            SubcommandScan::Boundary(boundary) => return Some(boundary),
            SubcommandScan::PnpmOwned => return None,
            SubcommandScan::Resume(resume_at) => {
                index = resume_at;
                prefix_allowed = true;
            }
        }
    }
    None
}

/// A prefix word such as `pnpm pm <cmd>`, which stands ahead of the
/// command it qualifies and only ahead of the first one.
fn is_command_prefix(arg: &str, prefix_allowed: bool) -> bool {
    prefix_allowed && COMMAND_PREFIXES.contains(&arg)
}

/// What the scan finds at a known subcommand.
enum SubcommandScan {
    /// The command's forwarded arguments begin here.
    Boundary(CommandBoundary),
    /// The command parses its own arguments, so pnpm owns the rest.
    PnpmOwned,
    /// Resume the scan at this index.
    Resume(usize),
}

fn subcommand_scan(
    argv: &[OsString],
    index: usize,
    (command, arg): (&Command, &str),
    arity: &ArgTable,
) -> Option<SubcommandScan> {
    let Some(subcommand) = matching_subcommand(command, arg) else {
        // Names no command: the `pnpm <script>` fallback.
        return Some(SubcommandScan::Boundary(CommandBoundary {
            index: index + 1,
            is_script_shortcut: false,
        }));
    };
    let name = subcommand.get_name();
    if COMMANDS_TAKING_A_FOREIGN_COMMAND_LINE.contains(&name) {
        let index = next_positional(argv, index + 1, arity)? + 1;
        return Some(SubcommandScan::Boundary(CommandBoundary {
            index,
            is_script_shortcut: false,
        }));
    }
    if SCRIPT_SHORTCUTS.contains(&name) {
        return Some(SubcommandScan::Boundary(CommandBoundary {
            index: index + 1,
            is_script_shortcut: true,
        }));
    }
    if name != "with" {
        return Some(SubcommandScan::PnpmOwned);
    }
    // `with` splits on its version. Any version but `current` execs a child
    // pnpm, whose command line is its own, so stripping an override there
    // would lose it.
    let version = next_positional(argv, index + 1, arity)?;
    if argv[version] != "current" {
        return Some(SubcommandScan::Boundary(CommandBoundary {
            index: version + 1,
            is_script_shortcut: false,
        }));
    }
    // `with current` is spliced into pnpm's own argv by
    // [`crate::with_current::rewrite`], which runs after the pre-clap
    // passes. So resume the scan just past it: whatever boundary the nested
    // command line has is the one that will apply, and pnpm still owns
    // everything ahead of it.
    Some(SubcommandScan::Resume(version + 1))
}

/// The long options — including aliases — that the command named in `argv`
/// declares itself.
///
/// A setting spelled like one of them belongs to the command: nopt merges
/// the universal setting table with the invoked command's own table, and
/// the command's entry is the one its handler reads (`pnpm clean
/// --lockfile` removes lockfiles rather than turning the `lockfile`
/// setting on). Empty when argv names no known command.
///
/// The command is the one pnpm resolves, which is `add` for an `install`
/// given a package name — see [`install_as_add`]. That rewrite runs after
/// the settings are extracted, so resolving here the same way is what
/// keeps `pnpm install <pkg> --offline` from reaching `add`'s grammar with
/// an option only `install` declares.
///
/// [`install_as_add`]: crate::install_as_add
pub(crate) fn subcommand_option_names(argv: &[OsString]) -> HashSet<&'static str> {
    let command = grammar();
    let arity = union_arity();
    let mut index = 1;
    while let Some(positional) = next_positional(argv, index, arity) {
        let Some(name) = argv[positional].to_str().filter(|name| *name != "--") else {
            break;
        };
        if COMMAND_PREFIXES.contains(&name) {
            index = positional + 1;
            continue;
        }
        let Some(mut subcommand) = matching_subcommand(command, name) else {
            break;
        };
        if subcommand.get_name() == "install" && names_a_package(argv, positional + 1, arity) {
            subcommand = matching_subcommand(command, "add").expect("`add` is a subcommand");
        }
        return subcommand
            .get_arguments()
            .flat_map(|arg| {
                arg.get_long().into_iter().chain(arg.get_all_aliases().into_iter().flatten())
            })
            .collect();
    }
    HashSet::new()
}

/// The arity view the pre-clap passes share: the top-level grammar with
/// every subcommand's options folded in, because the command is not known
/// yet. Both halves of the union — a global lives on the top-level
/// command, a command's own options on its subcommand — and both are
/// derived from the static [`grammar`], so the table is built once.
pub(crate) fn union_arity() -> &'static ArgTable {
    static ARITY: std::sync::OnceLock<ArgTable> = std::sync::OnceLock::new();
    ARITY.get_or_init(|| {
        let command = grammar();
        let mut arity = ArgTable::top_level(command);
        arity.absorb_subcommands(command);
        arity
    })
}

/// Whether a positional follows `from`. nopt strips the `--` terminator
/// and treats what follows as positionals, so `pnpm install -- <pkg>`
/// names a package too.
fn names_a_package(argv: &[OsString], from: usize, arity: &ArgTable) -> bool {
    next_positional(argv, from, arity)
        .is_some_and(|index| argv[index] != "--" || index + 1 < argv.len())
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
        match option_width(arg, next_token(argv, index), arity) {
            Some(width) => index += width,
            None => return Some(index),
        }
    }
    None
}

/// The token after the one at `index`, as far as it can be classified.
fn next_token(argv: &[OsString], index: usize) -> Option<&str> {
    argv.get(index + 1).and_then(|token| token.to_str())
}

/// The number of argv slots `arg` occupies when it is an option, or `None`
/// when it is a positional. `next` is the token after it, which decides
/// the width of a bare boolean setting flag.
///
/// Arity comes from clap rather than a hand-listed set of option names: a
/// value-taking option missing from such a list makes its *value* look like
/// a positional, which lands the boundary on the wrong token
/// (`pnpm dlx --package cowsay --silent cowsay` would forward pnpm's own
/// `--silent`). The union across subcommands is what the pre-clap passes
/// have to work with, since the command is not yet known.
pub(crate) fn option_width(arg: &str, next: Option<&str>, arity: &ArgTable) -> Option<usize> {
    let rest = arg.strip_prefix('-').filter(|rest| !rest.is_empty())?;
    if let Some(long) = rest.strip_prefix('-') {
        return Some(long_option_width(long, next, arity));
    }
    let short = rest.chars().next().expect("checked non-empty");
    let is_bare_short = rest.chars().count() == 1;
    let consumes_value = arity.short_consumes_value(short).unwrap_or(false);
    Some(if consumes_value && is_bare_short { 2 } else { 1 })
}

/// [`option_width`] for a `--long` option, given the token after it and
/// the option name with its `--` already stripped.
fn long_option_width(long: &str, next: Option<&str>, arity: &ArgTable) -> usize {
    // `--config.<key>=<value>` is always self-contained.
    if long.starts_with("config.") {
        return 1;
    }
    // An inline value is self-contained whatever the option's arity.
    let Some(name) = long.split_once('=').map_or(Some(long), |_| None) else {
        return 1;
    };
    // A setting spelled as a bare flag is stripped by
    // [`ConfigOverrides::extract`], which computes this boundary — so clap
    // declares no arity for it and the setting table answers instead, with
    // the same next-token rule extraction applies.
    if crate::config_overrides::bare_boolean_setting_claims(name, next) {
        return 2;
    }
    // Clap goes first for every other shape: a command that declares the
    // same option owns it, there and here.
    match arity.long_consumes_value(name) {
        Some(consumes_value) => 1 + usize::from(consumes_value),
        None => crate::config_overrides::bare_setting_flag_width(name, next),
    }
}

/// The subcommand `name` resolves to, whether by its own name or an alias.
///
/// Takes the already-built `command`: assembling the clap tree is the
/// expensive part of this module, and one boundary computation must not pay
/// for it more than once.
fn matching_subcommand<'a>(command: &'a Command, name: &str) -> Option<&'a Command> {
    command
        .get_subcommands()
        .find(|sub| sub.get_name() == name || sub.get_all_aliases().any(|alias| alias == name))
}

#[cfg(test)]
mod tests;
