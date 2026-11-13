//! Shape validation for structured YAML settings written by `pnpm config set`.
//!
//! Scalar settings are coerced by `castField`; structured sections require a
//! JSON object matching the target type so `pnpm config set` never writes a
//! `config.yaml` or `pnpm-workspace.yaml` that later commands refuse to load.

use super::{
    BTreeMap, HashMap, IndexMap, LoadWorkspaceYamlError, MacosBackupSettings,
    NodePackageMapSettings, Path, PathBuf, RegistryEntry, RemoteCacheSettings,
    RemoteSideEffectsCacheSettings, WORKSPACE_MANIFEST_FILENAME, WorkspaceSettings,
    deserialize_tools, registries, validation::is_valid_concurrency_group_name,
};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

/// Validate `value` for a structured config-file key (`camelCase`). Non-structured
/// keys and `null` (deletion) return `Ok(())`.
pub fn validate_structured_setting(
    camel_key: &str,
    value: &Value,
    is_project_manifest: bool,
) -> Result<(), String> {
    if value.is_null() {
        return Ok(());
    }
    match camel_key {
        "macosBackup" => validate_known_fields::<MacosBackupSettings>(
            value,
            &["excludeModulesDir", "excludeStoreDir"],
        ),
        "tools" => {
            require_object(value)?;
            deserialize_tools(value.clone())
                .map(|_| ())
                .map_err(|error| error.to_string())
        }
        "nodePackageMap" => {
            validate_known_fields::<NodePackageMapSettings>(value, &["mode", "type"])
        }
        "remoteSideEffectsCache" => validate_remote_side_effects_cache(value, is_project_manifest),
        "remoteCache" => validate_remote_cache(value, is_project_manifest),
        "concurrencyGroups" => validate_concurrency_groups(value),
        "namedRegistries" => parse_object::<BTreeMap<String, String>>(value).map(|_| ()),
        "nodeDownloadMirrors" => parse_object::<HashMap<String, String>>(value).map(|_| ()),
        "registries" => {
            let entries = parse_object::<IndexMap<String, RegistryEntry>>(value)?;
            registries::validate(&entries).map_err(|error| error.to_string())
        }
        _ => Ok(()),
    }
}

fn require_object(value: &Value) -> Result<&Map<String, Value>, String> {
    value.as_object().ok_or_else(|| "expected a JSON object".to_string())
}

fn parse_object<Setting: DeserializeOwned>(value: &Value) -> Result<Setting, String> {
    require_object(value)?;
    serde_json::from_value(value.clone()).map_err(|error| error.to_string())
}

fn validate_known_fields<Setting: DeserializeOwned>(
    value: &Value,
    allowed_fields: &[&str],
) -> Result<(), String> {
    let object = require_object(value)?;
    if let Some(unknown) = object
        .keys()
        .find(|field| !allowed_fields.contains(&field.as_str()))
    {
        return Err(format!("unknown field {unknown:?}"));
    }
    serde_json::from_value::<Setting>(value.clone())
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn validate_remote_side_effects_cache(
    value: &Value,
    is_project_manifest: bool,
) -> Result<(), String> {
    let settings = parse_object::<RemoteSideEffectsCacheSettings>(value)?;
    if is_project_manifest {
        WorkspaceSettings::reject_machine_only_fields(
            &settings,
            "remoteSideEffectsCache",
            Path::new(WORKSPACE_MANIFEST_FILENAME),
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn validate_remote_cache(value: &Value, is_project_manifest: bool) -> Result<(), String> {
    let settings = parse_object::<RemoteCacheSettings>(value)?;
    if is_project_manifest
        && let Some((field, _)) = settings
            .machine_only_fields()
            .into_iter()
            .find(|(_, is_set)| *is_set)
    {
        return Err(LoadWorkspaceYamlError::WorkspaceRemoteCacheTrust {
            path: PathBuf::from(WORKSPACE_MANIFEST_FILENAME),
            field,
        }
        .to_string());
    }
    Ok(())
}

fn validate_concurrency_groups(value: &Value) -> Result<(), String> {
    let groups = parse_object::<IndexMap<String, u32>>(value)?;
    if let Some(group) = groups
        .keys()
        .find(|group| !is_valid_concurrency_group_name(group))
    {
        return Err(format!("invalid concurrency group name {group:?}"));
    }
    Ok(())
}
