//! Pre-command plans for command lines clap did not parse into a command,
//! read from argv directly.

use super::{
    ConfigOverrides, KeyIssueReporting, OsString, PreCommandInput, PreCommandPlan, SwitchInput,
    SwitchProcessState, input::UnparsedArgv, pre_command_plan_from_input,
};

/// The `pnpm --version` path, which clap answers before a command is
/// parsed. pnpm checks the package manager there too, but skips the runtime
/// checks — printing the version must work in a project whose runtime pin
/// the system cannot satisfy.
pub(crate) fn pre_command_plan_for_version_flag(
    argv: &[OsString],
    config_overrides: &ConfigOverrides,
) -> miette::Result<Option<PreCommandPlan>> {
    pre_command_plan_from_input(
        &PreCommandInput {
            switch: SwitchInput::from_version_argv(argv),
            global: false,
            skip_pm_handling: false,
            check_runtimes: false,
            reporter: SwitchInput::reporter_flags_from_version_argv(argv),
            // Printing the version must work in a project whose
            // `pnpm-workspace.yaml` is broken, like the runtime checks above.
            key_issues: KeyIssueReporting::WarnOnly,
        },
        config_overrides,
        SwitchProcessState::current(),
    )
}

/// A command line clap rejected for an argument this pnpm does not know.
/// The pnpm the project pins may know it, so the command goes to that pnpm
/// when the pin calls for a switch. Anything short of a switch leaves the
/// rejection to stand, and records nothing for a command that is not run.
pub(crate) fn switch_plan_for_unparsed_argv(
    argv: &[OsString],
    config_overrides: &ConfigOverrides,
) -> miette::Result<Option<PreCommandPlan>> {
    let Some(UnparsedArgv {
        switch, global, project_location, ..
    }) = UnparsedArgv::scan(argv)
    else {
        return Ok(None);
    };
    // The config commands check the pin only for `--location project`.
    if switch.command
        .as_deref()
        .is_some_and(|command| matches!(command, "config" | "get" | "set"))
        && !project_location
    {
        return Ok(None);
    }
    let plan = pre_command_plan_from_input(
        &PreCommandInput {
            switch,
            global,
            skip_pm_handling: false,
            check_runtimes: false,
            reporter: SwitchInput::reporter_flags_from_version_argv(argv),
            key_issues: KeyIssueReporting::Skip,
        },
        config_overrides,
        SwitchProcessState::current(),
    )?;
    Ok(plan.filter(|plan| matches!(plan, PreCommandPlan::Switch(_))))
}
