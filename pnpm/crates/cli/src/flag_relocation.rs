//! Position-independent placement of subcommand options.
//!
//! pnpm parses argv with `nopt`, which merges the universal option table
//! with the invoked command's table and accepts any option anywhere on
//! the command line: `pnpm --ignore-scripts --prod deploy <dir>` and
//! `pnpm deploy --ignore-scripts --prod <dir>` are the same invocation
//! (pnpm's release tooling relies on this, `bundle-deps.ts` passes
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
//! name is never swallowed as a value. When the token that reading finds
//! is no command that declares the options before it, the scan is retried
//! with each subcommand's own arities, so `pnpm -o <dir> pack-app` still
//! finds `pack-app` although `add -o` takes no value. Tokens move only when the scan finds a real subcommand:
//! external commands (`pnpm <script>`) keep their argv untouched, as does
//! everything after a `--` terminator.

use clap::{
    Arg,
    Command,
};
use invoked_subcommand::find_invoked_subcommand;
use std::{
    collections::{
        HashMap,
        HashSet,
    },
    ffi::{
        OsStr,
        OsString,
    },
};

/// The long option name before `=` in `--<name>=<value>`, even when
/// `<value>` is not valid UTF-8. Clap splits the option name from its
/// `OsStr` value before decoding the name, so pre-clap scans must too.
pub(crate) fn inline_long_option_name(token: &OsStr) -> Option<&str> {
    let rest = token.as_encoded_bytes().strip_prefix(b"--")?;
    let eq = rest
        .iter()
        .position(|&byte| byte == b'=')?;
    std::str::from_utf8(&rest[..eq])
        .ok()
        .filter(|name| !name.is_empty())
}

/// The ASCII short-option prefix (without the leading `-`) of a token that
/// attaches a non-UTF-8 value directly to a value-taking short option, such
/// as `-C<non-UTF-8>` or `-rC<non-UTF-8>`.
pub(crate) fn attached_short_option_prefix(
    token: &OsStr,
    mut consumes_value: impl FnMut(char) -> Option<bool>,
) -> Option<&str> {
    let rest = token.as_encoded_bytes().strip_prefix(b"-")?;
    if rest.is_empty() || rest.starts_with(b"-") {
        return None;
    }
    for (index, &byte) in rest.iter().enumerate() {
        if !byte.is_ascii() {
            return None;
        }
        if consumes_value(char::from(byte)).unwrap_or(false) {
            return std::str::from_utf8(&rest[..=index]).ok();
        }
    }
    None
}

#[derive(Clone, Copy)]
pub(crate) enum PositionalTarget {
    Subcommand,
    Package,
}

impl PositionalTarget {
    fn skips_option(self, owner: OptionOwner) -> bool {
        match owner {
            OptionOwner::TopLevel | OptionOwner::Subcommand => true,
            OptionOwner::Unknown => match self {
                Self::Subcommand => true,
                Self::Package => false,
            },
        }
    }
}

/// A package must follow recognized options. An unknown option leaves its
/// arity ambiguous, so the following token cannot select the add grammar.
pub(crate) fn scan_for_positional(
    argv: &[OsString],
    mut index: usize,
    top_level: &ArgTable,
    subcommand_table: &ArgTable,
    target: PositionalTarget,
) -> Option<usize> {
    loop {
        let raw = argv.get(index)?;
        if raw == "--" {
            return match target {
                PositionalTarget::Subcommand => None,
                PositionalTarget::Package => (index + 1 < argv.len()).then_some(index + 1),
            };
        }
        let Some(option) = scan_option(raw, top_level, subcommand_table) else {
            return raw.to_str().map(|_| index);
        };
        if !target.skips_option(option.owner) {
            return None;
        }
        index += option.width;
    }
}

