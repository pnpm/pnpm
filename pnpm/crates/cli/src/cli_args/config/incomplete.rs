//! Warnings for a configuration that lacks the settings of a config
//! dependency that is not installed.

use crate::cli_args::config_warnings::emit_config_warning;
use pnpm_config::{Config, naming_cases::to_camel_case};
use pnpm_workspace::{IncludeSource, SettingOwner, SettingSlot, canonical_setting_name};

/// Warn when what `pnpm config get <key>` (or `pnpm config list`, when `key`
/// is `None`) prints may lack a setting of a config dependency that is not
/// installed.
pub(super) fn warn_if_incomplete(config: &Config, key: Option<&str>) {
    let Some(includes) = config.workspace_includes.as_deref() else {
        return;
    };
    let unavailable: Vec<&str> = includes.unavailable_packages().collect();
    if unavailable.is_empty() {
        return;
    }
    let Some(key) = key else {
        emit_config_warning(&format!(
            r#"The configuration is incomplete. These config dependencies are not installed, so the settings they own are not shown: {}. Run "pnpm install" to load them."#,
            unavailable.join(", "),
        ));
        return;
    };
    let setting = to_camel_case(
        key.split(['.', '['])
            .next()
            .unwrap_or(key),
    );
    let slot = SettingSlot::Setting(canonical_setting_name(&setting).to_string());
    if let SettingOwner::Include(IncludeSource::Package(name)) = includes.owner(&slot)
        && unavailable.contains(&name.as_str())
    {
        emit_config_warning(&format!(
            r#""{setting}" is delegated to the config dependency {name}, which is not installed, so its value is not shown. Run "pnpm install" to load it."#,
        ));
    }
}
