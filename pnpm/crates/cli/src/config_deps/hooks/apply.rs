use super::{
    BTreeMap, Config, HashMap, Host, IntoDiagnostic, Path, PathBuf, Result, Value,
    WorkspaceSettings, apply_store_dir_override, config_delta, default_state_dir,
    is_known_setting_key, resolve_configured_state_dir,
};
use miette::WrapErr;

struct HookExecutionChanges {
    changed_extra_bin_paths: Option<Vec<PathBuf>>,
    changed_extra_env: Option<HashMap<String, String>>,
}

fn hook_execution_changes(delta: &Value) -> Result<HookExecutionChanges> {
    let changed_extra_bin_paths = delta
        .get("extraBinPaths")
        .map(|value| serde_json::from_value::<Vec<PathBuf>>(value.clone()))
        .transpose()
        .into_diagnostic()
        .wrap_err("the updateConfig hook produced an invalid extraBinPaths value")?;
    let changed_extra_env = delta
        .get("extraEnv")
        .map(|value| serde_json::from_value::<HashMap<String, String>>(value.clone()))
        .transpose()
        .into_diagnostic()
        .wrap_err("the updateConfig hook produced an invalid extraEnv value")?;
    Ok(HookExecutionChanges { changed_extra_bin_paths, changed_extra_env })
}

fn filter_hook_delta(config: &Config, mut delta: Value) -> Value {
    if let Some(delta) = delta.as_object_mut() {
        delta.remove("macosBackup");
        delta.retain(|key, _| {
            !config.cli_settings.contains(key)
                && !config.cli_settings.contains(&pnpm_config::naming_cases::to_camel_case(key))
        });
    }
    delta
}

fn apply_hook_execution_changes(
    config: &mut Config,
    changes: HookExecutionChanges,
    script_shell_deleted: bool,
) {
    if script_shell_deleted {
        config.script_shell = None;
    }
    if let Some(extra_bin_paths) = changes.changed_extra_bin_paths {
        config.extra_bin_paths = extra_bin_paths;
    }
    if let Some(extra_env) = changes.changed_extra_env {
        config.extra_env = extra_env;
    }
}

pub(super) fn apply_hook_delta(
    config: &mut Config,
    input: &Value,
    current: &Value,
    base_dir: &Path,
) -> Result<()> {
    let delta = filter_hook_delta(config, config_delta(input, current));
    let script_shell_deleted =
        input.get("scriptShell").is_some() && current.get("scriptShell").is_none();
    if delta.as_object().is_none_or(serde_json::Map::is_empty) && !script_shell_deleted {
        return Ok(());
    }
    let changed_store_dir = delta
        .get("storeDir")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let execution_changes = hook_execution_changes(&delta)?;
    let cli_registries = cli_registries(config);
    apply_registry_routing_changes(config, &delta)?;

    let delta_settings: WorkspaceSettings = serde_json::from_value(delta.clone())
        .into_diagnostic()
        .wrap_err("deserialize the updateConfig hook result")?;
    record_explicit_setting_changes(config, &delta, &delta_settings);
    delta_settings.apply_to(config, base_dir);
    restore_cli_registries(config, cli_registries);
    apply_hook_execution_changes(config, execution_changes, script_shell_deleted);
    restore_defaults_of_nulled_settings(config, &delta, base_dir);
    apply_state_dir_change(config, &delta);
    if delta.get("shamefullyHoist").is_some() {
        config.apply_shamefully_hoist_derivation();
    }
    apply_hook_store_dir(config, changed_store_dir.as_deref(), base_dir)?;
    Ok(())
}

pub(super) fn apply_registry_routing_changes(config: &mut Config, delta: &Value) -> Result<()> {
    if let Some(mut routes) = hook_registry_routes(delta, "registriesByScope")? {
        if let Some(default) = routes.remove("default") {
            config.registry = default;
        }
        config.registries_by_scope = routes;
    }
    if let Some(routes) = hook_registry_routes(delta, "registriesByPrefix")? {
        config.registries_by_prefix = routes;
    }
    Ok(())
}

fn hook_registry_routes(delta: &Value, key: &str) -> Result<Option<BTreeMap<String, String>>> {
    delta
        .get(key)
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .into_diagnostic()
        .wrap_err_with(|| format!("the updateConfig hook produced an invalid {key} value"))
}

fn record_explicit_setting_changes(
    config: &mut Config,
    delta: &Value,
    delta_settings: &WorkspaceSettings,
) {
    config.record_explicit_settings(delta_settings);
    let Some(delta) = delta.as_object() else { return };
    for key in delta
        .iter()
        .filter(|(_, value)| value.is_null())
        .map(|(key, _)| key)
    {
        if is_known_setting_key(key) {
            config.explicit_settings.remove(key);
        }
    }
}

fn restore_defaults_of_nulled_settings(config: &mut Config, delta: &Value, base_dir: &Path) {
    let Some(delta) = delta.as_object() else { return };
    let mut defaults = None;
    for key in delta
        .iter()
        .filter(|(_, value)| value.is_null())
        .map(|(key, _)| key)
    {
        let defaults = defaults.get_or_insert_with(Config::default);
        WorkspaceSettings::reset_setting_to_default::<Host>(config, defaults, key, base_dir);
    }
}

fn apply_state_dir_change(config: &mut Config, delta: &Value) {
    let Some(dir) = delta
        .get("stateDir")
        .and_then(Value::as_str)
        .filter(|dir| !dir.is_empty())
    else {
        return;
    };
    let default_state_dir = default_state_dir::<Host>().unwrap_or_default();
    config.state_dir = resolve_configured_state_dir(&default_state_dir, dir);
}

fn apply_hook_store_dir(
    config: &mut Config,
    changed_store_dir: Option<&str>,
    base_dir: &Path,
) -> Result<()> {
    if let Some(store_dir) = changed_store_dir {
        apply_store_dir_override::<Host>(config, Path::new(store_dir), base_dir)?;
    } else {
        let virtual_store_dir_explicit = config.explicit_settings.contains_key("virtualStoreDir");
        let global_virtual_store_dir_explicit =
            config.explicit_settings.contains_key("globalVirtualStoreDir");
        config.apply_global_virtual_store_derivation(
            virtual_store_dir_explicit,
            global_virtual_store_dir_explicit,
        );
    }
    Ok(())
}

fn cli_registries(config: &Config) -> Vec<(String, String)> {
    config.cli_settings
        .iter()
        .filter_map(|key| {
            if key == "registry" {
                return Some(("default".to_string(), config.registry.clone()));
            }
            let scope = key
                .strip_suffix(":registry")
                .filter(|scope| scope.starts_with('@'))?;
            let url = config.registries_by_scope.get(scope)?;
            Some((scope.to_string(), url.clone()))
        })
        .collect()
}

fn restore_cli_registries(config: &mut Config, registries: Vec<(String, String)>) {
    for (scope, url) in registries {
        if scope == "default" {
            crate::config_overrides::apply_registry_override(config, &url);
        } else {
            config.registries_by_scope.insert(scope.clone(), url.clone());
            config.package_manager_bootstrap.registries.insert(scope, url);
        }
    }
}
