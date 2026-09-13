use super::{Config, Path, Value};

/// The resolved state an `updateConfig` hook reads that is not a settings
/// key, under the names pnpm 11 exposes it as.
///
/// These are derived from the settings rather than written by a user, so
/// [`super::WorkspaceSettings`] has no field for them and the write-back ignores
/// them, except registry routing, which [`super::apply_registry_routing_changes`]
/// applies.
///
/// `configByUri` carries each registry's credentials by scope; the
/// per-registry certificate settings pnpm 11 also indexes there are absent.
pub(super) fn resolved_config_views(
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
    set(
        "registriesByScope",
        serde_json::to_value(&registries_by_scope)?,
    );
    if !config.registries_by_prefix.is_empty() {
        set(
            "registriesByPrefix",
            serde_json::to_value(&config.registries_by_prefix)?,
        );
    }

    // The raw auth keys, plus the registry rows resolved across every
    // source, so `authConfig.registry` and `authConfig['@scope:registry']`
    // answer the URL the install fetches from rather than whichever
    // `.npmrc` line happened to name one. `pnpm config list` merges them
    // the same way.
    let mut auth_config: serde_json::Map<String, Value> = config.raw_auth_config
        .iter()
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect();
    for (scope, url) in &registries_by_scope {
        let key = if scope == "default" {
            "registry".to_string()
        } else {
            format!("{scope}:registry")
        };
        auth_config.insert(key, Value::String(url.clone()));
    }
    set("authConfig", Value::Object(auth_config));
    set(
        "configByUri",
        serde_json::to_value(&config.registry_creds_by_uri)?,
    );

    views.extend(resolved_directory_views(config, root_dir)?);

    Ok(views)
}

pub(super) fn resolved_directory_views(
    config: &Config,
    root_dir: &Path,
) -> serde_json::Result<serde_json::Map<String, Value>> {
    let mut views = serde_json::Map::new();
    let mut set = |key: &str, value: Value| {
        views.insert(key.to_owned(), value);
    };
    set("dir", path_value(root_dir));
    set("workspaceDir", serde_json::to_value(&config.workspace_dir)?);
    set("configDir", serde_json::to_value(&config.config_dir)?);
    set(
        "globalPkgDir",
        serde_json::to_value(&config.global_pkg_dir)?,
    );
    set("failIfNoMatch", Value::Bool(config.fail_if_no_match));
    set(
        "useGitBranchLockfile",
        Value::Bool(config.use_git_branch_lockfile),
    );
    set(
        "workspacePackagePatterns",
        serde_json::to_value(&config.workspace_package_patterns)?,
    );
    set(
        "sideEffectsCacheRead",
        Value::Bool(config.side_effects_cache_read()),
    );
    set(
        "sideEffectsCacheWrite",
        Value::Bool(config.side_effects_cache_write()),
    );
    // The settings key of the same name reports only a pinned value; a hook
    // reading it wants the directory the lockfile is actually written to.
    set("lockfileDir", path_value(config.lockfile_dir_for(root_dir)));
    Ok(views)
}

fn path_value(path: &Path) -> Value {
    Value::String(path.to_string_lossy().into_owned())
}
