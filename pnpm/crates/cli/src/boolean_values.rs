//! Explicit values for boolean flags.
//!
//! nopt splices `--prod=false` into `--prod` `false` and reads the value
//! back as the flag's, so pnpm 11 takes an explicit value on every option
//! typed as `Boolean`. A build host that appends `--prod=false` to its
//! install command relies on that (pnpm/pnpm#14553), and pacquet's clap
//! flags take no value at all, so the token aborts the parse.
//!
//! [`resolve_boolean_values`] collapses the token over argv rather than
//! teaching clap a value form, which would hang a `[<VALUE>]` placeholder
//! off every boolean in the help output. A false value resolves to the
//! `--no-` negation [`crate::boolean_negations`] pairs with every boolean
//! flag. Only a token clap rejects is ever rewritten, so the pass cannot
//! change what a command line that already parses means. A standalone
//! negation such as `--no-runtime` has no positive spelling to resolve
//! to, which leaves a false value on one for clap to report.
//!
//! Two of nopt's spellings stay with clap as well. A value written as its
//! own token (`--prod false`) would have to be claimed in
//! [`option_width`], the width rule every pre-clap scan shares, and a
//! foreign command line names its program with a positional: `pnpm -r
//! exec --report-summary true` runs `true`. A value on a short
//! (`-P=false`) reaches nopt through its shorthand table, which splices
//! `-P` into `--prod` before the value rule applies, where clap parses a
//! cluster on its own terms.

use crate::{
    boolean_negations::negation_of,
    cli_args::grammar,
    config_overrides::parse_bool,
    parse_boundary::{option_width, passthrough_from, union_arity},
};
use clap::{Arg, ArgAction, Command};
use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
    sync::OnceLock,
};

/// Rewrite every `--<flag>=<bool>` token in `argv` into the bare flag or
/// its negation. See the module docs.
///
/// Tokens are stepped over with [`option_width`], the width rule the
/// pre-clap scans share, so a value that happens to spell a boolean flag
/// is never rewritten and everything a command forwards to a child
/// process is left untouched.
pub fn resolve_boolean_values(mut argv: Vec<OsString>) -> Vec<OsString> {
    let flags = boolean_flags();
    let arity = union_arity();
    let owned_by_pnpm = passthrough_from(&argv).unwrap_or(argv.len()).min(argv.len());

    let mut index = 1;
    while index < owned_by_pnpm {
        let Some(token) = argv[index].to_str() else {
            index += 1;
            continue;
        };
        if token == "--" {
            break;
        }
        // A token past the boundary is the child's, so it can be neither
        // rewritten nor read as an option's value.
        let next = (index + 1 < owned_by_pnpm).then(|| argv[index + 1].to_str()).flatten();
        let width = option_width(token, next, arity).unwrap_or(1);
        if let Some((name, value)) = token.strip_prefix("--").and_then(|rest| rest.split_once('='))
            && let Some(spelling) = flags.spelling_for(name, value)
        {
            argv[index] = spelling;
        }
        index += width;
    }

    argv
}

/// Every boolean flag spelling in the grammar, paired with the spelling
/// that means its opposite, or `None` for a flag the grammar gives no
/// opposite.
struct BooleanFlags {
    opposites: HashMap<String, Option<String>>,
}

impl BooleanFlags {
    /// The spelling `--<name>=<value>` collapses to. `None` when `name`
    /// is no boolean flag, `value` no boolean, or the flag has no
    /// spelling for that value, all of which leave the token for clap to
    /// report.
    fn spelling_for(&self, name: &str, value: &str) -> Option<OsString> {
        let opposite = self.opposites.get(name)?;
        let long = if parse_bool(value)? { name } else { opposite.as_deref()? };
        Some(OsString::from(format!("--{long}")))
    }

    fn collect(command: &Command) -> Self {
        let mut flags = Self { opposites: HashMap::new() };
        let mut value_taking = HashSet::new();
        flags.absorb(command, &mut value_taking);
        // A name some command spells as a value-taking option is left
        // alone: `--foo=false` may well be asking for the value `false`
        // there, and a flag whose opposite is one has no false spelling.
        flags.opposites.retain(|name, _| !value_taking.contains(name));
        for opposite in flags.opposites.values_mut() {
            if opposite.as_ref().is_some_and(|name| value_taking.contains(name)) {
                *opposite = None;
            }
        }
        flags
    }

    fn absorb(&mut self, command: &Command, value_taking: &mut HashSet<String>) {
        let longs: HashSet<&str> = command.get_arguments().flat_map(spellings).collect();
        for arg in command.get_arguments() {
            if arg.get_action().takes_values() {
                value_taking.extend(spellings(arg).map(str::to_owned));
                continue;
            }
            self.absorb_boolean_arg(arg, &longs);
        }
        for subcommand in command.get_subcommands() {
            self.absorb(subcommand, value_taking);
        }
    }

    /// Pair one boolean flag with its opposite spelling. `longs` is the
    /// declaring command's own long spellings.
    fn absorb_boolean_arg(&mut self, arg: &Arg, longs: &HashSet<&str>) {
        if !matches!(arg.get_action(), ArgAction::SetTrue) {
            return;
        }
        let Some(long) = arg.get_long() else {
            return;
        };
        let opposite = match long.strip_prefix("no-") {
            // A negation pairs with the flag it negates, when the
            // command declares one.
            Some(positive) => longs.contains(positive).then(|| positive.to_string()),
            // Every other boolean flag is paired by
            // [`crate::boolean_negations`].
            None => Some(negation_of(long)),
        };
        for spelling in spellings(arg) {
            self.opposites.entry(spelling.to_string()).or_insert_with(|| opposite.clone());
        }
        if let Some(opposite) = opposite {
            self.opposites.entry(opposite).or_insert_with(|| Some(long.to_string()));
        }
    }
}

/// The long spellings `arg` answers to: its own, plus its aliases.
fn spellings(arg: &Arg) -> impl Iterator<Item = &str> {
    arg.get_long().into_iter().chain(arg.get_all_aliases().into_iter().flatten())
}

fn boolean_flags() -> &'static BooleanFlags {
    static FLAGS: OnceLock<BooleanFlags> = OnceLock::new();
    FLAGS.get_or_init(|| BooleanFlags::collect(grammar()))
}

#[cfg(test)]
mod tests;
