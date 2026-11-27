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

/// Force `pmOnFail=ignore` for the in-process run and its children, via the
/// `pnpm_config_pm_on_fail` env var, when `argv` (program name at index 0)
/// contains a `with current` command pair.
///
/// `single_threaded` is what permits the environment change, so this runs
/// in `main`, before the startup thread exists.
pub fn ignore_pm_check_if_requested(
    argv: &[OsString],
    single_threaded: &pnpm_ffi_process::SingleThreaded,
) -> Result<(), pnpm_ffi_process::OtherThreads> {
    if !is_requested(argv) {
        return Ok(());
    }
    single_threaded.set_var("pnpm_config_pm_on_fail".to_owned(), "ignore".to_owned())
}

fn is_requested(argv: &[OsString]) -> bool {
    PreCommandPrefixes::scan(argv).with_current.is_some()
}

/// Rewrite `argv` (program name at index 0) when it contains a
/// `with current` command pair, returning the argv clap should parse. A
/// no-op when there is no such pair.
pub fn rewrite(mut argv: Vec<OsString>) -> miette::Result<Vec<OsString>> {
    let Some((with_index, current_index)) = PreCommandPrefixes::scan(&argv).with_current else {
        return Ok(argv);
    };
    // `with current` with nothing after it is a usage error.
    if argv.len() <= current_index + 1 {
        return Err(MissingWithCurrentCommand.into());
    }
    argv.remove(current_index);
    argv.remove(with_index);
    Ok(argv)
}

#[cfg(test)]
mod tests;
