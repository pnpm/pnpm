use super::{
    BTreeMap, Config, HashMap, Host, IntoDiagnostic, Path, PathBuf, Result, TrustedSettings, Value,
    WorkspaceSettings, apply_store_dir_override, config_delta, default_state_dir,
    is_known_setting_key, resolve_configured_state_dir,
};
use miette::WrapErr;

struct HookExecutionChanges {
    changed_extra_bin_paths: Option<Vec<PathBuf>>,
    changed_extra_env: Option<HashMap<String, String>>,
}

struct LockfileDirPathsBeforeHook {
    lockfile_dir: Option<PathBuf>,
    configured_virtual_store_dir: Option<PathBuf>,
    node_path_dirs: [PathBuf; 2],
    hook_node_path: Option<Option<String>>,
    hook_node_options: Option<Option<String>>,
    hook_modules_dir: Option<PathBuf>,
    modules_dir_changed: bool,
    hook_virtual_store_dir: HookVirtualStoreDir,
}

#[derive(Clone, Copy)]
enum HookVirtualStoreDir {
    Unchanged,
    Cleared,
    Set,
}

struct NodePathRefresh {
    previous_dirs: [PathBuf; 2],
    hook_node_path: Option<Option<String>>,
    hook_node_options: Option<Option<String>>,
}

impl NodePathRefresh {
    fn apply_to(self, config: &mut Config) {
        config.refresh_node_path_after_lockfile_dir_change::<Host>(
            [&self.previous_dirs[0], &self.previous_dirs[1]],
            self.hook_node_path,
            self.hook_node_options,
        );
    }
}

/// `extraBinPaths` / `extraEnv` aren't `WorkspaceSettings` fields, so
/// `from_value(delta)` ignores them. The hook's values are pulled out
/// first and assigned directly.
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
        for key in super::HIDDEN_FROM_HOOKS {
            delta.remove(key);
        }
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

/// Apply what the `updateConfig` hooks changed between `input` and their
/// `output` back onto `config`.
pub(super) fn apply_hook_delta(
    config: &mut Config,
    input: &Value,
    current: &Value,
    base_dir: &Path,
) -> Result<()> {
    let delta = filter_hook_delta(config, config_delta(input, current));
    let lockfile_dir_paths_before_hook =
        lockfile_dir_paths_before_hook(config, input, current, &delta, base_dir);
    // `config_delta` only walks keys present in the hook output, so a
    // `scriptShell` the hook deleted (pnpm: `undefined`, no shell) leaves no
    // trace in the delta.
    let script_shell_deleted =
        input.get("scriptShell").is_some() && current.get("scriptShell").is_none();
    if delta.as_object().is_none_or(serde_json::Map::is_empty) && !script_shell_deleted {
        return Ok(());
    }
    let execution_changes = hook_execution_changes(&delta)?;
    apply_registry_routing_changes(config, &delta)?;

    let delta_settings = TrustedSettings::from_json(&delta)
        .into_diagnostic()
        .wrap_err("deserialize the updateConfig hook result")?;
    record_explicit_setting_changes(config, &delta, &delta_settings);
    delta_settings.apply_to(config, base_dir);
    config.apply_cli_registries();
    apply_hook_execution_changes(config, execution_changes, script_shell_deleted);
    restore_defaults_of_nulled_settings(config, &delta, base_dir);
    let node_path_refresh = lockfile_dir_paths_before_hook.map(|paths_before_hook| {
        apply_lockfile_dir_change(config, paths_before_hook)
    });
    apply_state_dir_change(config, &delta);
    apply_project_selector_changes(config, &delta, base_dir)?;
    if delta.get("shamefullyHoist").is_some() || config.cli_settings.contains("shamefullyHoist") {
        config.apply_shamefully_hoist_derivation();
    }
    apply_hook_store_dir_change(config, &delta, base_dir)?;
    if let Some(node_path_refresh) = node_path_refresh {
        node_path_refresh.apply_to(config);
    }
    Ok(())
}

