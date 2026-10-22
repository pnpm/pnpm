use super::{
    Arc, Config, Host, IntoDiagnostic, Path, Result, Value, WorkspaceSettings,
    apply::apply_hook_delta,
};
use miette::WrapErr;
use pnpm_workspace::{PackageEntry, WorkspaceIncludes};
use std::collections::HashSet;

/// Apply the settings of the config dependencies the workspace manifest
/// includes, now that they are installed.
///
/// Config loading reads the entries of the config dependencies already
/// installed. One installed or changed since is applied here, below the
/// settings the environment and the command line give. A command that
/// installs config dependencies fails rather than run without the settings
/// of one it could not load.
pub fn apply_config_dependency_settings(config: &mut Config) -> Result<()> {
    let Some(loaded) = config.workspace_includes.clone() else {
        return Ok(());
    };
    let Some(workspace_dir) = config.workspace_dir.clone() else {
        return Ok(());
    };
    if loaded.packages().next().is_none() {
        return Ok(());
    }
    let includes = pnpm_workspace::read_workspace_manifest_document(&workspace_dir)
        .map_err(miette::Report::new)?
        .and_then(|document| document.includes)
        .unwrap_or_else(|| WorkspaceIncludes::clone(&loaded));
    if let Some(name) = includes.unavailable_packages().next() {
        let package_dir = pnpm_workspace::config_dependency_dir(&workspace_dir, name);
        let package_dir = package_dir.display();
        return Err(miette::miette!(
            code = "ERR_PNPM_CONFIG_DEPENDENCY_SETTINGS_UNAVAILABLE",
            r#"The settings of the config dependency "{name}" cannot be loaded because it is not installed in {package_dir}"#,
        ));
    }
    if includes.package_entries != loaded.package_entries {
        apply_entries(config, (&loaded, &includes), &workspace_dir)?;
    }
    config.workspace_includes = Some(Arc::new(includes));
    Ok(())
}

/// Apply what changed between the `loaded` and the `installed` config
/// entries. A setting the installed entries no longer set returns to its
/// default, as one an `updateConfig` hook deletes does.
fn apply_entries(
    config: &mut Config,
    (loaded, installed): (&WorkspaceIncludes, &WorkspaceIncludes),
    base_dir: &Path,
) -> Result<()> {
    let input = serde_json::to_value(WorkspaceSettings::from_resolved(config))
        .into_diagnostic()
        .wrap_err("serialize the resolved settings")?;
    let mut current = input.clone();
    let overridden = environment_setting_keys()?;
    let installed_values = entry_settings(installed)?;
    for (key, _) in entry_settings(loaded)? {
        if !installed_values.contains_key(&key) && !overridden.contains(&key) {
            current[key] = Value::Null;
        }
    }
    for (key, value) in installed_values {
        if !overridden.contains(&key) {
            current[key] = value;
        }
    }
    apply_hook_delta(config, &input, &current, base_dir)
}

/// The settings the loaded config entries of `includes` set.
fn entry_settings(includes: &WorkspaceIncludes) -> Result<serde_json::Map<String, Value>> {
    let mut settings = serde_json::Map::new();
    for entry in includes.package_entries.values() {
        let PackageEntry::Loaded { path, text } = entry else {
            continue;
        };
        let entry = WorkspaceSettings::read_config_dependency_entry::<Host>(text, path)
            .map_err(miette::Report::new)?;
        if let Value::Object(values) = serde_json::to_value(entry).into_diagnostic()? {
            settings.extend(
                values
                    .into_iter()
                    .filter(|(_, value)| !value.is_null()),
            );
        }
    }
    Ok(settings)
}

/// The settings `PNPM_CONFIG_*` variables give, which a project setting does
/// not override.
fn environment_setting_keys() -> Result<HashSet<String>> {
    let Value::Object(values) =
        serde_json::to_value(WorkspaceSettings::from_pnpm_config_env::<Host>()).into_diagnostic()?
    else {
        return Ok(HashSet::new());
    };
    Ok(values
        .into_iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(key, _)| key)
        .collect())
}
