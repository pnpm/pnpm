//! Rewrite `pnpm with current <cmd> [args]` before clap parses argv.
//!
//! `pnpm [global-opts] with current <cmd> [args]` is sugar for
//! `pnpm [global-opts] <cmd> [args]` run with the project's
//! `packageManager` / `devEngines.packageManager` check disabled. Ports
//! the `with current` branch of pnpm's
//! [`parseCliArgs`](https://github.com/pnpm/pnpm/blob/a33eeec9cd/pnpm11/pnpm/src/parseCliArgs.ts#L45-L56):
//! the `with current` tokens are removed in place (so any global flags
//! before `with` are preserved) and the override is propagated via the
//! `pnpm_config_pm_on_fail` env var rather than an argv flag, so it
//! survives clap's `-v` / `--version` short-circuit and reaches both the
//! in-process config load and any child process the command spawns.

use derive_more::{Display, Error};
use miette::Diagnostic;
use std::ffi::OsString;

/// Global long options that do **not** take a value, so a following
/// `with` is the command rather than the option's argument. Every other
/// long option (known value-takers like `--dir` / `--reporter`, and any
/// unknown flag) is assumed to consume its successor — matching pnpm's
/// `longOptionConsumesValue`, which treats a non-boolean (including
/// unknown) option as value-consuming.
const BOOLEAN_LONG_OPTIONS: &[&str] = &["recursive"];

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

/// The index of the `with` token of the first `with current` command pair
/// in `argv`, skipping a pair whose `with` is the argument of a preceding
/// value-taking long option. Mirrors pnpm's `findWithCurrentIndex`.
fn find_with_current_index(argv: &[OsString]) -> Option<usize> {
    // Start at 1 to skip the program name; stop before the last element so
    // `index + 1` is always in bounds.
    for index in 1..argv.len().saturating_sub(1) {
        if argv[index] != "with" || argv[index + 1] != "current" {
            continue;
        }
        if let Some(prev) = argv.get(index - 1)
            && long_option_consumes_value(prev)
        {
            continue;
        }
        return Some(index);
    }
    None
}

/// Whether `token` is a long option that consumes the next argv token as
/// its value. Booleans and `--no-` negations don't; an inline `--opt=val`
/// carries its own value. Unknown long options are assumed to consume a
/// value, matching pnpm.
fn long_option_consumes_value(token: &OsString) -> bool {
    let Some(token) = token.to_str() else {
        return false;
    };
    if !token.starts_with("--") || token.contains('=') {
        return false;
    }
    let name = &token[2..];
    if name.starts_with("no-") {
        return false;
    }
    !BOOLEAN_LONG_OPTIONS.contains(&name)
}

#[cfg(test)]
mod tests;
