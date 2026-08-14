//! npm's spellings of pnpm's own options.
//!
//! pnpm accepts `--prefix` for `--dir` and `--store` for `--store-dir`,
//! renaming them over argv before parsing (the rename table in
//! `pnpm11/pnpm/src/parseCliArgs.ts`). Clap accepts the alias spellings
//! itself — they are hidden aliases on the two options, so the help output
//! documents neither, exactly like pnpm's — which leaves only the
//! precedence rule to reproduce: when a command line spells the same
//! option both ways, pnpm keeps the canonical one and drops the alias,
//! regardless of which came first. Clap alone would keep the last
//! occurrence, so [`drop_shadowed_aliases`] removes the alias tokens up
//! front.

use crate::{
    flag_relocation::{ArgTable, token_width},
    parse_boundary,
};
use clap::Command;
use std::ffi::OsString;

/// An npm spelling and the option it names.
struct RenamedOption {
    alias: &'static str,
    canonical_long: &'static str,
    canonical_short: Option<char>,
}

const RENAMED_OPTIONS: &[RenamedOption] = &[
    RenamedOption { alias: "prefix", canonical_long: "dir", canonical_short: Some('C') },
    RenamedOption { alias: "store", canonical_long: "store-dir", canonical_short: None },
];

/// Drop every alias token whose option is also spelled canonically. See
/// the module docs.
///
/// `cmd` must be the same [`Command`] the returned argv is parsed with, so
/// option values are stepped over with the arity the parse will use and a
/// value that happens to spell `--prefix` is never removed. Everything a
/// command forwards to a child process is left untouched.
pub fn drop_shadowed_aliases(cmd: &Command, argv: Vec<OsString>) -> Vec<OsString> {
    let mut arity = ArgTable::top_level(cmd);
    arity.absorb_subcommands(cmd);
    let passthrough_from = parse_boundary::passthrough_from(&argv).unwrap_or(argv.len());

    let mut canonical_seen = [false; RENAMED_OPTIONS.len()];
    let mut alias_tokens: Vec<(usize, usize, usize)> = Vec::new();
    let mut index = 1;
    while index < passthrough_from.min(argv.len()) {
        let Some(token) = argv[index].to_str() else {
            index += 1;
            continue;
        };
        if token == "--" {
            break;
        }
        if let Some(rest) = token.strip_prefix("--") {
            let (name, has_inline_value) =
                rest.split_once('=').map_or((rest, false), |(name, _)| (name, true));
            let width =
                token_width(arity.long_consumes_value(name).unwrap_or(false), has_inline_value);
            for (option_index, option) in RENAMED_OPTIONS.iter().enumerate() {
                if name == option.alias {
                    alias_tokens.push((option_index, index, width));
                } else if name == option.canonical_long {
                    canonical_seen[option_index] = true;
                }
            }
            index += width;
        } else if let Some(rest) = token.strip_prefix('-').filter(|rest| !rest.is_empty()) {
            let consumes_next = scan_short_cluster(rest, &arity, &mut canonical_seen);
            index += token_width(consumes_next, false);
        } else {
            index += 1;
        }
    }

    let shadowed: Vec<(usize, usize)> = alias_tokens
        .into_iter()
        .filter(|&(option_index, ..)| canonical_seen[option_index])
        .map(|(_, index, width)| (index, width))
        .collect();
    if shadowed.is_empty() {
        return argv;
    }
    let mut argv = argv;
    for (index, width) in shadowed.into_iter().rev() {
        argv.drain(index..(index + width).min(argv.len()));
    }
    argv
}

/// Record the canonical short options a `-abc` cluster names, and report
/// whether the cluster takes the next argv token as its value.
///
/// The chars are read left to right up to the first option that takes a
/// value, since everything past it is that value rather than more options —
/// attached when the cluster continues, the next token when it ends there.
fn scan_short_cluster(
    cluster: &str,
    arity: &ArgTable,
    canonical_seen: &mut [bool; RENAMED_OPTIONS.len()],
) -> bool {
    let mut chars = cluster.chars();
    while let Some(short) = chars.next() {
        for (option_index, option) in RENAMED_OPTIONS.iter().enumerate() {
            if option.canonical_short == Some(short) {
                canonical_seen[option_index] = true;
            }
        }
        if arity.short_consumes_value(short).unwrap_or(false) {
            return chars.next().is_none();
        }
    }
    false
}

#[cfg(test)]
mod tests;
