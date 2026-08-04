//! Generic `--no-<flag>` support for every boolean flag.
//!
//! pnpm parses its CLI with `nopt`, which auto-accepts a `--no-<name>`
//! negation for every option typed as `Boolean` — so `pnpm install
//! --no-frozen-lockfile`, `--no-lockfile-only`, `--no-dry-run`, etc. all
//! parse even though only the positive spelling is documented. pacquet's
//! clap flags are plain `#[clap(long)] bool`s, which accept only the
//! positive form, so any forwarded `--no-<bool>` aborts the parser with
//! "unexpected argument".
//!
//! Rather than hand-write a paired `--no-` flag per boolean (and keep them
//! in sync forever), [`with_boolean_negations`] walks the built clap
//! [`Command`] and, for every boolean flag that doesn't already have a
//! `--no-` sibling, adds a hidden negation that `overrides_with` the
//! positive flag. Last-one-wins override semantics mean `--no-foo` leaves
//! the field at its `false` default and `--foo --no-foo` resolves to
//! `false`, matching nopt. Because the negations are real clap args, the
//! grammar stays intact — trailing pass-through args (`run`, `exec`,
//! `dlx`) and value-taking flags are unaffected.

use clap::{Arg, ArgAction, Command};
use std::collections::HashSet;

/// Add a hidden `--no-<flag>` negation for every boolean flag in `cmd`
/// (recursively, across all subcommands) that lacks one. See the module
/// docs for why this mirrors pnpm/nopt.
pub fn with_boolean_negations(mut cmd: Command) -> Command {
    let existing_longs: HashSet<String> =
        cmd.get_arguments().filter_map(|arg| arg.get_long().map(String::from)).collect();

    let negations: Vec<(clap::Id, String, bool)> = cmd
        .get_arguments()
        .filter(|arg| matches!(arg.get_action(), ArgAction::SetTrue))
        .filter_map(|arg| {
            let long = arg.get_long()?;
            // Skip explicit negations (`--no-optional`, `--no-runtime`, ...)
            // and any positive flag that already ships its own `--no-`
            // counterpart. The latter also covers a global flag already
            // seen from an ancestor command on this recursion, so its
            // negation isn't added twice.
            if long.starts_with("no-") || existing_longs.contains(&format!("no-{long}")) {
                return None;
            }
            Some((arg.get_id().clone(), format!("no-{long}"), arg.is_global_set()))
        })
        .collect();

    for (positive_id, negated_long, is_global) in negations {
        let negated_id = format!("__negated__{negated_long}");
        cmd = cmd.mut_arg(positive_id.clone(), |arg| arg.overrides_with(negated_id.clone()));
        // A global source flag propagates into every subcommand, so its
        // negation must too — otherwise the override reference dangles at
        // the subcommand level.
        let mut negation = Arg::new(negated_id)
            .long(negated_long)
            .action(ArgAction::SetTrue)
            .hide(true)
            .overrides_with(positive_id);
        if is_global {
            negation = negation.global(true);
        }
        cmd = cmd.arg(negation);
    }

    let subcommand_names: Vec<String> =
        cmd.get_subcommands().map(|sub| sub.get_name().to_string()).collect();
    for name in subcommand_names {
        cmd = cmd.mut_subcommand(name, with_boolean_negations);
    }

    cmd
}

#[cfg(test)]
mod tests;
