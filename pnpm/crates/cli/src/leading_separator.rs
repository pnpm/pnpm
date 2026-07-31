//! Preservation of a `--` that opens a forwarded command line.
//!
//! clap treats the first `--` it meets as its own escape token and drops
//! it, unless a `trailing_var_arg` positional has already started taking
//! values. So a command whose arguments begin *at* the separator —
//! `pnpm stop -- --flag`, where no script name precedes it — loses it,
//! and the program the script runs reads `--flag` as its own option.
//!
//! Writing a second `--` at that position gives clap one to consume and
//! leaves the user's to reach the script. The position comes from
//! [`command_boundary`], the same scan the other pre-clap passes read, so
//! this cannot disagree with them about who owns a token.
//!
//! Only the script shortcuts can hit this: every other command that
//! forwards a command line puts a script or command name ahead of the
//! separator, which lands a value in the positional first, and from there
//! clap keeps every `--` verbatim — the reason `pnpm run build -- --flag`
//! was already right.

use crate::parse_boundary::command_boundary;
use std::ffi::OsString;

pub(crate) fn preserve_leading_separator(mut argv: Vec<OsString>) -> Vec<OsString> {
    let Some(boundary) = command_boundary(&argv).filter(|boundary| boundary.is_script_shortcut)
    else {
        return argv;
    };
    if argv.get(boundary.index).is_none_or(|token| token != "--") {
        return argv;
    }
    argv.insert(boundary.index, OsString::from("--"));
    argv
}

#[cfg(test)]
mod tests;
