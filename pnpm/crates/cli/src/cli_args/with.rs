//! `pacquet with <spec|current> <args...>` — run a package manager at a
//! specific version for a single invocation, ignoring the project's
//! `packageManager` / `devEngines.packageManager` pin.
//!
//! The spec is a pnpm version, range, or dist-tag, or another package
//! manager as `<name>@<spec>` (`yarn@4`, `npm@10`, `bun`).
//!
//! `with current <cmd>` is rewritten before clap parses argv (see
//! [`crate::with_current`]) into a direct dispatch of `<cmd>` with
//! `pmOnFail` forced to `ignore`, so this handler only ever sees a spec,
//! which it resolves, installs into the global virtual store, and spawns.

use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pacquet_config::Config;
use pacquet_reporter::Reporter;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

use crate::{
    cli_args::package_manager::PACKAGE_MANAGER_SWITCH_ENV_VARS,
    engine_pm::{
        channel::PackageManager,
        provision::{ProvisionedEngine, provision},
    },
};

/// Errors specific to `pacquet with`. The codes carry the shared
/// `ERR_PNPM_` prefix.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum WithError {
    #[display("Missing version argument. Usage: pnpm with <version|current> <args...>")]
    #[diagnostic(code(ERR_PNPM_MISSING_WITH_SPEC))]
    MissingSpec,

    #[display(r#"The "pnpm with" command does not work under corepack"#)]
    #[diagnostic(code(ERR_PNPM_CANT_USE_WITH_IN_COREPACK))]
    CantUseWithInCorepack,

    #[display(
        "Cannot add {dir} to PATH because it contains the path delimiter character ({delimiter})"
    )]
    #[diagnostic(code(ERR_PNPM_BAD_PATH_DIR))]
    BadPathDir { dir: String, delimiter: char },
}

#[derive(Debug, Args)]
pub struct WithArgs {
    /// The package manager to run: a pnpm version, range, or dist-tag,
    /// another package manager as `<name>@<spec>`, or `current` to use the
    /// pnpm that is already running. Followed by the command and its
    /// arguments.
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
        let (pm, version_spec) = parse_engine_spec(spec);
        // Corepack owns which pnpm runs, so running a different one behind
        // its back is refused. It has no say over the other package
        // managers pnpm provisions, so those are unaffected.
        if pm == PackageManager::Pnpm && is_executed_by_corepack() {
            return Err(WithError::CantUseWithInCorepack.into());
        }

        let engine = Box::pin(provision::<Reporter>(config, pm, version_spec)).await?;

        let status = if pm == PackageManager::Pnpm {
            // The child is pnpm, so it has to be told not to switch itself
            // back to the project's pin.
            spawn_pnpm(&engine.bin_dirs[0], args, PackageManagerCheck::Disabled)?
        } else {
            spawn_engine(&engine, args)?
        };
        if !status.success() {
            // Propagate the child's exit code. A signal-terminated child
            // has no code; fall back to 1, matching pnpm's `exitCode ?? 1`.
            std::process::exit(status.code().unwrap_or(1));
        }
        Ok(())
    }
}

/// Split a `pnpm with` spec into the package manager to run and its
/// version specifier. A spec that names no package manager is a pnpm
/// version, range, or dist-tag — the original form of the command, so
/// `pnpm with 10.5.0` keeps meaning pnpm 10.5.0.
fn parse_engine_spec(spec: &str) -> (PackageManager, &str) {
    let (name, version_spec) = spec.split_once('@').unwrap_or((spec, "latest"));
    PackageManager::parse(name).map_or((PackageManager::Pnpm, spec), |pm| (pm, version_spec))
}

