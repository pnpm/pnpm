use super::{
    Config, ConfigOverrides, Context, Host, Path, PreCommandInput, apply_state_dir_override,
    apply_store_dir_override, configure_color, seed_config,
};
use pnpm_config::SettingsRead;

/// What [`load_pre_command_config`] reads.
#[derive(Clone, Copy)]
pub(super) struct ConfigLoad {
    settings_read: SettingsRead,
    /// Place the store, which a switch or a sync uses.
    resolve_store: bool,
}

impl ConfigLoad {
    /// What deciding whether to switch reads.
    pub(super) const PIN_DECISION: Self =
        Self { settings_read: SettingsRead::PinDecision, resolve_store: false };

    /// What a switch reads: everything this pnpm can read, since the pnpm
    /// the project pins may read what this one cannot.
    pub(super) const SWITCH: Self =
        Self { settings_read: SettingsRead::Readable, resolve_store: true };

    /// What recording the pin reads: the configuration the command runs
    /// with.
    pub(super) const RECORD: Self = Self { settings_read: SettingsRead::All, resolve_store: true };
}

/// Load the configuration the pre-command pass reads, with the global
/// CLI flags that reach it applied.
pub(super) fn load_pre_command_config(
    input: &PreCommandInput,
    config_overrides: &ConfigOverrides,
    dir: &Path,
    load: ConfigLoad,
) -> miette::Result<Config> {
    let switch = &input.switch;
    let mut config = seed_config(
        switch.paths.npmrc_auth_file.as_deref(),
        switch.paths.ignore_workspace,
        config_overrides,
        switch.network.registry.as_deref(),
    );
    config.skip_store_dir_resolution = !load.resolve_store;
    config.settings_read = load.settings_read;
    let mut config = config
        .current_keeping_warnings::<Host>(dir)
        .map_err(|failure| miette::Report::new(failure.error))
        .wrap_err("load configuration")?;
    config_overrides.apply(&mut config, dir);
    switch.network.apply(&mut config);
    if let Some(color) = switch.color {
        config.color = color;
    }
    configure_color(config.color);
    if config.ci {
        pnpm_default_reporter::force_append_only();
    }
    if let Some(store_dir) = switch.paths.store_dir.as_deref() {
        apply_store_dir_override::<Host>(&mut config, store_dir, dir)?;
    }
    if let Some(state_dir) = switch.paths.state_dir.as_deref() {
        apply_state_dir_override::<Host>(&mut config, state_dir, dir);
    }
    // `--lockfile-dir` moves the lockfile the pin is recorded in, and
    // `--offline` governs how that record is resolved. Both are
    // install-family flags, and the record below is made for every
    // command.
    switch.pin_flags.apply_to(&mut config, dir);
    Ok(config)
}
