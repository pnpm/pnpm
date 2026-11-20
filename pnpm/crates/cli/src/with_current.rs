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

use crate::parse_boundary::PreCommandPrefixes;
use derive_more::{
    Display,
    Error,
};
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
///
/// `single_threaded` is what permits that environment change, and taking
/// it by value makes this the last one the startup path can make: the
/// caller cannot reach for the witness again once a runtime or a pool is
/// up.
#[expect(
    clippy::needless_pass_by_value,
    reason = "dropping the witness here is what ends the startup path's licence to change the environment"
)]
pub fn rewrite(
    argv: Vec<OsString>,
    single_threaded: pnpm_ffi_process::SingleThreaded,
) -> miette::Result<Vec<OsString>> {
    let (argv, force_pm_on_fail_ignore) = plan(argv)?;
    if force_pm_on_fail_ignore {
        single_threaded.set_var("pnpm_config_pm_on_fail", "ignore");
    }
    Ok(argv)
}

/// The pure core of [`rewrite`]: returns the rewritten argv and whether a
/// `with current` pair was stripped (in which case the caller must force
/// `pmOnFail=ignore`). Split out so the argv transformation is testable
/// without mutating the process environment.
fn plan(mut argv: Vec<OsString>) -> miette::Result<(Vec<OsString>, bool)> {
    let Some((with_index, current_index)) = PreCommandPrefixes::scan(&argv).with_current else {
        return Ok((argv, false));
    };
    // `with current` with nothing after it is a usage error.
    if argv.len() <= current_index + 1 {
        return Err(MissingWithCurrentCommand.into());
    }
    argv.remove(current_index);
    argv.remove(with_index);
    Ok((argv, true))
}

#[cfg(test)]
mod tests;
