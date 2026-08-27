//! `pnpm pm <cmd>` as a way to force pnpm's built-in command.
//!
//! A few built-in commands (`clean`, `purge`, ...) step aside when the
//! current project declares a `package.json` script by the same name. The
//! `pm` prefix opts out of that: it forces the built-in even when such a
//! script exists (<https://pnpm.io/cli/pm>).
//!
//! `pm` is not a command of its own — it is stripped from argv before clap
//! parses it, so the rest of the command line is dispatched exactly as if
//! the prefix had not been typed. Like pnpm, only a `pm` written as the
//! very first token counts; anywhere else it is an ordinary argument.

use std::ffi::OsString;

/// Split a leading `pm` token off `argv` (program name at index 0),
/// returning the argv clap should parse and whether the built-in command
/// was forced.
pub(crate) fn strip_prefix(mut argv: Vec<OsString>) -> (Vec<OsString>, bool) {
    let builtin_command_forced = argv.get(1).is_some_and(|token| token == "pm");
    if builtin_command_forced {
        argv.remove(1);
    }
    (argv, builtin_command_forced)
}

#[cfg(test)]
mod tests;
