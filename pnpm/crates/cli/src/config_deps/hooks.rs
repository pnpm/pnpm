use super::{
    Arc, BTreeMap, Config, HashMap, HookContext, HookLog, Host, IntoDiagnostic, LogEvent, LogFn,
    LogLevel, Path, PathBuf, PnpmLog, PnpmfileHooks, Reporter, Result, Value, WorkspaceSettings,
    apply_store_dir_override, default_state_dir, finder, get_catalogs_from_workspace_manifest,
    install_config_deps, is_known_setting_key, resolve_configured_state_dir,
};
use miette::WrapErr;

/// The pnpmfile paths that contribute hooks for `root_dir`, in
/// application order: config-dependency plugin pnpmfiles (lexical
/// order) first, then the workspace-root `.pnpmfile.{cjs,mjs}`. Shared
/// by the `updateConfig` install hook and the `beforePacking`
/// pack/publish hook so both apply the same pnpmfile set, matching
/// pnpm's single loaded hooks object.
pub fn resolve_pnpmfile_paths(
    config: &Config,
    root_dir: &Path,
) -> Result<Vec<PathBuf>, finder::MissingPnpmfileError> {
    if config.ignore_pnpmfile {
        return Ok(Vec::new());
    }
    finder::validate_configured_pnpmfiles(pnpm_package_manager::pnpmfile_selection(config))?;
    let config_modules_dir = root_dir.join("node_modules").join(".pnpm-config");
    let mut pnpmfiles: Vec<PathBuf> = match config.config_dependencies.as_ref() {
        Some(deps) => finder::calc_pnpmfile_paths_of_plugin_deps(
            &config_modules_dir,
            deps.keys().map(String::as_str),
        ),
        None => Vec::new(),
    };
    for pnpmfile in
        finder::find_pnpmfiles(root_dir, pnpm_package_manager::pnpmfile_selection(config))
    {
        if !pnpmfiles.contains(&pnpmfile) {
            pnpmfiles.push(pnpmfile);
        }
    }
    Ok(pnpmfiles)
}

/// Load the pnpmfiles that contribute a `beforePacking` hook for
/// `root_dir` (see [`resolve_pnpmfile_paths`]), returning one shareable
/// hook handle per pnpmfile. A recursive pack loads them once and clones
/// the `Arc`s into each project so a pnpmfile's Node worker is spawned
/// once, not once per packed project.
pub fn load_before_packing_hooks(
    config: &Config,
    root_dir: &Path,
) -> Result<Vec<Arc<dyn PnpmfileHooks>>, finder::MissingPnpmfileError> {
    Ok(resolve_pnpmfile_paths(config, root_dir)?
        .into_iter()
        .map(finder::load_pnpmfile_at)
        .collect())
}

/// Install the project's `configDependencies` and apply the `updateConfig`
/// pnpmfile hooks to `config`, returning the loaded hook objects. `dir` is
/// the command's working directory; the config root is derived from it the
/// same way the install family derives it.
///
/// The entry point for commands outside the install family, each of which
/// resolves its own [`Config`]: pnpm applies `updateConfig` once per
/// invocation, before the command runs, so a hook's settings — including
/// `extraEnv` and `extraBinPaths` — reach every command's child processes.
pub async fn prepare_config<Reporter: self::Reporter>(
    config: &mut Config,
    dir: &Path,
) -> Result<Vec<Arc<dyn PnpmfileHooks>>> {
    let config_root = config.root_project_manifest_dir(dir).to_path_buf();
    install_config_deps::<Reporter>(config, &config_root, config.frozen_lockfile.unwrap_or(false))
        .await?;
    run_update_config_hooks::<Reporter>(config, &config_root).await
}