/// Find the next positional before the separator while looking for a command.
pub(crate) fn find_positional(
    argv: &[OsString],
    index: usize,
    top_level: &ArgTable,
    subcommand_union: &ArgTable,
) -> Option<usize> {
    scan_for_positional(argv, index, top_level, subcommand_union, PositionalTarget::Subcommand)
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

    expand_recursive_alias(&mut argv, &top_level, &subcommand_union);

    let Some((subcommand_index, subcommand)) =
        find_invoked_subcommand(cmd, &argv, &top_level, &subcommand_union)
    else {
        return argv;
    };
    let subcommand_table = ArgTable::subcommand(subcommand);

    let moved_indexes =
        moved_option_indexes(&argv, subcommand_index, &top_level, &subcommand_table);
    if moved_indexes.is_empty() {
        return argv;
    }
    reorder_after_subcommand(argv, &moved_indexes, subcommand_index)
}

/// Rewrite a leading `recursive` / `multi` / `m` word into `--recursive`,
/// which is how pnpm spells the same thing when a subcommand follows.
fn expand_recursive_alias(
    argv: &mut [OsString],
    top_level: &ArgTable,
    subcommand_union: &ArgTable,
) {
    let mut current_idx = 1;
    while let Some(pos_idx) = find_positional(argv, current_idx, top_level, subcommand_union) {
        if let Some(token) = argv.get(pos_idx).and_then(|t| t.to_str())
            && matches!(token, "recursive" | "multi" | "m")
            && find_positional(argv, pos_idx + 1, top_level, subcommand_union).is_some()
        {
            argv[pos_idx] = OsString::from("--recursive");
            current_idx = pos_idx + 1;
            continue;
        }
        break;
    }
}

/// The argv positions, before the subcommand token, that spell an option
/// the subcommand declares and so have to travel with it.
fn moved_option_indexes(
    argv: &[OsString],
    subcommand_index: usize,
    top_level: &ArgTable,
    subcommand_table: &ArgTable,
) -> HashSet<usize> {
    let mut moved_indexes = HashSet::new();
    let mut index = 1;
    while index < subcommand_index {
        let Some(scanned) = scan_option(&argv[index], top_level, subcommand_table) else {
            break;
        };
        match scanned.owner {
            OptionOwner::Subcommand => {
                moved_indexes.extend(index..(index + scanned.width).min(subcommand_index));
            }
            OptionOwner::TopLevel | OptionOwner::Unknown => {}
        }
        index += scanned.width;
    }
    moved_indexes
}

/// How much argv one option token covers, and whether it belongs to the
/// subcommand's grammar rather than the top-level one.
struct ScannedToken {
    width: usize,
    owner: OptionOwner,
}

#[derive(Clone, Copy)]
enum OptionOwner {
    TopLevel,
    Subcommand,
    Unknown,
}

/// Classify an option, including one with an attached non-UTF-8 value.
fn scan_option(
    raw: &OsStr,
    top_level: &ArgTable,
    subcommand_table: &ArgTable,
) -> Option<ScannedToken> {
    let Some(token) = raw.to_str() else {
        return scan_non_utf8_option(raw, top_level, subcommand_table);
    };
    if token == "--" {
        return None;
    }
    if let Some(rest) = token.strip_prefix("--") {
        return Some(scan_long_option(rest, top_level, subcommand_table));
    }
    let rest = token
        .strip_prefix('-')
        .filter(|rest| !rest.is_empty())?;
    Some(scan_short_cluster(rest, top_level, subcommand_table))
}

fn scan_non_utf8_option(
    raw: &OsStr,
    top_level: &ArgTable,
    subcommand_table: &ArgTable,
) -> Option<ScannedToken> {
    if let Some(name) = inline_long_option_name(raw) {
        return Some(scan_long_option_parts(name, true, top_level, subcommand_table));
    }
    let consumes = |short| {
        top_level
            .short_consumes_value(short)
            .or_else(|| subcommand_table.short_consumes_value(short))
    };
    let shorts = attached_short_option_prefix(raw, consumes)?;
    Some(ScannedToken { width: 1, ..scan_short_cluster(shorts, top_level, subcommand_table) })
}