/// Re-anchor the paths that follow the lockfile directory. `NODE_PATH` is
/// refreshed by the returned value once the store paths are settled.
fn apply_lockfile_dir_change(
    config: &mut Config,
    paths_before_hook: LockfileDirPathsBeforeHook,
) -> NodePathRefresh {
    let project_dir = config.dir.clone();
    let modules_dir = config.modules_dir.clone();
    let install_state_dir = config.install_state_dir.clone();
    if let Some(lockfile_dir) =
        config.configured_lockfile_dir.clone().or_else(|| config.lockfile_dir.clone())
    {
        if paths_before_hook.lockfile_dir.is_none() {
            config.lockfile_dir = None;
        }
        config.pin_lockfile_dir(&lockfile_dir, &project_dir);
    }
    restore_default_lockfile_dir_paths(config, &paths_before_hook, modules_dir, install_state_dir);
    NodePathRefresh {
        previous_dirs: paths_before_hook.node_path_dirs,
        hook_node_path: paths_before_hook.hook_node_path,
        hook_node_options: paths_before_hook.hook_node_options,
    }
}

fn restore_default_lockfile_dir_paths(
    config: &mut Config,
    paths_before_hook: &LockfileDirPathsBeforeHook,
    modules_dir: PathBuf,
    install_state_dir: PathBuf,
) {
    if config.lockfile_dir.is_some() {
        return;
    }
    let configured_virtual_store_dir = paths_before_hook
        .configured_virtual_store_dir
        .as_ref();
    config.modules_dir = modules_dir;
    let hook_virtual_store_dir = match paths_before_hook.hook_virtual_store_dir {
        HookVirtualStoreDir::Unchanged => {
            config.configured_virtual_store_dir = configured_virtual_store_dir.cloned();
            None
        }
        HookVirtualStoreDir::Cleared => {
            config.configured_virtual_store_dir = None;
            None
        }
        HookVirtualStoreDir::Set => config.configured_virtual_store_dir.clone(),
    };
    config.install_state_dir = install_state_dir;
    if paths_before_hook.lockfile_dir.is_some() {
        restore_default_lockfile_dir(config, configured_virtual_store_dir, hook_virtual_store_dir);
    }
    if let Some(hook_modules_dir) = paths_before_hook.hook_modules_dir.as_ref() {
        config.modules_dir.clone_from(hook_modules_dir);
    }
    if paths_before_hook.modules_dir_changed
        && (!config.explicit_settings.contains_key("virtualStoreDir")
            || config.enable_global_virtual_store)
    {
        config.install_state_dir = config.modules_dir.join(".pnpm");
    }
}

fn restore_default_lockfile_dir(
    config: &mut Config,
    configured_virtual_store_dir: Option<&PathBuf>,
    hook_virtual_store_dir: Option<PathBuf>,
) {
    let project_dir = config.dir.clone();
    let lockfile_dir = config.lockfile_dir_for(&project_dir).to_path_buf();
    let paths_dir = config.workspace_dir.clone().unwrap_or(lockfile_dir);
    let project_relative_virtual_store_dir = config.explicit_settings
        .get("virtualStoreDir")
        .and_then(Value::as_str)
        .and_then(|raw| {
            configured_virtual_store_dir.filter(|dir| {
                !Path::new(raw).is_absolute()
                    && pnpm_fs::lexical_normalize(dir)
                        == pnpm_fs::lexical_normalize(&project_dir.join(raw))
            })
        });
    config.anchor_lockfile_paths(&paths_dir);
    if !config.enable_global_virtual_store
        && let Some(dir) = project_relative_virtual_store_dir
    {
        config.set_virtual_store_dir(dir.clone());
    }
    if !config.enable_global_virtual_store
        && let Some(dir) = hook_virtual_store_dir
    {
        config.set_virtual_store_dir(dir);
    }
}

fn changed_extra_env_value(input: &Value, current: &Value, key: &str) -> Option<Option<String>> {
    let before = input
        .get("extraEnv")
        .and_then(|env| env.get(key))
        .and_then(Value::as_str);
    let after = current
        .get("extraEnv")
        .and_then(|env| env.get(key))
        .and_then(Value::as_str);
    (before != after).then(|| after.map(str::to_owned))
}

fn lockfile_dir_paths_before_hook(
    config: &Config,
    input: &Value,
    current: &Value,
    delta: &Value,
    base_dir: &Path,
) -> Option<LockfileDirPathsBeforeHook> {
    delta
        .get("lockfileDir")
        .map(|_| LockfileDirPathsBeforeHook {
            lockfile_dir: config.lockfile_dir.clone(),
            configured_virtual_store_dir: config.configured_virtual_store_dir.clone(),
            node_path_dirs: [
                config.install_state_dir.join("node_modules"),
                config.modules_dir.clone(),
            ],
            hook_node_path: changed_extra_env_value(input, current, "NODE_PATH"),
            hook_node_options: changed_extra_env_value(input, current, "NODE_OPTIONS"),
            hook_modules_dir: delta
                .get("modulesDir")
                .and_then(Value::as_str)
                .map(|modules_dir| pnpm_fs::lexical_normalize(&base_dir.join(modules_dir))),
            modules_dir_changed: delta.get("modulesDir").is_some(),
            hook_virtual_store_dir: match delta.get("virtualStoreDir") {
                None => HookVirtualStoreDir::Unchanged,
                Some(Value::Null) => HookVirtualStoreDir::Cleared,
                Some(_) => HookVirtualStoreDir::Set,
            },
        })
}