/// Spawn a provisioned package manager with its directories prepended to
/// `PATH`, inheriting stdio.
fn spawn_engine<Args, Arg>(
    engine: &ProvisionedEngine,
    args: Args,
) -> miette::Result<std::process::ExitStatus>
where
    Args: IntoIterator<Item = Arg>,
    Arg: AsRef<std::ffi::OsStr>,
{
    let path = prepend_to_path(&engine.bin_dirs)?;
    let mut cmd = Command::new(&engine.program);
    cmd.args(args);
    // Drop any inherited PATH-like key before re-inserting our own, so a
    // Windows `Path`/`PATH` pair can't collapse to an unspecified winner.
    cmd.env_remove("PATH");
    cmd.env_remove("Path");
    cmd.env("PATH", &path);
    cmd.status().into_diagnostic().wrap_err("run the requested package manager")
}

#[derive(Clone, Copy)]
pub(crate) enum PackageManagerCheck {
    Enabled,
    Disabled,
}

/// Spawn the downloaded `pnpm` with `bin_dir` prepended to `PATH`,
/// inheriting stdio.
pub(crate) fn spawn_pnpm<Args, Arg>(
    bin_dir: &Path,
    args: Args,
    package_manager_check: PackageManagerCheck,
) -> miette::Result<std::process::ExitStatus>
where
    Args: IntoIterator<Item = Arg>,
    Arg: AsRef<std::ffi::OsStr>,
{
    let path = prepend_to_path(&[bin_dir.to_path_buf()])?;
    // Resolve `pnpm` strictly within `bin_dir`, never the full PATH, so a
    // missing or broken shim is an error rather than silently falling
    // through to a different `pnpm` elsewhere on PATH (which would run the
    // wrong engine). `which_in` is used only to pick the platform-correct
    // shim name (e.g. `pnpm.cmd` on Windows).
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
    disable_package_manager_switching(&mut cmd);
    if matches!(package_manager_check, PackageManagerCheck::Disabled) {
        // The child pnpm must skip the packageManager / devEngines check so the
        // requested version stays active. `COREPACK_ROOT` is honored by every
        // pnpm release that supports corepack (older versions skip the check
        // whenever it is set); `pnpm_config_pm_on_fail=ignore` is the
        // principled override for releases that ship the `pmOnFail` setting.
        if std::env::var_os("COREPACK_ROOT").is_none() {
            cmd.env("COREPACK_ROOT", "pnpm-with");
        }
        cmd.env("pnpm_config_pm_on_fail", "ignore");
    }

    cmd.status().into_diagnostic().wrap_err("run the requested pnpm version")
}

fn disable_package_manager_switching(cmd: &mut Command) {
    for name in PACKAGE_MANAGER_SWITCH_ENV_VARS {
        cmd.env(name, "false");
    }
}

/// Prepend `dirs` to the current process `PATH`, rejecting a directory
/// that contains the platform path delimiter (it cannot be expressed as a
/// single `PATH` entry and would silently split into several). Mirrors the
/// `BAD_PATH_DIR` guard `exec`'s `prepend_dirs_to_path` already applies;
/// the directories here are the engine's store-resident `bin` directory
/// and, for a JavaScript engine on a host without Node.js, the managed
/// runtime's.
pub(crate) fn prepend_to_path(dirs: &[PathBuf]) -> Result<OsString, WithError> {
    let delimiter = if cfg!(windows) { ';' } else { ':' };
    let separator = if cfg!(windows) { ";" } else { ":" };
    let mut out = OsString::new();
    for dir in dirs {
        if dir.to_string_lossy().contains(delimiter) {
            return Err(WithError::BadPathDir {
                dir: dir.to_string_lossy().into_owned(),
                delimiter,
            });
        }
        if !out.is_empty() {
            out.push(separator);
        }
        out.push(dir);
    }
    if let Some(current) = std::env::var_os("PATH").filter(|value| !value.is_empty()) {
        if !out.is_empty() {
            out.push(separator);
        }
        out.push(current);
    }
    Ok(out)
}

/// `true` when pnpm is running under corepack (which sets `COREPACK_ROOT`
/// and manages its own version switching).
fn is_executed_by_corepack() -> bool {
    std::env::var_os("COREPACK_ROOT").is_some()
}

#[cfg(test)]
mod tests;
