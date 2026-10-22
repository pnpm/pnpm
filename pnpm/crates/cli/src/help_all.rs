//! `pnpm help --all` (`-a`).
//!
//! pnpm's `help` lists only its common commands unless `--all` is given
//! (`pnpm11/pnpm/src/cmd/help.ts`). Clap's help subcommand already lists
//! every command and declares no options, so the flag is dropped before
//! clap parses argv.

use crate::flag_relocation::{ArgTable, find_positional};
use clap::Command;
use std::ffi::{OsStr, OsString};

/// Remove `--all`, `-a`, and the other spellings of the boolean (`--all=<value>`,
/// `--no-all`) from the arguments of the `help` subcommand. Their value is
/// ignored, since clap's help is always the full list. A no-op for every other
/// command line.
pub(crate) fn drop_all_flag(cmd: &Command, argv: Vec<OsString>) -> Vec<OsString> {
    let top_level = ArgTable::top_level(cmd);
    let subcommand_union = ArgTable::subcommand_union(cmd);
    let Some(subcommand_index) = find_positional(&argv, 1, &top_level, &subcommand_union) else {
        return argv;
    };
    if argv[subcommand_index] != "help" {
        return argv;
    }
    argv.into_iter()
        .enumerate()
        .filter(|(index, token)| *index <= subcommand_index || !spells_all_flag(token))
        .map(|(_, token)| token)
        .collect()
}

fn spells_all_flag(token: &OsStr) -> bool {
    token
        .to_str()
        .is_some_and(|token| {
            matches!(token, "--all" | "-a" | "--no-all") || token.starts_with("--all=")
        })
}
