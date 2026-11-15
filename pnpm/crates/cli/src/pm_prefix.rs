//! `pnpm pm <cmd>` as a way to force pnpm's built-in command.
//!
//! A few built-in commands (`clean`, `purge`, ...) step aside when the
//! current project declares a `package.json` script by the same name. The
//! `pm` prefix opts out of that: it forces the built-in even when such a
//! script exists (<https://pnpm.io/cli/pm>).
//!
//! `pm` is not a command of its own — it is stripped from argv before clap
//! parses it, so the rest of the command line is dispatched as if the prefix
//! had not been typed. Only a `pm` at the command slot (past any pre-command
//! options or `with current`) counts; anywhere else it is an ordinary argument.

use crate::parse_boundary::PreCommandPrefixes;
use std::ffi::OsString;

/// Split a `pm` prefix off `argv` (program name at index 0), returning the
/// argv clap should parse and whether the built-in command was forced.
pub(crate) fn strip_prefix(mut argv: Vec<OsString>) -> (Vec<OsString>, bool) {
    let Some(pm_index) = PreCommandPrefixes::scan(&argv).pm else {
        return (argv, false);
    };
    argv.remove(pm_index);
    (argv, true)
}

#[cfg(test)]
mod tests;