/// Run the `updateConfig` pnpmfile hooks contributed by config-dependency
/// plugins and the project's own pnpmfile, applying their result to `config`.
/// The returned handles are the same loaded hook objects that ran
/// `updateConfig`, so packing can retain the original hook set when the hook
/// changes a hook-selection setting such as `ignorePnpmfile`.
///
/// A hook is handed the resolved configuration, by way of
/// [`WorkspaceSettings::from_resolved`] plus the non-settings views in
/// [`resolved_config_views`], so it reads what the install runs with rather
/// than one file's contribution to it.
///
/// Config round-trips through [`WorkspaceSettings`], so any settings key a
/// hook changes is applied back the same way `pnpm-workspace.yaml` is. Only
/// the keys a hook actually changed are applied, so values resolved from
/// `.npmrc` or CLI flags that the hooks leave untouched are not clobbered.
/// The `catalog:`/`catalogs:` blocks — which pacquet models outside
/// `WorkspaceSettings` — are seeded into the hook input and, when changed,
/// captured into [`Config::catalogs`] for install and packing commands.
pub async fn run_update_config_hooks<Reporter: self::Reporter>(
    config: &mut Config,
    root_dir: &Path,
) -> Result<Vec<Arc<dyn PnpmfileHooks>>> {
    let pnpmfiles = resolve_pnpmfile_paths(config, root_dir)
        .map_err(|error| miette::miette!(code = "ERR_PNPM_PNPMFILE_NOT_FOUND", "{error}"))?;
    let hooks: Vec<Arc<dyn PnpmfileHooks>> =
        pnpmfiles.iter().cloned().map(finder::load_pnpmfile_at).collect();
    if pnpmfiles.is_empty() {
        return Ok(hooks);
    }

    let base_dir = hook_base_dir(root_dir)?;
    let mut input = serde_json::to_value(WorkspaceSettings::from_resolved(config))
        .into_diagnostic()
        .wrap_err("serialize the resolved settings for updateConfig hooks")?;
    seed_hook_input(&mut input, config, root_dir, &pnpmfiles, manifest_catalogs_value(root_dir)?)?;

    let prefix = root_dir.to_string_lossy().into_owned();
    let mut current = input.clone();
    let mut has_filter_log = false;
    for (pnpmfile, hook) in pnpmfiles.iter().zip(&hooks) {
        let ctx = HookContext { log: hook_logger::<Reporter>(pnpmfile, &prefix), dir: None };
        current = hook
            .update_config(current, ctx)
            .await
            .map_err(|err| miette::miette!("{err}"))
            .wrap_err_with(|| format!("running updateConfig hook from {}", pnpmfile.display()))?;
        has_filter_log |= hook.has_filter_log().await;
    }
    if has_filter_log {
        Reporter::emit(&LogEvent::Pnpm(PnpmLog {
            level: LogLevel::Warn,
            message: "The pnpmfile filterLog hook is deprecated and is not supported by pnpm v12. Remove it from the pnpmfile."
                .to_string(),
            prefix: prefix.clone(),
        }));
    }

    adopt_hook_catalogs(config, &current)?;

    apply_hook_delta(config, &input, &current, &base_dir)?;
    Ok(hooks)
}

/// The directory relative hook-output paths resolve against: the workspace
/// manifest's, else the root.
fn hook_base_dir(root_dir: &Path) -> Result<PathBuf> {
    Ok(match WorkspaceSettings::find_and_load(root_dir).into_diagnostic()? {
        Some((path, _)) => path.parent().map_or_else(|| root_dir.to_path_buf(), Path::to_path_buf),
        None => root_dir.to_path_buf(),
    })
}

/// The catalogs read from the workspace manifest (`catalog:` +
/// `catalogs:`), which `WorkspaceSettings` doesn't carry, seeded into the
/// hook input so a hook can read and extend them.
fn manifest_catalogs_value(root_dir: &Path) -> Result<Value> {
    let workspace_manifest = pnpm_workspace::read_workspace_manifest(root_dir).into_diagnostic()?;
    let yaml_catalogs = get_catalogs_from_workspace_manifest(workspace_manifest.as_ref())
        .into_diagnostic()
        .wrap_err("reading catalogs for updateConfig hooks")?;
    serde_json::to_value(&yaml_catalogs).into_diagnostic()
}

/// Seed the hook input with the values pnpm resolves outside
/// [`WorkspaceSettings`], so a hook can read and extend them. They are
/// read back out of the delta rather than through `apply_to`.
fn seed_hook_input(
    input: &mut Value,
    config: &Config,
    root_dir: &Path,
    pnpmfiles: &[PathBuf],
    catalogs: Value,
) -> Result<()> {
    let Some(object) = input.as_object_mut() else {
        return Ok(());
    };
    object.insert("catalogs".to_string(), catalogs);
    // PnpmBuild's `updateConfig` appends its bin dir to `extraBinPaths`
    // and sets `npm_config_nodedir` in `extraEnv`, so both have to arrive
    // carrying what pnpm already resolved.
    object.insert(
        "extraBinPaths".to_string(),
        serde_json::to_value(&config.extra_bin_paths).into_diagnostic()?,
    );
    object
        .insert("extraEnv".to_string(), serde_json::to_value(&config.extra_env).into_diagnostic()?);
    object.append(&mut resolved_config_views(config, root_dir).into_diagnostic()?);
    // The pnpmfiles being run, which is what the setting resolves to and
    // what pnpm reports, rather than only a pinned `pnpmfile` value.
    object.insert("pnpmfile".to_string(), serde_json::to_value(pnpmfiles).into_diagnostic()?);
    // A setting nothing set is absent, as it is on pnpm 11, so that
    // `'key' in config` answers there and a hook doesn't read a null as a
    // configured value.
    object.retain(|_, value| !value.is_null());
    Ok(())
}

