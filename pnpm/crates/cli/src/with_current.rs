//! Rewrite `pnpm with current <cmd> [args]` before clap parses argv.
//!
//! `pnpm [global-opts] with current <cmd> [args]` is sugar for
//! `pnpm [global-opts] <cmd> [args]` run with the project's
//! `packageManager` / `devEngines.packageManager` check disabled. The
//! `with current` tokens are removed in place (so any global flags
//! before `with` are preserved) and the override is propagated via the
//! `pnpm_config_pm_on_fail` env var rather than an argv flag, so it
//! survives clap's `-v` / `--version` short-circuit and reaches both the
//! in-process config load and any child process the command spawns.

use crate::{
    cli_args::grammar,
    flag_relocation::{ArgTable, short_cluster_consumes_value},
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use std::ffi::OsString;

/// Raised when `with current` has no command after it.
#[derive(Debug, Display, Error, Diagnostic)]
#[display(r#"Missing command after "current". Usage: pnpm with current <command> [args...]"#)]
#[diagnostic(code(ERR_PNPM_MISSING_WITH_CURRENT_CMD))]
pub struct MissingWithCurrentCommand;

/// Rewrite `argv` (program name at index 0) when it contains a
/// `with current` command pair, returning the argv clap should parse. A
/// no-op when there is no such pair. As a side effect, forces
/// `pmOnFail=ignore` for the in-process run (and its children) via the
/// `pnpm_config_pm_on_fail` env var.
pub fn rewrite(argv: Vec<OsString>) -> miette::Result<Vec<OsString>> {
    let (argv, force_pm_on_fail_ignore) = plan(argv)?;
    if force_pm_on_fail_ignore {
        // SAFETY: `rewrite` runs in `main` before the tokio runtime and
        // rayon pool start, so the process is single-threaded and no other
        // thread can be reading the environment concurrently.
        unsafe {
            std::env::set_var("pnpm_config_pm_on_fail", "ignore");
        }
    }
    Ok(argv)
}

/// The pure core of [`rewrite`]: returns the rewritten argv and whether a
/// `with current` pair was stripped (in which case the caller must force
/// `pmOnFail=ignore`). Split out so the argv transformation is testable
/// without mutating the process environment.
fn plan(mut argv: Vec<OsString>) -> miette::Result<(Vec<OsString>, bool)> {
    let Some(index) = find_with_current_index(&argv) else {
        return Ok((argv, false));
    };
    // `with current` with nothing after it is a usage error.
    if argv.len() <= index + 2 {
        return Err(MissingWithCurrentCommand.into());
    }
    argv.drain(index..index + 2);
    Ok((argv, true))
}

/// The index of the `with` token when `with current` is the actual
/// top-level subcommand, or `None` otherwise.
///
/// `with current` is only the sugar when `with` sits at the subcommand
/// position — `pnpm [global-opts] with current ...`. A `with current`
/// appearing later as data for another command (`pnpm exec with current
/// ...`) must be left alone: the rewrite only applies when the parsed
/// command is `with` with first param `current`.
fn find_with_current_index(argv: &[OsString]) -> Option<usize> {
    let command = command_index(argv)?;
    let is_with = argv.get(command).is_some_and(|token| token == "with");
    let is_current = argv.get(command + 1).is_some_and(|token| token == "current");
    (is_with && is_current).then_some(command)
}

/// Index of the top-level subcommand token: the first positional after the
/// program name, skipping global options (and the values they consume) and
/// honoring `--` as end-of-options. `None` when argv carries no subcommand.
fn command_index(argv: &[OsString]) -> Option<usize> {
    let mut index = 1;
    while index < argv.len() {
        let Some(token) = argv[index].to_str() else {
            // A non-UTF-8 token can't be a known option or `with`; treat it
            // as the subcommand position (it simply won't match `with`).
            return Some(index);
        };
        if token == "--" {
            let next = index + 1;
            return (next < argv.len()).then_some(next);
        }
        if !token.starts_with('-') {
            return Some(index);
        }
        index += if option_consumes_value(token) { 2 } else { 1 };
    }
    None
}

/// Whether the option token `token` consumes the next argv token as its
/// value.
fn option_consumes_value(token: &str) -> bool {
    if token.starts_with("--") {
        long_option_consumes_value(token)
    } else {
        let Some(rest) = token.strip_prefix('-').filter(|rest| !rest.is_empty()) else {
            return false;
        };
        short_cluster_consumes_value(rest, |short| top_level_arity().short_consumes_value(short))
    }
}

/// Whether a long option consumes the next argv token as its value.
/// Known option arity comes from clap's top-level grammar. `--no-`
/// negations don't consume a value, and an inline `--opt=val` carries its
/// own value. Unknown long options are assumed to consume a value.
fn long_option_consumes_value(token: &str) -> bool {
    if token.contains('=') {
        return false;
    }
    let name = &token[2..];
    if name.starts_with("no-") {
        return false;
    }
    top_level_arity().long_consumes_value(name).unwrap_or(true)
}

fn top_level_arity() -> &'static ArgTable {
    static ARITY: std::sync::OnceLock<ArgTable> = std::sync::OnceLock::new();
    ARITY.get_or_init(|| ArgTable::top_level(grammar()))
}

#[cfg(test)]
mod tests;
