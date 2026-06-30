//! `pacquet with <version|current> <args...>` — run pnpm at a specific
//! version (or the currently running one) for a single invocation,
//! ignoring the project's `packageManager` / `devEngines.packageManager`
//! pin.
//!
//! Ports pnpm's
//! [`with` command](https://github.com/pnpm/pnpm/blob/a33eeec9cd/pnpm11/engine/pm/commands/src/with/with.ts).
//! `with current <cmd>` is rewritten before clap parses argv (see
//! [`crate::with_current`]) into a direct dispatch of `<cmd>` with
//! `pmOnFail` forced to `ignore`, so this handler only ever sees a
//! version / range / dist-tag spec, which it resolves, installs into the
//! global virtual store, and spawns.

mod install_pnpm_to_store;

use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pacquet_config::Config;
use pacquet_reporter::Reporter;
use std::{ffi::OsString, fs, path::Path, process::Command};

use crate::config_deps;

/// Errors specific to `pacquet with`. Codes mirror pnpm's `PnpmError`
/// codes (pnpm prefixes them with `ERR_PNPM_`).
#[derive(Debug, Display, Error, Diagnostic)]
pub enum WithError {
    #[display("Missing version argument. Usage: pnpm with <version|current> <args...>")]
    #[diagnostic(code(ERR_PNPM_MISSING_WITH_SPEC))]
    MissingSpec,

    #[display(r#"The "pnpm with" command does not work under corepack"#)]
    #[diagnostic(code(ERR_PNPM_CANT_USE_WITH_IN_COREPACK))]
    CantUseWithInCorepack,

    #[display(r#"Cannot resolve pnpm version for "{spec}""#)]
    #[diagnostic(code(ERR_PNPM_CANNOT_RESOLVE_PNPM))]
    CannotResolvePnpm { spec: String },

    #[display("Unable to find the global packages directory")]
    #[diagnostic(
        code(ERR_PNPM_NO_GLOBAL_BIN_DIR),
        help(
            r#"Run "pnpm setup" to create it automatically, or set the global-bin-dir setting, or the PNPM_HOME env variable."#
        )
    )]
    NoGlobalDir,

    #[display(
        "Cannot add {dir} to PATH because it contains the path delimiter character ({delimiter})"
    )]
    #[diagnostic(code(ERR_PNPM_BAD_PATH_DIR))]
    BadPathDir { dir: String, delimiter: char },
}

#[derive(Debug, Args)]
pub struct WithArgs {
    /// The pnpm version, range, or dist-tag to run, followed by the pnpm
    /// command and its arguments. (`with current <args>` is handled before
    /// argv parsing, so it never reaches this handler.)
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    pub params: Vec<String>,
}

impl WithArgs {
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        config: &'static Config,
    ) -> miette::Result<()> {
        let Some((spec, args)) = self.params.split_first() else {
            return Err(WithError::MissingSpec.into());
        };
        if is_executed_by_corepack() {
            return Err(WithError::CantUseWithInCorepack.into());
        }

        let resolved = config_deps::resolve_pnpm_version(config, spec)
            .await?
            .ok_or_else(|| WithError::CannotResolvePnpm { spec: spec.clone() })?;

        let env_root = config.global_pkg_dir.clone().ok_or(WithError::NoGlobalDir)?;
        fs::create_dir_all(&env_root)
            .into_diagnostic()
            .wrap_err("create the global packages directory")?;

        let bin_dir = Box::pin(install_pnpm_to_store::install_pnpm_to_store::<Reporter>(
            config,
            &env_root,
            spec,
            &resolved.version,
        ))
        .await?;

        let status = spawn_pnpm(&bin_dir, args)?;
        if !status.success() {
            // Propagate the child's exit code. A signal-terminated child
            // has no code; fall back to 1, matching pnpm's `exitCode ?? 1`.
            std::process::exit(status.code().unwrap_or(1));
        }
        Ok(())
    }
}

/// Spawn the downloaded `pnpm` with `bin_dir` prepended to `PATH` and the
/// child's package-manager check disabled, inheriting stdio. Mirrors the
/// `crossSpawn.sync` at the end of pnpm's `with` handler.
fn spawn_pnpm(bin_dir: &Path, args: &[String]) -> miette::Result<std::process::ExitStatus> {
    let path = prepend_to_path(bin_dir)?;
    // Resolve `pnpm` strictly within `bin_dir`, never the full PATH, so a
    // missing or broken shim is an error rather than silently falling
    // through to a different `pnpm` elsewhere on PATH (which would run the
    // wrong engine). Mirrors pnpm's `with`, which spawns the explicit
    // `path.join(binDir, 'pnpm')`; `which_in` is used only to pick the
    // platform-correct shim name (e.g. `pnpm.cmd` on Windows).
    let program = which::which_in("pnpm", Some(bin_dir), bin_dir)
        .into_diagnostic()
        .wrap_err("locate the requested pnpm binary in the engine's bin directory")?;

    let mut cmd = Command::new(program);
    cmd.args(args);
    // Drop any inherited PATH-like key before re-inserting our own, so a
    // Windows `Path`/`PATH` pair can't collapse to an unspecified winner.
    cmd.env_remove("PATH");
    cmd.env_remove("Path");
    cmd.env("PATH", &path);
    // The child pnpm must skip the packageManager / devEngines check so the
    // requested version stays active. `COREPACK_ROOT` is honored by every
    // pnpm release that supports corepack (older versions skip the check
    // whenever it is set); `pnpm_config_pm_on_fail=ignore` is the
    // principled override for releases that ship the `pmOnFail` setting.
    if std::env::var_os("COREPACK_ROOT").is_none() {
        cmd.env("COREPACK_ROOT", "pnpm-with");
    }
    cmd.env("pnpm_config_pm_on_fail", "ignore");

    cmd.status().into_diagnostic().wrap_err("run the requested pnpm version")
}

/// Prepend `dir` to the current process `PATH`, rejecting a `dir` that
/// contains the platform path delimiter (it cannot be expressed as a
/// single `PATH` entry and would silently split into several). Mirrors the
/// `BAD_PATH_DIR` guard `exec`'s `prepend_dirs_to_path` already applies;
/// `dir` here is the engine's store-resident `bin` directory.
fn prepend_to_path(dir: &Path) -> Result<OsString, WithError> {
    let delimiter = if cfg!(windows) { ';' } else { ':' };
    if dir.to_string_lossy().contains(delimiter) {
        return Err(WithError::BadPathDir { dir: dir.to_string_lossy().into_owned(), delimiter });
    }
    let mut out = OsString::from(dir);
    if let Some(current) = std::env::var_os("PATH").filter(|value| !value.is_empty()) {
        out.push(if cfg!(windows) { ";" } else { ":" });
        out.push(current);
    }
    Ok(out)
}

/// `true` when pnpm is running under corepack (which sets `COREPACK_ROOT`
/// and manages its own version switching). Mirrors pnpm's
/// `isExecutedByCorepack`.
fn is_executed_by_corepack() -> bool {
    std::env::var_os("COREPACK_ROOT").is_some()
}

#[cfg(test)]
mod tests;
