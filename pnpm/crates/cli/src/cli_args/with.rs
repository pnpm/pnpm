//! `pacquet with <version|current> <args...>` — run pnpm at a specific
//! version (or the currently running one) for a single invocation,
//! ignoring the project's `packageManager` / `devEngines.packageManager`
//! pin.
//!
//! This is about pnpm's own version. Running one of the *other* package
//! managers is `pnpm dlx <pm>@<spec>` (`pnx yarn@4 install`), which
//! provisions it through the same engine installer.
//!
//! `with current <cmd>` is rewritten before clap parses argv (see
//! [`crate::with_current`]) into a direct dispatch of `<cmd>` with
//! `pmOnFail` forced to `ignore`, so this handler only ever sees a
//! version / range / dist-tag spec, which it resolves, installs into the
//! global virtual store, and spawns.

use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_reporter::Reporter;
use std::{path::PathBuf, process::Command};

use crate::{
    cli_args::package_manager::PACKAGE_MANAGER_SWITCH_ENV_VARS,
    engine_pm::{
        channel::PackageManager,
        error::EngineError,
        provision::{engine_bin, provision},
    },
    path_env::{BadPathDir, prepend_dirs_to_path, set_command_path},
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

impl From<BadPathDir> for WithError {
    fn from(BadPathDir { dir, delimiter }: BadPathDir) -> Self {
        WithError::BadPathDir { dir, delimiter }
    }
}

#[derive(Debug, Args)]
pub struct WithArgs {
    /// The pnpm version, range, or dist-tag to run — or `current` to use
    /// the pnpm that is already running — followed by the pnpm command and
    /// its arguments.
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

        let engine = Box::pin(provision::<Reporter>(config, PackageManager::Pnpm, spec)).await?;

        let status = spawn_pnpm(&engine.bin_dirs, args, PackageManagerCheck::Disabled)?;
        if !status.success() {
            // Propagate the child's exit code. A signal-terminated child
            // has no code; fall back to 1, matching pnpm's `exitCode ?? 1`.
            std::process::exit(status.code().unwrap_or(1));
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub(crate) enum PackageManagerCheck {
    Enabled,
    Disabled,
}

/// Spawn the downloaded `pnpm`, inheriting stdio. The first entry is the
/// engine's own bin directory; any that follow are what it needs to run,
/// such as a managed Node.js.
pub(crate) fn spawn_pnpm<Args, Arg>(
    bin_dirs: &[PathBuf],
    args: Args,
    package_manager_check: PackageManagerCheck,
) -> miette::Result<std::process::ExitStatus>
where
    Args: IntoIterator<Item = Arg>,
    Arg: AsRef<std::ffi::OsStr>,
{
    let bin_dir = bin_dirs.first().expect("an installed engine has a bin directory");
    let program = engine_bin(bin_dir, "pnpm").ok_or_else(|| EngineError::MissingEngineBin {
        name: "pnpm",
        dir: bin_dir.display().to_string(),
    })?;

    let mut cmd = Command::new(program);
    cmd.args(args);
    configure_pnpm_environment(&mut cmd, bin_dirs, package_manager_check)?;

    cmd.status().into_diagnostic().wrap_err("run the requested pnpm version")
}

fn configure_pnpm_environment(
    cmd: &mut Command,
    bin_dirs: &[PathBuf],
    package_manager_check: PackageManagerCheck,
) -> miette::Result<()> {
    if matches!(package_manager_check, PackageManagerCheck::Disabled) {
        let path = prepend_dirs_to_path(bin_dirs).map_err(WithError::from)?;
        set_command_path(cmd, &path);
        disable_package_manager_switching(cmd);
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
    Ok(())
}

fn disable_package_manager_switching(cmd: &mut Command) {
    for name in PACKAGE_MANAGER_SWITCH_ENV_VARS {
        cmd.env(name, "false");
    }
}

/// `true` when pnpm is running under corepack (which sets `COREPACK_ROOT`
/// and manages its own version switching).
fn is_executed_by_corepack() -> bool {
    std::env::var_os("COREPACK_ROOT").is_some()
}

#[cfg(test)]
mod tests;
