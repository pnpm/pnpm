use super::{
    ConfigLoad, ConfigOverrides, Path, PreCommandAction, PreCommandInput, PreCommandPlan,
    SwitchProcessState, load_pre_command_config, resolve_pin,
};

/// Switch to the pinned pnpm when the configuration fails to load, since
/// the pinned one may read what this one cannot. The configuration is read
/// again without the `pnpm-workspace.yaml` settings this pnpm cannot read,
/// and only when that fails too, with no configuration file at all: no
/// setting may keep a project from the pnpm it pins, while one this pnpm can
/// read, such as a `pmOnFail` that declines the switch, still decides.
/// `None` leaves the failure to be reported.
pub(super) fn switch_past_unreadable_settings(
    input: &PreCommandInput,
    config_overrides: &ConfigOverrides,
    dir: &Path,
    process_state: SwitchProcessState,
) -> Option<PreCommandPlan> {
    let readable = ConfigLoad { skip_unreadable_settings: true, ..ConfigLoad::default() };
    let (load, config) =
        if let Ok(config) = load_pre_command_config(input, config_overrides, dir, readable) {
            (readable, config)
        } else {
            let defaults = ConfigLoad { defaults_only: true, ..ConfigLoad::default() };
            (defaults, load_pre_command_config(input, config_overrides, dir, defaults).ok()?)
        };
    match resolve_pin(input, config_overrides, dir, process_state, config, load).ok()?.action {
        PreCommandAction::Switch(plan) => Some(PreCommandPlan::Switch(plan)),
        PreCommandAction::Continue { .. } => None,
    }
}