fn scan_long_option(rest: &str, top_level: &ArgTable, subcommand_table: &ArgTable) -> ScannedToken {
    let (name, has_inline_value) = rest
        .split_once('=')
        .map(|(name, _)| (name, true))
        .unwrap_or((rest, false));
    scan_long_option_parts(name, has_inline_value, top_level, subcommand_table)
}

fn scan_long_option_parts(
    name: &str,
    has_inline_value: bool,
    top_level: &ArgTable,
    subcommand_table: &ArgTable,
) -> ScannedToken {
    if let Some(consumes_value) = top_level.long_consumes_value(name) {
        return ScannedToken {
            width: token_width(consumes_value, has_inline_value),
            owner: OptionOwner::TopLevel,
        };
    }
    let Some(consumes_value) = subcommand_table.long_consumes_value(name) else {
        return ScannedToken { width: 1, owner: OptionOwner::Unknown };
    };
    ScannedToken {
        width: token_width(consumes_value, has_inline_value),
        owner: OptionOwner::Subcommand,
    }
}

/// A cluster is judged by every short it stacks, not by its first one:
/// `-rt linux-x64` mixes the global `-r` with `pack-app`'s `-t`, and the whole
/// token has to travel for clap to see the option `pack-app` owns. One
/// short the command does not declare pins the whole cluster, since moving
/// it would hand that short to the command too.
fn scan_short_cluster(
    rest: &str,
    top_level: &ArgTable,
    subcommand_table: &ArgTable,
) -> ScannedToken {
    let mut owner = OptionOwner::TopLevel;
    let consumes_value = short_cluster_consumes_value(rest, |short| {
        if let Some(consumes_value) = top_level.short_consumes_value(short) {
            return Some(consumes_value);
        }
        if let Some(consumes_value) = subcommand_table.short_consumes_value(short) {
            owner = match owner {
                OptionOwner::TopLevel | OptionOwner::Subcommand => OptionOwner::Subcommand,
                OptionOwner::Unknown => OptionOwner::Unknown,
            };
            Some(consumes_value)
        } else {
            owner = OptionOwner::Unknown;
            None
        }
    });
    ScannedToken { width: token_width(consumes_value, false), owner }
}

/// Rebuild argv with the marked tokens lifted to directly after the
/// subcommand token.
fn reorder_after_subcommand(
    argv: Vec<OsString>,
    moved_indexes: &HashSet<usize>,
    subcommand_index: usize,
) -> Vec<OsString> {
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
        Self::from_subcommands(cmd.get_subcommands())
    }

    pub(crate) fn from_subcommands<'a>(commands: impl IntoIterator<Item = &'a Command>) -> Self {
        let mut table = Self::default();
        table.absorb(commands.into_iter().flat_map(Command::get_arguments));
        table
    }

    /// One subcommand's own args, which decide whether a token written
    /// before it belongs to the command being invoked.
    pub(crate) fn subcommand(cmd: &Command) -> Self {
        Self::from_subcommands([cmd])
    }

    /// The options a command line invoking `subcommand` of `root` accepts:
    /// `root`'s own and `subcommand`'s.
    pub(crate) fn for_subcommand(root: &Command, subcommand: &Command) -> Self {
        let mut table = Self::top_level(root);
        table.absorb(subcommand.get_arguments());
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
            for long in arg
                .get_long()
                .into_iter()
                .chain(
                    arg.get_all_aliases()
                        .into_iter()
                        .flatten(),
                )
            {
                merge_arity(self.longs.entry(long.to_string()), consumes_value);
            }
            for short in arg
                .get_short()
                .into_iter()
                .chain(
                    arg.get_all_short_aliases()
                        .into_iter()
                        .flatten(),
                )
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
    entry
        .and_modify(|existing| *existing = *existing && consumes_value)
        .or_insert(consumes_value);
}

mod invoked_subcommand;

#[cfg(test)]
mod tests;
