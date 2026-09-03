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
//! option tokens that appear before the subcommand and belong to the
//! invoked command's grammar rather than the top-level one move to
//! directly after the subcommand token (relative order preserved), so
//! clap parses them with that command's grammar exactly as if they had
//! been written there. Ownership is decided against the invoked command
//! alone: an option only some other command declares stays where it is,
//! as does one no grammar defines at all, so clap reports it the way
//! nopt does instead of a `trailing_var_arg` command such as `exec`
//! taking it for the command to run. The scan that has yet to find the
//! subcommand has no command to consult, so it steps over options using
//! the union of every subcommand's arg table; on an arity conflict
//! between subcommands the option is treated as boolean so a subcommand
//! name is never swallowed as a value. Tokens move only when the first
//! positional token names a real subcommand — external commands
//! (`pnpm <script>`) keep their argv untouched, as does everything after
//! a `--` terminator.

use clap::{Arg, Command};
use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
};

/// Where [`scan_for_positional`] stopped: at a positional token, or at
/// the `--` terminator.
pub(crate) enum PositionalScan {
    Positional(usize),
    Separator(usize),
}

/// The index of the next positional token at or after `index`, stepping
/// over option tokens (and the values they consume, per the arity tables).
/// `None` when argv ends or a `--` terminator is reached first.
pub(crate) fn find_positional(
    argv: &[OsString],
    index: usize,
    top_level: &ArgTable,
    subcommand_union: &ArgTable,
) -> Option<usize> {
    match scan_for_positional(argv, index, top_level, subcommand_union)? {
        PositionalScan::Positional(index) => Some(index),
        PositionalScan::Separator(_) => None,
    }
}

/// [`find_positional`]'s scan with the stop reason kept: a caller that
/// mirrors nopt — which strips the `--` terminator and treats what
/// follows as positionals — needs to tell the terminator apart from
/// running out of argv (`None`).
pub(crate) fn scan_for_positional(
    argv: &[OsString],
    mut index: usize,
    top_level: &ArgTable,
    subcommand_union: &ArgTable,
) -> Option<PositionalScan> {
    loop {
        let token = argv.get(index).and_then(|token| token.to_str())?;
        if token == "--" {
            return Some(PositionalScan::Separator(index));
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
            let consumes_value = short_cluster_consumes_value(rest, |short| {
                top_level
                    .short_consumes_value(short)
                    .or_else(|| subcommand_union.short_consumes_value(short))
            });
            index += token_width(consumes_value, false);
        } else {
            return Some(PositionalScan::Positional(index));
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
    let Some(subcommand) = cmd.find_subcommand(&argv[subcommand_index]) else {
        return argv;
    };
    let subcommand_table = ArgTable::subcommand(subcommand);

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
            } else if let Some(consumes_value) = subcommand_table.long_consumes_value(name) {
                let width = token_width(consumes_value, has_inline_value);
                for offset in 0..width.min(argv.len() - index) {
                    moved_indexes.insert(index + offset);
                }
                index += width;
            } else {
                index += token_width(false, has_inline_value);
            }
        } else if let Some(rest) = token.strip_prefix('-').filter(|rest| !rest.is_empty()) {
            // A cluster is judged by every short it stacks, not by its
            // first one: `-ro dist` mixes the global `-r` with
            // `pack-app`'s `-o`, and the whole token has to travel for
            // clap to see the option `pack-app` owns. One short the
            // command does not declare pins the whole cluster, since
            // moving it would hand that short to the command too.
            let mut has_subcommand_short = false;
            let mut has_unknown_short = false;
            let consumes_value = short_cluster_consumes_value(rest, |short| {
                if let Some(consumes_value) = top_level.short_consumes_value(short) {
                    return Some(consumes_value);
                }
                if let Some(consumes_value) = subcommand_table.short_consumes_value(short) {
                    has_subcommand_short = true;
                    Some(consumes_value)
                } else {
                    has_unknown_short = true;
                    None
                }
            });
            let width = token_width(consumes_value, false);
            if has_subcommand_short && !has_unknown_short {
                for offset in 0..width.min(argv.len() - index) {
                    moved_indexes.insert(index + offset);
                }
            }
            index += width;
        } else {
            break;
        }
    }

    if moved_indexes.is_empty() {
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
pub(crate) fn token_width(consumes_value: bool, has_inline_value: bool) -> usize {
    if consumes_value && !has_inline_value { 2 } else { 1 }
}

/// Whether a short-option token consumes the next argv token as its
/// value. `rest` is the token with its leading `-` stripped, and may be a
/// cluster: clap lets boolean shorts stack ahead of a value-taking one
/// (`-rC dir` is `-r -C dir`), and a value written against its option
/// (`-rCdir`) leaves nothing for the next token. `arity` reports whether
/// one short takes a value, or `None` for a short the grammar does not
/// define, which consumes nothing; it is called for exactly the shorts
/// clap parses as options, so a caller can classify them as it scans.
pub(crate) fn short_cluster_consumes_value(
    rest: &str,
    mut arity: impl FnMut(char) -> Option<bool>,
) -> bool {
    let mut shorts = rest.chars();
    while let Some(short) = shorts.next() {
        if arity(short).unwrap_or(false) {
            return shorts.as_str().is_empty();
        }
    }
    false
}

/// Option-name lookup table: long / short spelling → whether the option
/// consumes the next argv token as its value.
#[derive(Debug, Default)]
pub(crate) struct ArgTable {
    longs: HashMap<String, bool>,
    shorts: HashMap<char, bool>,
}

impl ArgTable {
    /// The top-level grammar: everything already valid before the
    /// subcommand, which therefore stays in place. Clap only adds the
    /// automatic `--help` / `-h` at build time, so they are seeded
    /// manually.
    pub(crate) fn top_level(cmd: &Command) -> Self {
        let mut table = Self::default();
        table.longs.insert("help".to_string(), false);
        table.shorts.insert('h', false);
        table.absorb(cmd.get_arguments());
        table
    }

    /// The union of every subcommand's args. It cannot say which command
    /// owns an option, so it serves only the scan that has yet to find the
    /// subcommand and needs a token's width to step over it.
    pub(crate) fn subcommand_union(cmd: &Command) -> Self {
        let mut table = Self::default();
        table.absorb(cmd.get_subcommands().flat_map(Command::get_arguments));
        table
    }

    /// One subcommand's own args, which decide whether a token written
    /// before it belongs to the command being invoked.
    pub(crate) fn subcommand(cmd: &Command) -> Self {
        let mut table = Self::default();
        table.absorb(cmd.get_arguments());
        table
    }

    /// Fold every subcommand's own options into this table, for callers
    /// that need one arity view over the whole CLI because the command is
    /// not known yet (the pre-clap passes, via
    /// [`crate::parse_boundary::passthrough_from`]).
    pub(crate) fn absorb_subcommands(&mut self, cmd: &Command) {
        self.absorb(cmd.get_subcommands().flat_map(Command::get_arguments));
    }

    fn absorb<'a, Args: IntoIterator<Item = &'a Arg>>(&mut self, args: Args) {
        for arg in args {
            let consumes_value = arg.get_action().takes_values()
                && arg.get_num_args().is_none_or(|range| range.takes_values())
                && !arg.is_require_equals_set();
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

    pub(crate) fn long_consumes_value(&self, name: &str) -> Option<bool> {
        self.longs.get(name).copied()
    }

    pub(crate) fn short_consumes_value(&self, short: char) -> Option<bool> {
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
