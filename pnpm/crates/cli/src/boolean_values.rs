//! Explicit values for boolean flags.
//!
//! nopt gives every option typed as `Boolean` a value form: it splices
//! `--prod=false` into `--prod` `false` and reads the value back as the
//! flag's. That is how a build host that appends `--prod=false` to its
//! install command asks for devDependencies (pnpm/pnpm#14553). pacquet's
//! clap flags take no value at all, so the token aborts the parse with
//! "unexpected value".
//!
//! Rather than teach clap a value form — which would put a `[<VALUE>]`
//! placeholder next to every boolean in the help output —
//! [`resolve_boolean_values`] collapses the token over argv: a true value
//! leaves the bare flag, a false one becomes the `--no-` negation that
//! [`crate::boolean_negations`] pairs with every boolean flag. The
//! grammar clap parses therefore stays exactly as it is written, and the
//! only command lines this pass rewrites are ones that abort the parse
//! today.
//!
//! A value written as its own token (`--prod false`, which nopt reads the
//! same way) is deliberately left alone. Claiming it belongs in
//! [`option_width`], the width rule every pre-clap scan shares, and a
//! foreign command line names its program with a positional: `pnpm -r
//! exec --report-summary true` runs `true`, which a flag claiming the
//! token after it would swallow.
//!
//! Only long spellings take a value. A short is clap's own spelling:
//! nopt reaches one through its shorthand table, which splices `-P` into
//! `--prod` before the value rule applies, and the two part ways on a
//! cluster such as `-PD=false`.

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
/// that means its opposite.
struct BooleanFlags {
    opposites: HashMap<String, String>,
}

impl BooleanFlags {
    /// The spelling `--<name>=<value>` collapses to, or `None` when
    /// `name` is no boolean flag, or `value` no boolean — which leaves
    /// the token for clap to report.
    fn spelling_for(&self, name: &str, value: &str) -> Option<OsString> {
        let opposite = self.opposites.get(name)?;
        let long = if parse_bool(value)? { name } else { opposite.as_str() };
        Some(OsString::from(format!("--{long}")))
    }

    /// The grammar's boolean flags, reaching the same depth as the arity
    /// table the pre-clap scans share, so a flag is never rewritten in a
    /// token the scan did not recognize as an option.
    fn collect(command: &Command) -> Self {
        let mut flags = Self { opposites: HashMap::new() };
        let mut value_taking = HashSet::new();
        flags.absorb(command, &mut value_taking);
        for subcommand in command.get_subcommands() {
            flags.absorb(subcommand, &mut value_taking);
        }
        // A name another command spells as a value-taking option is left
        // alone: `--foo=false` may well be asking for the value `false`
        // there.
        for name in value_taking {
            flags.opposites.remove(&name);
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
            if !matches!(arg.get_action(), ArgAction::SetTrue) {
                continue;
            }
            let Some(long) = arg.get_long() else {
                continue;
            };
            let Some(opposite) = (match long.strip_prefix("no-") {
                // A hand-written negation pairs with the flag it negates,
                // when the command declares one.
                Some(positive) => longs.contains(positive).then(|| positive.to_string()),
                // Every other boolean flag is paired by
                // [`crate::boolean_negations`].
                None => Some(negation_of(long)),
            }) else {
                continue;
            };
            for spelling in spellings(arg) {
                self.opposites.entry(spelling.to_string()).or_insert_with(|| opposite.clone());
            }
            self.opposites.entry(opposite).or_insert_with(|| long.to_string());
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
