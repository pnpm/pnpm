//! `pnpm install <pkg>` as a spelling of `pnpm add <pkg>`.
//!
//! pnpm resolves the command name before it parses argv, and an `install`
//! that was given a package name resolves to `add`, so `pnpm i valibot`
//! saves the dependency instead of installing the project. Clap has no
//! such step: `install` declares no positional, so the package name ends
//! the parse with "unexpected argument" (pnpm/pnpm#13886).
//!
//! Renaming the subcommand token in argv hands the whole command line to
//! `add`'s grammar, which is the same thing pnpm's own parser does with
//! it. The scan runs after [`relocate_pre_subcommand_flags`], so an option
//! written before the subcommand already sits after it and cannot be
//! mistaken for the package name. The settings extraction ahead of both
//! resolves the command the same way — see [`subcommand_option_names`].
//!
//! [`relocate_pre_subcommand_flags`]: crate::flag_relocation::relocate_pre_subcommand_flags
//! [`subcommand_option_names`]: crate::parse_boundary::subcommand_option_names

use crate::{
    cli_args::grammar,
    flag_relocation::{ArgTable, PositionalScan, find_positional, scan_for_positional},
    parse_boundary::{option_width, union_arity},
};
use clap::{
    Command,
    error::{ContextKind, ContextValue, ErrorKind},
};
use std::ffi::OsString;

/// Rewrite the `install` subcommand token to `add` when a package name
/// follows it. A no-op for every other command line, `pnpm install` with
/// options but no package included.
pub(crate) fn rewrite(cmd: &Command, mut argv: Vec<OsString>) -> Vec<OsString> {
    let top_level = ArgTable::top_level(cmd);
    let subcommand_union = ArgTable::subcommand_union(cmd);
    let Some(subcommand_index) = find_positional(&argv, 1, &top_level, &subcommand_union) else {
        return argv;
    };
    let names_install = cmd
        .find_subcommand(&argv[subcommand_index])
        .is_some_and(|subcommand| subcommand.get_name() == "install");
    if !names_install {
        return argv;
    }
    let names_package =
        match scan_for_positional(&argv, subcommand_index + 1, &top_level, &subcommand_union) {
            Some(PositionalScan::Positional(_)) => true,
            // nopt strips the `--` terminator and treats what follows as
            // positionals, so `pnpm install -- <pkg>` names a package too.
            Some(PositionalScan::Separator(separator_index)) => separator_index + 1 < argv.len(),
            None => false,
        };
    if !names_package {
        return argv;
    }
    argv[subcommand_index] = OsString::from("add");
    argv
}

pub(crate) fn suggest_no_frozen_lockfile(argv: &[OsString], error: &mut clap::Error) {
    if error.kind() != ErrorKind::UnknownArgument
        || !matches!(error.get(ContextKind::InvalidArg), Some(ContextValue::String(arg)) if arg == "--frozen-lockfile")
    {
        return;
    }
    let command = grammar();
    let arity = union_arity();
    let Some(subcommand_index) = find_positional(argv, 1, arity, arity) else {
        return;
    };
    if command
        .find_subcommand(&argv[subcommand_index])
        .is_none_or(|subcommand| subcommand.get_name() != "install")
    {
        return;
    }
    let mut index = subcommand_index + 1;
    while let Some(token) = argv.get(index).and_then(|token| token.to_str()) {
        if token == "--" {
            break;
        }
        let next = argv.get(index + 1).and_then(|token| token.to_str());
        if token == "--frozen-lockfile" && next == Some("false") {
            let hint = "to disable frozen-lockfile mode, use '--no-frozen-lockfile' instead of '--frozen-lockfile false'".into();
            error.insert(ContextKind::Suggested, ContextValue::StyledStrs(vec![hint]));
            break;
        }
        index += option_width(token, next, arity).unwrap_or(1);
    }
}

#[cfg(test)]
mod tests;