/// Apply what the `updateConfig` hooks changed between `input` and their
/// `output` back onto `config`.
fn apply_hook_delta(
    config: &mut Config,
    input: &Value,
    current: &Value,
    base_dir: &Path,
) -> Result<()> {
    let explicit_registry = config.explicit_settings.get("registry").cloned();
    let delta = config_delta(input, current);
    // `config_delta` only walks keys present in the hook output, so a
    // `scriptShell` the hook deleted (pnpm: `undefined`, no shell) leaves no
    // trace in the delta.
    let script_shell_deleted =
        input.get("scriptShell").is_some() && current.get("scriptShell").is_none();
    if delta.as_object().is_none_or(serde_json::Map::is_empty) && !script_shell_deleted {
        return Ok(());
    }
    let changed_store_dir = delta.get("storeDir").and_then(Value::as_str).map(str::to_owned);
    // `extraBinPaths` / `extraEnv` aren't `WorkspaceSettings` fields, so
    // `from_value(delta)` below ignores them. Pull the hook's values out
    // first and assign them directly.
    let HookExecutionChanges { changed_extra_bin_paths, changed_extra_env } =
        hook_execution_changes(&delta)?;
    apply_registry_routing_changes(config, &delta)?;

    let delta_settings: WorkspaceSettings = serde_json::from_value(delta.clone())
        .into_diagnostic()
        .wrap_err("deserialize the updateConfig hook result")?;
    record_explicit_setting_changes(config, &delta, &delta_settings);
    delta_settings.apply_to(config, base_dir);
    if script_shell_deleted {
        config.script_shell = None;
    }
    if let Some(extra_bin_paths) = changed_extra_bin_paths {
        config.extra_bin_paths = extra_bin_paths;
    }
    if let Some(extra_env) = changed_extra_env {
        config.extra_env = extra_env;
    }
    restore_defaults_of_nulled_settings(config, &delta, base_dir);
    apply_state_dir_change(config, &delta);
    if delta.get("shamefullyHoist").is_some() {
        config.apply_shamefully_hoist_derivation();
    }
    apply_hook_store_dir(config, changed_store_dir.as_deref(), base_dir)?;
    if let Some(Value::String(explicit_reg)) = explicit_registry {
        config.registry = explicit_reg;
    }
    Ok(())
}

