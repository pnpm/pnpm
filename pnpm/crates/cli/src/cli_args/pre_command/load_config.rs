#[cfg(test)]
pub(in crate::cli_args) mod tests;

use super::{Config, ConfigOverrides, Context, Host, Invocation, Path};
use crate::{
    cli_args::{install::resolve_bool_override, reporter::configure_color},
    config_overrides::{apply_state_dir_override, apply_store_dir_override},
};
use pnpm_config::{ColorMode, ConfigLoadFailure, SettingsRead};

/// The config every load starts from. `npmrc_auth_file` is seeded from the
/// CLI flag before `current()` reads `.npmrc`, so the override redirects the
/// user-level read. Mirrors pnpm's `--npmrc-auth-file`. Production callers
/// turbofish `Host` explicitly so the dependency-injection plumbing is
/// visible at the call site. See
/// [pnpm/pacquet#339](https://github.com/pnpm/pacquet/issues/339).
pub(in crate::cli_args) fn seed_config(
    npmrc_auth_file: Option<&Path>,
    ignore_workspace: bool,
    config_overrides: &ConfigOverrides,
    registry: Option<&str>,
) -> Config {
    Config {
        #[cfg(test)]
        config_dir: tests::current_test_global_config_dir(),
        npmrc_auth_file: npmrc_auth_file.map(Path::to_path_buf),
        ignore_workspace,
        cli: config_overrides.cli_layer(registry),
        ..Config::default()
    }
}

/// Layer the CLI flags and `--config.<key>` overrides of `invocation` onto
/// `config`, anchored at `dir`. Shared by the pre-command loads and the
/// command's full load so every stage sees the same CLI overrides at the
/// same `lockfileDir`.
pub(in crate::cli_args) fn apply_cli_layers(
    config: &mut Config,
    invocation: &Invocation,
    config_overrides: &ConfigOverrides,
    dir: &Path,
) -> miette::Result<()> {
    config_overrides.apply(config, dir);
    apply_color_override(config, invocation.color, invocation.no_color);
    if config.ci {
        pnpm_default_reporter::force_append_only();
    }
    invocation.network.apply(config);
    apply_location_overrides(
        config,
        dir,
        invocation.paths.store_dir.as_deref(),
        invocation.paths.state_dir.as_deref(),
    )?;
    if !invocation.paths.workspace_packages.is_empty() {
        config.cli_settings.insert("workspacePackagePatterns".to_string());
        config.workspace_package_patterns = Some(invocation.paths.workspace_packages.clone());
    }
    if !invocation.global
        && let Some(lockfile_dir) = invocation.lockfile_dir.as_deref()
    {
        config.pin_lockfile_dir(&dir.join(lockfile_dir));
        if !config.cli.npmrc_tokens
            .iter()
            .any(|(key, _)| matches!(key.as_str(), "lockfile-dir" | "lockfileDir"))
            && let Some(raw) = lockfile_dir.to_str()
        {
            config.cli.npmrc_tokens.push(("lockfile-dir".to_string(), raw.to_string()));
        }
    }
    config.offline = resolve_bool_override(
        invocation.offline == Some(true),
        invocation.offline == Some(false),
        config.offline,
    );
    config.prefer_offline = resolve_bool_override(
        invocation.prefer_offline == Some(true),
        invocation.prefer_offline == Some(false),
        config.prefer_offline,
    );
    Ok(())
}

/// `--color` wins over `--no-color`, and both over the configured value.
fn apply_color_override(cfg: &mut Config, color: Option<ColorMode>, no_color: bool) {
    if color.is_some() || no_color {
        cfg.cli_settings.insert("color".to_string());
    }
    cfg.color = match (color, no_color) {
        (Some(color), _) => color,
        (None, true) => ColorMode::Never,
        (None, false) => cfg.color,
    };
    configure_color(cfg.color);
}

/// The CLI overrides that redirect where pnpm reads and writes.
fn apply_location_overrides(
    cfg: &mut Config,
    anchor: &Path,
    store_dir: Option<&Path>,
    state_dir: Option<&Path>,
) -> miette::Result<()> {
    if let Some(store_dir) = store_dir {
        apply_store_dir_override::<Host>(cfg, store_dir, anchor)?;
        cfg.cli_settings.insert("storeDir".to_string());
        if !cfg.cli.npmrc_tokens
            .iter()
            .any(|(key, _)| matches!(key.as_str(), "store-dir" | "storeDir"))
            && let Some(raw) = store_dir.to_str()
        {
            cfg.cli.npmrc_tokens.push(("store-dir".to_string(), raw.to_string()));
        }
    }
    if let Some(state_dir) = state_dir {
        apply_state_dir_override::<Host>(cfg, state_dir, anchor);
        cfg.cli_settings.insert("stateDir".to_string());
        if !cfg.cli.npmrc_tokens
            .iter()
            .any(|(key, _)| matches!(key.as_str(), "state-dir" | "stateDir"))
            && let Some(raw) = state_dir.to_str()
        {
            cfg.cli.npmrc_tokens.push(("state-dir".to_string(), raw.to_string()));
        }
    }
    Ok(())
}

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
    invocation: &Invocation,
    config_overrides: &ConfigOverrides,
    dir: &Path,
    load: ConfigLoad,
) -> miette::Result<Config> {
    let mut config = load_raw_pre_command_config(invocation, config_overrides, dir, load)
        .map_err(|failure| miette::Report::new(failure.error))
        .wrap_err("load configuration")?;
    apply_cli_layers(&mut config, invocation, config_overrides, dir)?;
    Ok(config)
}

/// Load the configuration for [`ConfigLoad::RECORD`], preserving a
/// [`ConfigLoadFailure`] so the command's full load can report it without
/// reading the configuration a second time.
pub(super) fn try_load_record_config(
    invocation: &Invocation,
    config_overrides: &ConfigOverrides,
    dir: &Path,
) -> miette::Result<Result<Config, ConfigLoadFailure>> {
    match load_raw_pre_command_config(invocation, config_overrides, dir, ConfigLoad::RECORD) {
        Ok(mut config) => {
            apply_cli_layers(&mut config, invocation, config_overrides, dir)?;
            Ok(Ok(config))
        }
        Err(failure) => Ok(Err(failure)),
    }
}

fn load_raw_pre_command_config(
    invocation: &Invocation,
    config_overrides: &ConfigOverrides,
    dir: &Path,
    load: ConfigLoad,
) -> Result<Config, ConfigLoadFailure> {
    let mut config = seed_config(
        invocation.paths.npmrc_auth_file.as_deref(),
        invocation.paths.ignore_workspace,
        config_overrides,
        invocation.network.registry.as_deref(),
    );
    config.skip_store_dir_resolution = !load.resolve_store;
    config.settings_read = load.settings_read;
    config.current_keeping_warnings::<Host>(dir)
}
