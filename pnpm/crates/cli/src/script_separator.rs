//! Preservation of the `--` that introduces a script's own arguments.
//!
//! pnpm hands every token after a script's name to the script, the `--`
//! included: `pnpm run build -- --flag` runs the `build` script with
//! `-- --flag` appended, so the program the script invokes reads `--flag`
//! as an operand rather than as one of its own options. clap instead
//! treats that `--` as its escape token and drops it, which quietly
//! changes what the script receives — `node -e … --flag` fails with
//! "bad option" where `node -e … -- --flag` succeeds.
//!
//! [`preserve_script_separator`] writes a second `--` in that position so
//! the token clap consumes is the one this function added, leaving the
//! user's own separator to reach the script.
//!
//! Only the *first* argument token needs this. Once a value has started
//! filling the script's trailing arguments, clap keeps every later `--`
//! as an ordinary value, so `pnpm run build x -- y` already matches pnpm.

use crate::flag_relocation::{ArgTable, token_width};
use clap::Command;
use std::ffi::OsString;

/// Commands whose trailing tokens are a script's argv. `test` and `start`
/// belong to the same family but declare no trailing arguments yet, so
/// they have no separator to preserve.
const SCRIPT_COMMANDS: [&str; 3] = ["run", "stop", "restart"];

/// The subcommands after which the script *name* is still to come, so the
/// argument region starts one positional later.
const TAKES_SCRIPT_NAME: [&str; 1] = ["run"];

pub fn preserve_script_separator(cmd: &Command, mut argv: Vec<OsString>) -> Vec<OsString> {
    let top_level = ArgTable::top_level(cmd);
    let subcommand_union = ArgTable::subcommand_union(cmd);

    let Some(subcommand_index) = next_token(&argv, 1, &top_level, &subcommand_union) else {
        return argv;
    };
    let Some(subcommand) = argv[subcommand_index].to_str() else {
        return argv;
    };
    if !SCRIPT_COMMANDS.contains(&subcommand) {
        return argv;
    }

    let mut index = subcommand_index + 1;
    if TAKES_SCRIPT_NAME.contains(&subcommand) {
        let Some(script_name_index) = next_token(&argv, index, &top_level, &subcommand_union)
            .filter(|&index| argv[index] != "--")
        else {
            return argv;
        };
        index = script_name_index + 1;
    }

    let Some(separator_index) = next_token(&argv, index, &top_level, &subcommand_union)
        .filter(|&index| argv[index] == "--")
    else {
        return argv;
    };
    argv.insert(separator_index, OsString::from("--"));
    argv
}

/// The index of the next token at or after `index` that is not an option
/// (nor an option's value), including a `--` terminator — which
/// [`crate::flag_relocation::find_positional`] deliberately reports as the
/// end of the positionals instead of returning.
fn next_token(
    argv: &[OsString],
    mut index: usize,
    top_level: &ArgTable,
    subcommand_union: &ArgTable,
) -> Option<usize> {
    loop {
        let token = argv.get(index)?.to_str()?;
        if token == "--" {
            return Some(index);
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
            return Some(index);
        }
    }
}

#[cfg(test)]
mod tests;
