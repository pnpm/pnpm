//! Universal CLI shorthands.
//!
//! pnpm expands a table of universal shorthands over argv before parsing
//! (`pnpm11/pnpm/src/shorthands.ts`): `--silent` and `-s` both mean
//! `--reporter=silent` for every command. A command's own shorthand table
//! overrides the universal one — `run` redefines `s` as `--sequential` —
//! so `-s` is left untouched when the effective command is `run`,
//! including the `pnpm <script>` fallback, which resolves to `run` and
//! therefore inherits its shorthands. The long `--silent` form has no
//! per-command override anywhere, so it always expands.
//!
//! Only the universal shorthands whose expansion targets exist in pacquet
//! are handled here. The loglevel family (`-d`, `-q`, `--quiet`,
//! `--verbose`, ...) expands to `--loglevel=<level>`, which pacquet has
//! not grown yet.

use crate::flag_relocation::{ArgTable, find_positional, token_width};
use clap::Command;
use std::ffi::OsString;

/// Expand the universal shorthands in `argv`. Runs before
/// [`crate::flag_relocation::relocate_pre_subcommand_flags`], so tokens
/// may still appear ahead of the subcommand; option values are stepped
/// over with the same arity tables the relocation pass uses, so a value
/// that happens to spell `-s` (e.g. a `--filter` pattern) is never
/// rewritten. Everything after a `--` terminator is left untouched.
///
/// `cmd` must be the same [`Command`] the returned argv is parsed with.
pub fn expand_universal_shorthands(cmd: &Command, mut argv: Vec<OsString>) -> Vec<OsString> {
    let top_level = ArgTable::top_level(cmd);
    let subcommand_union = ArgTable::subcommand_union(cmd);
    let run_owns_short_s = effective_command_is_run(cmd, &argv, &top_level, &subcommand_union);

    let mut index = 1;
    while index < argv.len() {
        let Some(token) = argv[index].to_str() else {
            index += 1;
            continue;
        };
        if token == "--" {
            break;
        }
        if token == "--silent" || (token == "-s" && !run_owns_short_s) {
            argv[index] = OsString::from("--reporter=silent");
            index += 1;
            continue;
        }
        if let Some(rest) = token.strip_prefix("--") {
            let (name, has_inline_value) =
                rest.split_once('=').map_or((rest, false), |(name, _)| (name, true));
            let consumes_value = top_level
                .long_consumes_value(name)
                .or_else(|| subcommand_union.long_consumes_value(name))
                .unwrap_or(false);
            index += token_width(consumes_value, has_inline_value);
        } else if let Some(rest) = token.strip_prefix('-').filter(|rest| !rest.is_empty()) {
            let short = rest.chars().next().expect("checked non-empty");
            let is_bare_short = rest.chars().count() == 1;
            let consumes_value = top_level
                .short_consumes_value(short)
                .or_else(|| subcommand_union.short_consumes_value(short))
                .unwrap_or(false);
            index += token_width(consumes_value && is_bare_short, false);
        } else {
            index += 1;
        }
    }
    argv
}

/// Whether argv invokes `run` — directly, through an alias, through
/// `recursive run`, or through the `pnpm <script>` fallback (a positional
/// that names no known subcommand dispatches to `run`). Only `run` gives
/// `-s` a different meaning, so this is what gates the `-s` expansion.
fn effective_command_is_run(
    cmd: &Command,
    argv: &[OsString],
    top_level: &ArgTable,
    subcommand_union: &ArgTable,
) -> bool {
    let Some(mut index) = find_positional(argv, 1, top_level, subcommand_union) else {
        // No subcommand at all: nothing owns `-s`, the universal table wins.
        return false;
    };
    loop {
        let Some(subcommand) = cmd.find_subcommand(&argv[index]) else {
            // `pnpm <script>` fallback: dispatches to `run`.
            return true;
        };
        if subcommand.get_name() == "recursive" {
            // `pnpm recursive run ...`: the nested token names the command.
            match find_positional(argv, index + 1, top_level, subcommand_union) {
                Some(next) => index = next,
                None => return false,
            }
            continue;
        }
        return subcommand.get_name() == "run";
    }
}

#[cfg(test)]
mod tests;
