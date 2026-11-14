//! Shape validation for structured YAML settings written by `pnpm config set`.
//!
//! Scalar settings are coerced by `castField`; structured sections validate
//! through the YAML reader's placeholder-aware parser so `pnpm config set`
//! accepts `${VAR}` / `${VAR:-fallback}` placeholders while refusing values
//! that later commands would reject on load.

use super::{
    MachineSettings, Path, SystemEnv, WORKSPACE_MANIFEST_FILENAME, WorkspaceSettings,
    has_env_placeholder, parse_settings, validation::is_valid_concurrency_group_name,
};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

/// Which YAML configuration file `pnpm config set` is targeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YamlConfigTarget {
    /// Global `<configDir>/config.yaml` (machine-controlled).
    Global,
    /// Project `pnpm-workspace.yaml` (repository-controlled).
    Project,
}

/// Validate `value` for a structured config-file key (`camelCase`). Non-structured
/// keys and `null` (deletion) return `Ok(())`.
pub fn validate_structured_setting(
    camel_key: &str,
    value: &Value,
    target: YamlConfigTarget,
) -> Result<(), String> {
    if value.is_null() {
        return Ok(());
    }
    match camel_key {
        "macosBackup" => {
            reject_unknown_fields(value, &["excludeModulesDir", "excludeStoreDir"])?;
            parse_yaml_setting::<MachineSettings>(camel_key, value).map(|_| ())
        }
        "tools" => {
            require_object(value)?;
            parse_yaml_setting::<MachineSettings>(camel_key, value).map(|_| ())
        }
        "nodePackageMap" => {
            reject_unknown_fields(value, &["mode", "type"])?;
            parse_workspace_yaml_setting(camel_key, value, target).map(|_| ())
        }
        "sideEffectsCache" => {
            if value.is_object() {
                reject_unknown_fields(value, &["read", "write", "remote", "exclude"])?;
            }
            parse_workspace_yaml_setting(camel_key, value, target).map(|_| ())
        }
        "concurrencyGroups" => validate_concurrency_groups(value, target),
        "remoteSideEffectsCache"
        | "remoteCache"
        | "namedRegistries"
        | "nodeDownloadMirrors"
        | "registries" => {
            require_object(value)?;
            parse_workspace_yaml_setting(camel_key, value, target).map(|_| ())
        }
        _ => Ok(()),
    }
}

fn validate_concurrency_groups(value: &Value, target: YamlConfigTarget) -> Result<(), String> {
    require_object(value)?;
    let settings = parse_workspace_yaml_setting("concurrencyGroups", value, target)?;
    if let Some(group) = settings.concurrency_groups
        .iter()
        .flat_map(|groups| groups.keys())
        .find(|group| !has_env_placeholder(group) && !is_valid_concurrency_group_name(group))
    {
        return Err(format!("invalid concurrency group name {group:?}"));
    }
    Ok(())
}

fn require_object(value: &Value) -> Result<&Map<String, Value>, String> {
    value.as_object().ok_or_else(|| "expected a JSON object".to_string())
}

fn reject_unknown_fields(value: &Value, allowed_fields: &[&str]) -> Result<(), String> {
    let object = require_object(value)?;
    if let Some(unknown) = object
        .keys()
        .find(|field| !allowed_fields.contains(&field.as_str()))
    {
        return Err(format!("unknown field {unknown:?}"));
    }
    Ok(())
}

fn parse_workspace_yaml_setting(
    camel_key: &str,
    value: &Value,
    target: YamlConfigTarget,
) -> Result<WorkspaceSettings, String> {
    let mut settings = parse_yaml_setting::<WorkspaceSettings>(camel_key, value)?;
    settings.validate_sections(false).map_err(|error| error.to_string())?;
    match target {
        YamlConfigTarget::Project => settings
            .reject_repo_controlled_trust_material(Path::new(WORKSPACE_MANIFEST_FILENAME))
            .map_err(|error| error.to_string())?,
        YamlConfigTarget::Global => {}
    }
    Ok(settings)
}

fn parse_yaml_setting<Settings: DeserializeOwned + Default>(
    camel_key: &str,
    value: &Value,
) -> Result<Settings, String> {
    let yaml = render_yaml_top_level_field(camel_key, value);
    parse_settings::<SystemEnv, Settings>(&yaml).map_err(|error| error.to_string())
}

/// Render a top-level YAML `key: value` block. Scalars render inline; objects
/// and arrays render as an indented block body via [`yaml_serde`].
#[must_use]
pub fn render_yaml_top_level_field(key: &str, value: &Value) -> String {
    let key_text = render_yaml_scalar(key);
    match value {
        Value::String(scalar) => format!("{key_text}: {}\n", render_yaml_scalar(scalar)),
        Value::Number(number) => format!("{key_text}: {number}\n"),
        Value::Bool(boolean) => format!("{key_text}: {boolean}\n"),
        Value::Null => format!("{key_text}: null\n"),
        Value::Array(_) | Value::Object(_) => {
            let body =
                yaml_serde::to_string(value).expect("serializing a JSON value to YAML never fails");
            let mut out = format!("{key_text}:\n");
            for line in body.trim_end_matches('\n').lines() {
                if line.is_empty() {
                    out.push('\n');
                } else {
                    out.push_str("  ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
            out
        }
    }
}

fn render_yaml_scalar(value: &str) -> String {
    yaml_serde::to_string(&yaml_serde::Value::from(value))
        .expect("serializing a string scalar to YAML never fails")
        .trim_end()
        .to_string()
}