fn apply_hook_store_dir_change(config: &mut Config, delta: &Value, base_dir: &Path) -> Result<()> {
    let changed_store_dir = delta
        .get("storeDir")
        .and_then(Value::as_str)
        .map(str::to_owned);
    apply_hook_store_dir(config, changed_store_dir.as_deref(), base_dir)
}

/// Apply the routing a hook rewrote under the `registriesByScope` /
/// `registriesByPrefix` names it reads it under. `WorkspaceSettings`
/// reaches the same lookups through its file-shaped `registries` key, which
/// `apply_to` still honors, so this runs first and an entry the hook wrote
/// through `registries` wins.
pub(super) fn apply_registry_routing_changes(config: &mut Config, delta: &Value) -> Result<()> {
    if let Some(mut routes) = hook_registry_routes(delta, "registriesByScope")? {
        // The hook reads the default registry as the `default` entry of the
        // map, but the config carries it as the standalone `registry`
        // setting beside a map of `@scope` routes only.
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

/// The routing map the hook output holds under `key`, if it changed one.
fn hook_registry_routes(delta: &Value, key: &str) -> Result<Option<BTreeMap<String, String>>> {
    delta
        .get(key)
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .into_diagnostic()
        .wrap_err_with(|| format!("the updateConfig hook produced an invalid {key} value"))
}

/// Record what the hook set in [`Config::explicit_settings`], as loading a
/// settings file does, and drop the settings it set to null, so the
/// derivations that read whether a setting was set at all see the hook's
/// answer.
fn record_explicit_setting_changes(
    config: &mut Config,
    delta: &Value,
    delta_settings: &TrustedSettings,
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

/// A setting the hook set to null is unset, as it is on pnpm 11, and
/// resolves to the default pnpm would have chosen. `apply_to` has no value
/// to apply for it, so each is restored through
/// [`WorkspaceSettings::reset_setting_to_default`].
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

/// Apply the `filter` / `filterProd` selectors a hook set. Either one
/// replaces the `{.}...` default, which pnpm falls back to only when
/// nothing selected projects.
///
/// Relative directory selectors a hook sets are rebased from `base_dir` to
/// [`Config::dir`] so they select the same projects when `--dir` or the
/// working directory is not the workspace root.
fn apply_project_selector_changes(
    config: &mut Config,
    delta: &Value,
    base_dir: &Path,
) -> Result<()> {
    let filter = hook_selectors(delta, "filter")?;
    let filter_prod = hook_selectors(delta, "filterProd")?;
    if filter.is_none() && filter_prod.is_none() {
        return Ok(());
    }
    let prev_filter = config.filter.clone();
    let prev_filter_prod = config.filter_prod.clone();
    let prefix = if config.dir.as_os_str().is_empty() { base_dir } else { &config.dir };
    if let Some(filter) = filter {
        config.filter = filter
            .into_iter()
            .map(|selector| {
                pnpm_workspace_projects_filter::rebase_project_selector(&selector, base_dir, prefix)
            })
            .collect();
    } else if !config.cli_settings.contains("filter") {
        config.filter.clear();
    }
    if let Some(filter_prod) = filter_prod {
        config.filter_prod = filter_prod
            .into_iter()
            .map(|selector| {
                pnpm_workspace_projects_filter::rebase_project_selector(&selector, base_dir, prefix)
            })
            .collect();
    }
    if config.filter != prev_filter || config.filter_prod != prev_filter_prod {
        config.filter_order.clear();
    }
    Ok(())
}

/// The selectors the hook output holds under `key`, if it set any.
fn hook_selectors(delta: &Value, key: &str) -> Result<Option<Vec<String>>> {
    delta
        .get(key)
        .filter(|value| !value.is_null())
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .into_diagnostic()
        .wrap_err_with(|| format!("the updateConfig hook produced an invalid {key} value"))
}

/// `stateDir` resolves against the host's state root rather than the
/// workspace, the way [`Config::current`] resolves it, so `apply_to` leaves
/// it to this.
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
