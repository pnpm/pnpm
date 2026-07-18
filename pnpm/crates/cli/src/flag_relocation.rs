//! Position-independent placement of subcommand options.
//!
//! pnpm parses argv with `nopt`, which merges the universal option table
//! with the invoked command's table and accepts any option anywhere on
//! the command line: `pnpm --ignore-scripts --prod deploy <dir>` and
//! `pnpm deploy --ignore-scripts --prod <dir>` are the same invocation
//! (pnpm's release tooling relies on this — `bundle-deps.ts` passes
//! install flags ahead of `deploy`). Clap instead scopes options to the
//! level they are declared on, so an option owned by a subcommand aborts
//! the parse with "unexpected argument" when it appears before the
//! subcommand.
//!
//! [`relocate_pre_subcommand_flags`] closes the gap in argv space:
//! option tokens that appear before the subcommand and are not part of
//! the top-level grammar move to directly after the subcommand token
//! (relative order preserved), so clap parses them with the subcommand's
//! grammar exactly as if they had been written there. Whether a moved
//! option consumes the following token as its value is decided from the
//! union of every subcommand's arg table; on an arity conflict between
//! subcommands the option is treated as boolean so a subcommand name is
//! never swallowed as a value. Tokens move only when the first
//! positional token names a real subcommand — external commands
//! (`pnpm <script>`) keep their argv untouched, as does everything after
//! a `--` terminator.

use clap::{Arg, ArgAction, Command};
use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
};

/// Move pre-subcommand option tokens that belong to a subcommand's
/// grammar to directly after the subcommand token. See the module docs.
///
/// `cmd` must be the same [`Command`] the returned argv is parsed with
/// (including the [`crate::boolean_negations`] augmentation), so the
/// hidden `--no-<flag>` negations relocate like their positive forms.
fn find_positional(
    argv: &[OsString],
    mut index: usize,
    top_level: &ArgTable,
    subcommand_union: &ArgTable,
) -> Option<usize> {
    loop {
        let token = argv.get(index).and_then(|token| token.to_str())?;
        if token == "--" {
            return None;
        }
        if let Some(rest) = token.strip_prefix("--") {
            let (name, has_inline_value) =
                rest.split_once('=').map_or((rest, false), |(name, _)| (name, true));
            if let Some(consumes_value) = top_level.long_consumes_value(name) {
                index += token_width(consumes_value, has_inline_value);
            } else {
                let consumes_value = subcommand_union.long_consumes_value(name).unwrap_or(false);
                index += token_width(consumes_value, has_inline_value);
            }
        } else if let Some(rest) = token.strip_prefix('-').filter(|rest| !rest.is_empty()) {
            let short = rest.chars().next().expect("checked non-empty");
            let is_bare_short = rest.chars().count() == 1;
            if let Some(consumes_value) = top_level.short_consumes_value(short) {
                index += token_width(consumes_value && is_bare_short, false);
            } else {
                let consumes_value = subcommand_union.short_consumes_value(short).unwrap_or(false);
                index += token_width(consumes_value && is_bare_short, false);
            }
        } else {
            return Some(index);
        }
    }
}