/// Apply the routing a hook rewrote under the `registriesByScope` /
/// `registriesByPrefix` names it reads it under. `WorkspaceSettings`
/// reaches the same lookups through its file-shaped `registries` key, which
/// `apply_to` still honors, so this runs first and an entry the hook wrote
/// through `registries` wins.
fn apply_registry_routing_changes(config: &mut Config, delta: &Value) -> Result<()> {
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
    delta_settings: &WorkspaceSettings,
) {
    config.record_explicit_settings(delta_settings);
    let Some(delta) = delta.as_object() else { return };
    for key in delta.iter().filter(|(_, value)| value.is_null()).map(|(key, _)| key) {
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
    for key in delta.iter().filter(|(_, value)| value.is_null()).map(|(key, _)| key) {
        let defaults = defaults.get_or_insert_with(Config::default);
        WorkspaceSettings::reset_setting_to_default::<Host>(config, defaults, key, base_dir);
    }
}

/// `stateDir` resolves against the host's state root rather than the
/// workspace, the way [`Config::current`] resolves it, so `apply_to` leaves
/// it to this.
fn apply_state_dir_change(config: &mut Config, delta: &Value) {
    let Some(dir) = delta.get("stateDir").and_then(Value::as_str).filter(|dir| !dir.is_empty())
    else {
        return;
    };
    let default_state_dir = default_state_dir::<Host>().unwrap_or_default();
    config.state_dir = resolve_configured_state_dir(&default_state_dir, dir);
}

/// The resolved state an `updateConfig` hook reads that is not a settings
/// key, under the names pnpm 11 exposes it as.
///
/// These are derived from the settings rather than written by a user, so
/// [`WorkspaceSettings`] has no field for them and the write-back ignores
/// them, except registry routing, which [`apply_registry_routing_changes`]
/// applies.
///
/// `configByUri` carries each registry's credentials by scope; the
/// per-registry certificate settings pnpm 11 also indexes there are absent.
fn resolved_config_views(
    config: &Config,
    root_dir: &Path,
) -> serde_json::Result<serde_json::Map<String, Value>> {
    let mut views = serde_json::Map::new();
    let mut set = |key: &str, value: Value| {
        views.insert(key.to_string(), value);
    };

    // The scope map reports the built-in `@jsr` route it resolves through and
    // the default registry every unscoped package is fetched from, which the
    // config carries as the `registry` setting. The prefix map reports only
    // the prefixes the project declares, and nothing when it declares none,
    // as pnpm 11 does.
    let mut registries_by_scope = config.resolved_registry_lookups().registries_by_scope;
    registries_by_scope.insert("default".to_string(), config.registry.clone());
    set("registriesByScope", serde_json::to_value(&registries_by_scope)?);
    if !config.registries_by_prefix.is_empty() {
        set("registriesByPrefix", serde_json::to_value(&config.registries_by_prefix)?);
    }

    // The raw auth keys, plus the registry rows resolved across every
    // source, so `authConfig.registry` and `authConfig['@scope:registry']`
    // answer the URL the install fetches from rather than whichever
    // `.npmrc` line happened to name one. `pnpm config list` merges them
    // the same way.
    let mut auth_config: serde_json::Map<String, Value> =
        config.raw_auth_config.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect();
    for (scope, url) in &registries_by_scope {
        let key =
            if scope == "default" { "registry".to_string() } else { format!("{scope}:registry") };
        auth_config.insert(key, Value::String(url.clone()));
    }
    set("authConfig", Value::Object(auth_config));
    set("configByUri", serde_json::to_value(&config.registry_creds_by_uri)?);

    set("dir", Value::String(root_dir.to_string_lossy().into_owned()));
    set("workspaceDir", serde_json::to_value(&config.workspace_dir)?);
    set("configDir", serde_json::to_value(&config.config_dir)?);
    set("globalPkgDir", serde_json::to_value(&config.global_pkg_dir)?);
    set("failIfNoMatch", Value::Bool(config.fail_if_no_match));
    set("useGitBranchLockfile", Value::Bool(config.use_git_branch_lockfile));
    set("workspacePackagePatterns", serde_json::to_value(&config.workspace_package_patterns)?);
    set("sideEffectsCacheRead", Value::Bool(config.side_effects_cache_read()));
    set("sideEffectsCacheWrite", Value::Bool(config.side_effects_cache_write()));
    // The settings key of the same name reports only a pinned value; a hook
    // reading it wants the directory the lockfile is actually written to.
    set(
        "lockfileDir",
        Value::String(config.lockfile_dir_for(root_dir).to_string_lossy().into_owned()),
    );

    Ok(views)
}

/// The keys whose value the hooks changed between the serialized input
/// config and the hooks' output. Applying only these avoids clobbering
/// config resolved elsewhere (`.npmrc`, CLI flags) that a hook left
/// untouched.
fn config_delta(input: &Value, output: &Value) -> Value {
    let (Some(input_obj), Some(output_obj)) = (input.as_object(), output.as_object()) else {
        return output.clone();
    };
    let mut delta = serde_json::Map::new();
    for (key, value) in output_obj {
        if input_obj.get(key) != Some(value) {
            delta.insert(key.clone(), value.clone());
        }
    }
    Value::Object(delta)
}

/// A `context.log(...)` sink that forwards each hook log line to the
/// `pnpm:hook` channel, tagged with the pnpmfile it came from.
fn hook_logger<Reporter: self::Reporter>(pnpmfile: &Path, prefix: &str) -> LogFn {
    let from = pnpmfile.to_string_lossy().into_owned();
    let prefix = prefix.to_owned();
    Arc::new(move |message| {
        Reporter::emit(&LogEvent::Hook(HookLog {
            level: LogLevel::Debug,
            from: from.clone(),
            hook: "updateConfig".to_string(),
            prefix: prefix.clone(),
            message,
        }));
    })
}

/// Hook output replaces the seeded catalogs, including removed entries.
fn adopt_hook_catalogs(config: &mut Config, current: &Value) -> Result<()> {
    config.catalogs = Some(
        current
            .get("catalogs")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .into_diagnostic()
            .wrap_err("the updateConfig hook produced an invalid catalogs value")?
            .unwrap_or_default(),
    );

    Ok(())
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
