//! The verify-deps-before-run gate: before `pnpm run` / `pnpm exec`
//! execute anything, verify that `node_modules` is in sync with the
//! lockfile and apply the configured action — spawn an install, prompt
//! for one, error out, or warn. pnpm's counterpart is
//! `runDepsStatusCheck` in `exec/commands`.

use std::{
    io::IsTerminal,
    path::Path,
    process::{Command, exit},
};

use derive_more::{Display, Error};
use dialoguer::Confirm;
use miette::{Diagnostic, IntoDiagnostic};
use pacquet_config::{Config, VerifyDepsBeforeRun};
use pacquet_default_reporter::colors::Colors;
use pacquet_package_manager::{RunDepsStatus, check_deps_status_before_run_at};

#[derive(Debug, Display, Error, Diagnostic)]
enum VerifyDepsError {
    #[display("{issue}")]
    #[diagnostic(code(ERR_PNPM_VERIFY_DEPS_BEFORE_RUN), help(r#"Run "pnpm install""#))]
    OutOfSync { issue: String },

    #[display("{issue}")]
    #[diagnostic(
        code(ERR_PNPM_VERIFY_DEPS_BEFORE_RUN),
        help(
            r#"Run "pnpm install" before running scripts. The "verifyDepsBeforeRun: prompt" setting cannot prompt for confirmation in non-interactive environments."#
        )
    )]
    CannotPrompt { issue: String },
}

/// Run the configured verify-deps-before-run action for the project at
/// `dir`. `Ok(())` means the script may proceed — including after a
/// spawned install, a declined prompt, or a warning.
#[expect(clippy::exit, reason = "an interrupted prompt exits 1, like pnpm's ExitPromptError")]
pub(crate) fn verify_deps_before_run(
    dir: &Path,
    config: &Config,
    silent: bool,
) -> miette::Result<()> {
    if !config.verify_deps_before_run.is_enabled() {
        return Ok(());
    }
    let Some(status) = check_deps_status_before_run_at(dir, config) else {
        return Ok(());
    };
    let (issue, install_args) = match status {
        RunDepsStatus::UpToDate => return Ok(()),
        RunDepsStatus::SkippedPnp => {
            warn(silent, "verify-deps-before-run does not work with node-linker=pnp");
            return Ok(());
        }
        RunDepsStatus::Outdated { issue, install_args } => (issue, install_args),
    };
    match config.verify_deps_before_run {
        VerifyDepsBeforeRun::Install => spawn_install(dir, &install_args, silent),
        VerifyDepsBeforeRun::Prompt => {
            if !std::io::stdin().is_terminal() {
                return Err(VerifyDepsError::CannotPrompt { issue }.into());
            }
            let command = std::iter::once("install")
                .chain(install_args.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" ");
            let message = format!(
                "Your \"node_modules\" directory is out of sync with the \"pnpm-lock.yaml\" file. This can lead to issues during scripts execution.\n\nWould you like to run \"pnpm {command}\" to update your \"node_modules\"?"
            );
            match Confirm::new().with_prompt(message).default(true).interact() {
                Ok(true) => spawn_install(dir, &install_args, silent),
                Ok(false) => Ok(()),
                // The prompt was interrupted (Esc / Ctrl-C); exit like
                // pnpm's ExitPromptError handler.
                Err(_) => exit(1),
            }
        }
        VerifyDepsBeforeRun::Error => Err(VerifyDepsError::OutOfSync { issue }.into()),
        VerifyDepsBeforeRun::Warn => {
            warn(silent, &format!("Your node_modules are out of sync with your lockfile. {issue}"));
            Ok(())
        }
        // `true` runs the check without acting on the verdict; `false`
        // returned before the check.
        VerifyDepsBeforeRun::True | VerifyDepsBeforeRun::False => Ok(()),
    }
}

/// Re-run the kind of install the workspace state recorded, in-place
/// and with inherited stdio, the way pnpm's `runDepsStatusCheck` spawns
/// `pnpm install` through `runPnpmCli`. The spawned install never
/// re-enters this gate: only `run` / `exec` consult it.
#[expect(clippy::exit, reason = "a failed spawned install must preserve the child exit code")]
fn spawn_install(dir: &Path, install_args: &[String], silent: bool) -> miette::Result<()> {
    let exe = std::env::current_exe().into_diagnostic()?;
    let mut command = Command::new(exe);
    command.arg("install").args(install_args).current_dir(dir);
    if silent {
        command.arg("--reporter=silent");
    }
    let status = command.status().into_diagnostic()?;
    if !status.success() {
        // The child already reported its own failure; propagate its exit
        // code without a second error dump (`exitCode ?? 1`, like the
        // exec path).
        exit(status.code().unwrap_or(1));
    }
    Ok(())
}

/// Print a `globalWarn`-shaped line to stderr. The gate runs before any
/// reporter pipeline exists, so it renders the label the way the
/// default reporter would.
fn warn(silent: bool, message: &str) {
    if silent {
        return;
    }
    let colors = Colors {
        enabled: std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none(),
    };
    eprintln!("{} {message}", colors.warn_label());
}
