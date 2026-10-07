use super::{
    Config, ConfigOverrides, Context, Host, KeyIssueReporting, Path, PreCommandInput,
    apply_state_dir_override, apply_store_dir_override, configure_color, emit_npmrc_warnings,
    seed_config,
};

/// What [`load_pre_command_config`] reads.
#[derive(Default, Clone, Copy)]
pub(super) struct ConfigLoad {
    /// Place the store, which a switch or a sync uses.
    pub(super) resolve_store: bool,
    /// See [`Config::skip_unreadable_workspace_settings`].
    pub(super) skip_unreadable_settings: bool,
}

/// Load the configuration the pre-command pass reads, with the global
/// CLI flags that reach it applied. A failed load still prints the
/// `.npmrc` warnings it collected, since they often explain the failure.
pub(super) fn load_pre_command_config(
    input: &PreCommandInput,
    config_overrides: &ConfigOverrides,
    dir: &Path,
    load: ConfigLoad,
) -> miette::Result<Config> {
    let switch = &input.switch;
    let mut config = seed_config(
        switch.paths.npmrc_auth_file.as_deref(),
        switch.ignore_workspace,
        config_overrides,
    );
    config.skip_store_dir_resolution = !load.resolve_store;
    config.skip_unreadable_workspace_settings = load.skip_unreadable_settings;
    let mut config = config
        .current_keeping_warnings::<Host>(dir)
        .map_err(|failure| {
            // A load that skips unreadable settings retries one that failed
            // and printed these already.
            if input.key_issues != KeyIssueReporting::Skip && !load.skip_unreadable_settings {
                emit_npmrc_warnings(&failure.warnings);
            }
            miette::Report::new(failure.error)
        })
        .wrap_err("load configuration")?;
    config_overrides.apply(&mut config, dir);
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
