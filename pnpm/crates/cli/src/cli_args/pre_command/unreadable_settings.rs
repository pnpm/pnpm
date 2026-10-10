use super::{
    ConfigLoad, ConfigOverrides, Path, PreCommandAction, PreCommandInput, PreCommandPlan,
    SwitchProcessState, load_pre_command_config, resolve_pin,
};

/// Switch to the pinned pnpm when the configuration fails to load, since
/// the pinned one may read what this one cannot. The configuration is read
/// again without what this pnpm cannot read (see
/// [`pnpm_config::Config::skip_unreadable_settings`]): no setting may keep a
/// project from the pnpm it pins, while every source this pnpm can read,
/// such as the machine's `pmOnFail`, registry, and credentials, still
/// applies. `None` leaves the failure to be reported.
pub(super) fn switch_past_unreadable_settings(
    input: &PreCommandInput,
    config_overrides: &ConfigOverrides,
    dir: &Path,
    process_state: SwitchProcessState,
) -> Option<PreCommandPlan> {
    let load = ConfigLoad { skip_unreadable_settings: true, ..ConfigLoad::default() };
    let config = load_pre_command_config(input, config_overrides, dir, load).ok()?;
    match resolve_pin(input, config_overrides, dir, process_state, config, load).ok()?.action {
        PreCommandAction::Switch(plan) => Some(PreCommandPlan::Switch(plan)),
        PreCommandAction::Continue { .. } => None,
    }
}
