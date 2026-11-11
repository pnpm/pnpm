//! Handing the command line to the pnpm a project pins.
//!
//! Each answer here is a `bool`: whether the pinned pnpm took the
//! command, in which case it has already run and this process is done.

use super::{ConfigOverrides, block_on_runtime, cli_args};
use std::ffi::OsString;

/// Whether the pinned pnpm answered `--version` for this one.
///
/// Unlike the other pre-command checks, a pinned pnpm this process cannot
/// install only warns. Printing a version has to work under a read-only
/// home, where installing it and recording the pin have nowhere to write.
pub(super) fn pinned_pnpm_printed_the_version(
    argv: &[OsString],
    child_argv: &[OsString],
    config_overrides: &ConfigOverrides,
) -> miette::Result<bool> {
    let Some(plan) =
        cli_args::pre_command::pre_command_plan_for_version_flag(argv, config_overrides)?
    else {
        return Ok(false);
    };
    block_on_runtime("pacquet-pre-command", cli_args::pre_command::execute_plan(plan, child_argv))
        .or_else(|error| {
            cli_args::pre_command::warn_pinned_pnpm_unusable(&error);
            Ok(false)
        })
}

/// Whether the pnpm the project pins took the command. When it did, it has
/// already run to completion and this process has nothing left to do.
pub(super) fn dispatched_to_pinned_pnpm(
    invocation: &cli_args::pre_command::Invocation,
    config_overrides: &ConfigOverrides,
    child_argv: &[OsString],
    preloaded_config: &mut Option<Result<pnpm_config::Config, pnpm_config::ConfigLoadFailure>>,
) -> miette::Result<bool> {
    let outcome = cli_args::pre_command::pre_command_plan(invocation, config_overrides)?;
    *preloaded_config = outcome.preloaded_config;
    let Some(plan) = outcome.plan else {
        return Ok(false);
    };
    match block_on_runtime(
        "pacquet-pre-command",
        cli_args::pre_command::execute_plan_reusing_config(plan, child_argv),
    )? {
        cli_args::pre_command::PlanExecution::Switched => Ok(true),
        cli_args::pre_command::PlanExecution::Continue(config) => {
            if let Some(config) = *config {
                *preloaded_config = Some(Ok(config));
            }
            Ok(false)
        }
    }
}

/// Whether the pnpm the project pins took a command line this one rejected.
/// A newer or older pnpm may accept an option this one does not know.
pub(super) fn dispatched_unparsed_to_pinned_pnpm(
    argv: &[OsString],
    child_argv: &[OsString],
    config_overrides: &ConfigOverrides,
) -> miette::Result<bool> {
    let plan = cli_args::pre_command::switch_plan_for_unparsed_argv(argv, config_overrides)?;
    execute_pre_command_plan(plan, child_argv)
}

fn execute_pre_command_plan(
    plan: Option<cli_args::pre_command::PreCommandPlan>,
    child_argv: &[OsString],
) -> miette::Result<bool> {
    let Some(plan) = plan else {
        return Ok(false);
    };
    block_on_runtime("pacquet-pre-command", cli_args::pre_command::execute_plan(plan, child_argv))
}