/// Move pre-subcommand option tokens that belong to a subcommand's
/// grammar to directly after the subcommand token. See the module docs.
///
/// `cmd` must be the same [`Command`] the returned argv is parsed with
/// (including the [`crate::boolean_negations`] augmentation), so the
/// hidden `--no-<flag>` negations relocate like their positive forms.
pub fn relocate_pre_subcommand_flags(cmd: &Command, mut argv: Vec<OsString>) -> Vec<OsString> {
    let top_level = ArgTable::top_level(cmd);
    let subcommand_union = ArgTable::subcommand_union(cmd);

    let mut current_idx = 1;
    while let Some(pos_idx) = find_positional(&argv, current_idx, &top_level, &subcommand_union) {
        if let Some(token) = argv.get(pos_idx).and_then(|t| t.to_str())
            && matches!(token, "recursive" | "multi" | "m")
            && find_positional(&argv, pos_idx + 1, &top_level, &subcommand_union).is_some()
        {
            argv[pos_idx] = OsString::from("--recursive");
            current_idx = pos_idx + 1;
            continue;
        }
        break;
    }

    let mut moved_indexes: HashSet<usize> = HashSet::new();
    let subcommand_index = find_positional(&argv, 1, &top_level, &subcommand_union);
    let Some(subcommand_index) = subcommand_index else {
        return argv;
    };

    // Now we must re-calculate moved_indexes, because find_positional just skipped.
    let mut index = 1;
    while index < subcommand_index {
        let Some(token) = argv.get(index).and_then(|t| t.to_str()) else {
            break;
        };
        if token == "--" {
            break;
        }
        if let Some(rest) = token.strip_prefix("--") {
            let (name, has_inline_value) =
                rest.split_once('=').map_or((rest, false), |(name, _)| (name, true));
            if let Some(consumes_value) = top_level.long_consumes_value(name) {
                index += token_width(consumes_value, has_inline_value);
            } else {
                let consumes_value = subcommand_union.long_consumes_value(name).unwrap_or(false);
                let width = token_width(consumes_value, has_inline_value);
                for offset in 0..width.min(argv.len() - index) {
                    moved_indexes.insert(index + offset);
                }
                index += width;
            }
        } else if let Some(rest) = token.strip_prefix('-').filter(|rest| !rest.is_empty()) {
            let short = rest.chars().next().expect("checked non-empty");
            let is_bare_short = rest.chars().count() == 1;
            if let Some(consumes_value) = top_level.short_consumes_value(short) {
                index += token_width(consumes_value && is_bare_short, false);
            } else {
                let consumes_value = subcommand_union.short_consumes_value(short).unwrap_or(false);
                let width = token_width(consumes_value && is_bare_short, false);
                for offset in 0..width.min(argv.len() - index) {
                    moved_indexes.insert(index + offset);
                }
                index += width;
            }
        } else {
            break;
        }
    }

    if moved_indexes.is_empty() || cmd.find_subcommand(&argv[subcommand_index]).is_none() {
        return argv;
    }

    let mut result: Vec<OsString> = Vec::with_capacity(argv.len());
    let mut moved: Vec<OsString> = Vec::with_capacity(moved_indexes.len());
    for (token_index, token) in argv.into_iter().enumerate() {
        if moved_indexes.contains(&token_index) {
            moved.push(token);
        } else {
            result.push(token);
            if token_index == subcommand_index {
                result.append(&mut moved);
            }
        }
    }
    result
}

/// The number of argv tokens an option occupies: itself, plus its value
/// when the value is a separate token rather than `--flag=value` inline.
fn token_width(consumes_value: bool, has_inline_value: bool) -> usize {
    if consumes_value && !has_inline_value { 2 } else { 1 }
}

/// Option-name lookup table: long / short spelling → whether the option
/// consumes the next argv token as its value.
#[derive(Debug, Default)]
struct ArgTable {
    longs: HashMap<String, bool>,
    shorts: HashMap<char, bool>,
}

impl ArgTable {
    /// The top-level grammar: everything already valid before the
    /// subcommand, which therefore stays in place. Clap only adds the
    /// automatic `--help` / `-h` at build time, so they are seeded
    /// manually.
    fn top_level(cmd: &Command) -> Self {
        let mut table = Self::default();
        table.longs.insert("help".to_string(), false);
        table.shorts.insert('h', false);
        table.absorb(cmd.get_arguments());
        table
    }

    /// The union of every subcommand's args, used only to decide how
    /// many tokens a to-be-moved option occupies.
    fn subcommand_union(cmd: &Command) -> Self {
        let mut table = Self::default();
        table.absorb(cmd.get_subcommands().flat_map(Command::get_arguments));
        table
    }

    fn absorb<'a, Args: IntoIterator<Item = &'a Arg>>(&mut self, args: Args) {
        for arg in args {
            let consumes_value = matches!(arg.get_action(), ArgAction::Set | ArgAction::Append);
            for long in
                arg.get_long().into_iter().chain(arg.get_all_aliases().into_iter().flatten())
            {
                merge_arity(self.longs.entry(long.to_string()), consumes_value);
            }
            for short in
                arg.get_short().into_iter().chain(arg.get_all_short_aliases().into_iter().flatten())
            {
                merge_arity(self.shorts.entry(short), consumes_value);
            }
        }
    }

    fn long_consumes_value(&self, name: &str) -> Option<bool> {
        self.longs.get(name).copied()
    }

    fn short_consumes_value(&self, short: char) -> Option<bool> {
        self.shorts.get(&short).copied()
    }
}

/// On an arity conflict across subcommands, prefer "does not consume a
/// value": misparsing a value as a flag fails loudly in clap, while
/// consuming a subcommand name as a value would silently derail the
/// whole parse.
fn merge_arity<Key>(entry: std::collections::hash_map::Entry<'_, Key, bool>, consumes_value: bool) {
    entry.and_modify(|existing| *existing = *existing && consumes_value).or_insert(consumes_value);
}

#[cfg(test)]
mod tests;
