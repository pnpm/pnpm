use super::{ArgTable, find_positional};
use clap::Command;
use std::ffi::{OsStr, OsString};

/// The invoked subcommand and its argv index, read first with the union's
/// arities. The union reads an option whose arity differs between
/// subcommands as boolean, so when an option before the found token is
/// one that token's command does not declare, the scan is retried with
/// each subcommand's own arities. The union's reading stands when no
/// subcommand's reading finds itself.
pub(super) fn find_invoked_subcommand<'cmd>(
    cmd: &'cmd Command,
    argv: &[OsString],
    top_level: &ArgTable,
    subcommand_union: &ArgTable,
) -> Option<(usize, &'cmd Command)> {
    let index = find_positional(argv, 1, top_level, subcommand_union)?;
    let found = cmd
        .find_subcommand(&argv[index])
        .map(|subcommand| (index, subcommand));
    let options_before = &argv[1..index];
    if !options_before.iter().any(|token| names_undeclared_option(token, top_level, None)) {
        return found;
    }
    if let Some((_, subcommand)) = found {
        let subcommand_table = ArgTable::subcommand(subcommand);
        if !options_before
            .iter()
            .any(|token| names_undeclared_option(token, top_level, Some(&subcommand_table)))
        {
            return found;
        }
    }
    cmd.get_subcommands()
        .find_map(|subcommand| {
            let index = find_positional(argv, 1, top_level, &ArgTable::subcommand(subcommand))?;
            cmd.find_subcommand(&argv[index])
                .is_some_and(|found| found.get_name() == subcommand.get_name())
                .then_some((index, subcommand))
        })
        .or(found)
}

/// Whether `token` is an option spelling a name neither `top_level` nor
/// `subcommand_table` declares. A short cluster's attached value counts as
/// names too, which errs toward a retry that finds nothing.
fn names_undeclared_option(
    token: &OsStr,
    top_level: &ArgTable,
    subcommand_table: Option<&ArgTable>,
) -> bool {
    let Some(token) = token.to_str() else {
        return false;
    };
    if let Some(rest) = token.strip_prefix("--") {
        let name = rest.split_once('=').map_or(rest, |(name, _)| name);
        return !name.is_empty()
            && top_level.long_consumes_value(name).is_none()
            && subcommand_table.is_none_or(|table| table.long_consumes_value(name).is_none());
    }
    token
        .strip_prefix('-')
        .is_some_and(|shorts| {
            shorts
                .chars()
                .any(|short| {
                    top_level.short_consumes_value(short).is_none()
                        && subcommand_table.is_none_or(|table| {
                            table.short_consumes_value(short).is_none()
                        })
                })
        })
}
