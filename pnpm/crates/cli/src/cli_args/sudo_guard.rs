//! Refuse commands that write to home-directory locations (global
//! installs, `pnpm setup`, `pnpm self-update`) when pnpm is executed
//! through `sudo`. Those commands would target root's home directory,
//! which is never what a user coming from `sudo npm install -g` wants.
//!
//! `checkSudo` in pnpm v11's `pnpm11/pnpm/src/checkSudo.ts` detects the
//! same commands under the same condition, but only warns there:
//! refusing them is a breaking change, so it lands in v12 while v11
//! gives users a release to migrate.

use super::cli_command::CliCommand;

#[cfg(unix)]
use super::config::{ConfigArgs, ConfigSubcommand};
#[cfg(unix)]
use derive_more::{Display, Error};
#[cfg(unix)]
use miette::Diagnostic;

pub(crate) fn check_sudo(command: &CliCommand) -> miette::Result<()> {
    #[cfg(unix)]
    {
        // SAFETY: geteuid is always safe to call.
        let euid = unsafe { libc::geteuid() };
        let sudo_user = std::env::var("SUDO_USER").ok();
        check_sudo_as(command, euid, sudo_user.as_deref())?;
    }
    #[cfg(not(unix))]
    let _ = command;
    Ok(())
}

#[cfg(unix)]
const SUDO_HINT: &str = "pnpm installs global packages and writes global configuration inside your home directory, so they do not require root permissions, and running this command as root would target the root user's home directory instead of yours. Rerun the command without sudo. If you really intend to manage the root user's own global packages, run pnpm from a session where the SUDO_USER environment variable is not set (for example: sudo env -u SUDO_USER pnpm ...).";

#[cfg(unix)]
#[derive(Debug, Display, Error, Diagnostic)]
#[display("Running \"{operation}\" with sudo is not supported")]
#[diagnostic(code(ERR_PNPM_SUDO_NOT_SUPPORTED), help("{SUDO_HINT}"))]
pub struct SudoNotSupportedError {
    operation: String,
}

#[cfg(unix)]
fn check_sudo_as(
    command: &CliCommand,
    euid: libc::uid_t,
    sudo_user: Option<&str>,
) -> Result<(), SudoNotSupportedError> {
    if euid != 0 {
        return Ok(());
    }
    if !sudo_user.is_some_and(|user| !user.is_empty() && user != "root") {
        return Ok(());
    }
    match sudo_blocked_operation(command) {
        Some(operation) => Err(SudoNotSupportedError { operation }),
        None => Ok(()),
    }
}

/// The user-facing name of the blocked operation, or `None` when the
/// command is allowed under sudo. Global commands that only read
/// (`pnpm bin -g`, `pnpm list -g`, `pnpm config get -g`, ...) stay
/// allowed.
#[cfg(unix)]
fn sudo_blocked_operation(command: &CliCommand) -> Option<String> {
    let global_write = |global: bool, name: &str| global.then(|| format!("pnpm {name} --global"));
    match command {
        CliCommand::Setup(_) => Some("pnpm setup".to_string()),
        CliCommand::SelfUpdate(_) => Some("pnpm self-update".to_string()),
        CliCommand::Add(args) => global_write(args.global, "add"),
        CliCommand::ApproveBuilds(args) => global_write(args.global, "approve-builds"),
        CliCommand::Remove(args) => global_write(args.global, "remove"),
        CliCommand::Runtime(args) => global_write(args.global, "runtime"),
        CliCommand::Update(args) => global_write(args.global, "update"),
        // `pnpm link` with no arguments links the current project into the
        // global directory.
        CliCommand::Link(args) if args.package_paths.is_empty() => {
            Some("pnpm link --global".to_string())
        }
        // Config writes default to the global config file when no
        // `--location` is given, so gate on the effective scope, not the
        // `--global` flag alone.
        CliCommand::Config(ConfigArgs { command: ConfigSubcommand::Set(args), .. }) => {
            global_write(super::config::resolve_global(args.flags), "config set")
        }
        CliCommand::Config(ConfigArgs { command: ConfigSubcommand::Delete(args), .. }) => {
            global_write(super::config::resolve_global(args.flags), "config delete")
        }
        _ => None,
    }
}

#[cfg(all(test, unix))]
mod tests;
