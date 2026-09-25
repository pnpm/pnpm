//! Protection of the arguments forwarded by a script shortcut.
//!
//! Script shortcuts forward everything after the shortcut name to the
//! script. A `--` at that boundary keeps clap from claiming an option that
//! belongs to the script as one of pnpm's global options (for example,
//! `pnpm test --filter=Foo`).
//!
//! When the user supplied `--` there, inserting another one gives clap one
//! to consume and leaves the user's separator to reach the script. The
//! position comes from [`command_boundary`], the same scan the other
//! pre-clap passes read, so this cannot disagree with them about who owns a
//! token.
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
    if argv.get(boundary.index).is_none() {
        return argv;
    }
    argv.insert(boundary.index, OsString::from("--"));
    argv
}

#[cfg(test)]
mod tests;
