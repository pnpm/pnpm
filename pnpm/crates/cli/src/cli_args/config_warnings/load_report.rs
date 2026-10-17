//! Reporting the warnings of the configuration a command loads.

use super::{emit_config_warning, key_issues_fail_when_strict, report_workspace_key_issues};
use crate::cli_args::{
    cli_command::CliCommand,
    config::ConfigSubcommand,
    package_manager::{PinRoots, pins_running_pnpm, read_root_manifest, wanted_package_manager},
    pre_command::{is_global, should_skip_command},
};
use pnpm_config::Config;
use pnpm_network::redact_and_sanitize;
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

/// What to do about the warnings of the configuration a command loads,
/// decided from its command line.
#[derive(Clone, Copy)]
pub(crate) struct ConfigWarnings {
    key_issues: KeyIssueReporting,
    /// A `--global` invocation does not act on the project, so a satisfied
    /// pin does not harden its unrecognized-key report into an error.
    global: bool,
}

/// What to do about the problem keys of the project's `pnpm-workspace.yaml`
/// (see [`report_workspace_key_issues`]).
#[derive(Clone, Copy, PartialEq)]
enum KeyIssueReporting {
    /// `pnpm config get <key>` prints one value for a script to capture, so
    /// config-load warnings stay off it entirely, as pnpm keeps them off. A
    /// command that skips the package-manager checks reports none either.
    Skip,
    /// The other `pnpm config` subcommands are how a user inspects and
    /// repairs the config, so they must keep working on a broken file:
    /// unrecognized keys warn even under a satisfied pin.
    WarnOnly,
    /// Unrecognized keys warn, or fail the command when the running pnpm is
    /// the version the project pins (no version-skew excuse remains).
    Enforce,
}

/// Set once the warnings are reported: a command may load its configuration
/// more than once.
static REPORTED: AtomicBool = AtomicBool::new(false);

impl ConfigWarnings {
    pub(crate) fn of(command: &CliCommand) -> Self {
        let key_issues = match command {
            _ if should_skip_command(command) => KeyIssueReporting::Skip,
            CliCommand::Get(get) if get.args.key.is_some() => KeyIssueReporting::Skip,
            CliCommand::Config(args) => match &args.command {
                ConfigSubcommand::Get(get) if get.key.is_some() => KeyIssueReporting::Skip,
                _ => KeyIssueReporting::WarnOnly,
            },
            CliCommand::Get(_) | CliCommand::Set(_) => KeyIssueReporting::WarnOnly,
            _ => KeyIssueReporting::Enforce,
        };
        Self { key_issues, global: is_global(command) }
    }

    /// What `pnpm --version` reports: it must work on a broken
    /// `pnpm-workspace.yaml`.
    pub(crate) fn for_version_flag() -> Self {
        Self { key_issues: KeyIssueReporting::WarnOnly, global: false }
    }

    /// What a command line clap rejected reports: none, as the command does
    /// not run.
    pub(crate) fn for_unparsed_argv() -> Self {
        Self { key_issues: KeyIssueReporting::Skip, global: false }
    }

    /// Report the warnings `config`, loaded for the project in `dir`,
    /// collected. Only the first call reports.
    pub(crate) fn report(self, config: &Config, dir: &Path) -> miette::Result<()> {
        if self.key_issues == KeyIssueReporting::Skip || REPORTED.swap(true, Ordering::Relaxed) {
            return Ok(());
        }
        emit_npmrc_warnings(&config.npmrc_warnings);
        report_workspace_key_issues(&config.workspace_key_issues, self.strict(config, dir))
    }

    /// Whether [`Self::report`] fails the command, which reports nothing.
    pub(crate) fn fail(self, config: &Config, dir: &Path) -> bool {
        key_issues_fail_when_strict(&config.workspace_key_issues) && self.strict(config, dir)
    }

    /// Whether an unrecognized key fails the command: the running pnpm is
    /// the version the project pins, so no version-skew excuse remains.
    fn strict(self, config: &Config, dir: &Path) -> bool {
        self.key_issues == KeyIssueReporting::Enforce
            && !self.global
            && runs_the_pinned_pnpm(config, dir)
    }

    /// Report the `.npmrc` warnings a load that failed collected, since they
    /// often explain the failure.
    pub(crate) fn report_failed_load(self, warnings: &[String]) {
        if self.key_issues != KeyIssueReporting::Skip && !REPORTED.swap(true, Ordering::Relaxed) {
            emit_npmrc_warnings(warnings);
        }
    }
}

fn emit_npmrc_warnings(warnings: &[String]) {
    for warning in warnings {
        emit_config_warning(&redact_and_sanitize(warning));
    }
}

/// Whether the running pnpm is the version the project in `dir` pins.
fn runs_the_pinned_pnpm(config: &Config, dir: &Path) -> bool {
    let manifest = read_root_manifest(&PinRoots::of(config, dir).manifest);
    pins_running_pnpm(
        manifest
            .as_ref()
            .and_then(wanted_package_manager)
            .as_ref(),
    )
}
