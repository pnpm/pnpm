use crate::state::InitStateError;
use indexmap::IndexMap;
use pnpm_config::Config;
use pnpm_package_manifest::PackageManifest;
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use std::path::Path;

/// Validate and promote root resolutions for `prepare_root_config`.
pub fn prepare_root_resolutions<ReporterType: Reporter>(
    config: &mut Config,
    config_root: &Path,
) -> miette::Result<()> {
    let root_manifest_path = config_root.join("package.json");
    if root_manifest_path.is_file() {
        let manifest =
            PackageManifest::from_path(root_manifest_path).map_err(InitStateError::Manifest)?;
        let warnings = apply_root_resolutions_to_config(config, &manifest)?;
        let prefix = config_root.to_string_lossy().to_string();
        for message in warnings {
            ReporterType::emit(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Warn,
                message,
                prefix: prefix.clone(),
            }));
        }
    }
    Ok(())
}

/// Apply root resolutions for the fast path if package.json exists.
pub fn apply_fast_path_resolutions(dir: &Path, config: &mut Config, emit: fn(&LogEvent)) -> bool {
    let manifest_path = dir.join("package.json");
    if !manifest_path.is_file() {
        return true;
    }
    let Ok(manifest) = PackageManifest::from_path(manifest_path) else {
        return false;
    };
    let Ok(warnings) = apply_root_resolutions_to_config(config, &manifest) else {
        return false;
    };
    let prefix = dir.to_string_lossy().to_string();
    for message in warnings {
        emit(&LogEvent::Pnpm(PnpmLog { level: LogLevel::Warn, message, prefix: prefix.clone() }));
    }
    true
}

/// Promote `resolutions` from the root `package.json` into `config.overrides`
/// when no workspace overrides exist, or return an advisory warning when
/// workspace overrides take precedence.
pub fn apply_root_resolutions_to_config(
    config: &mut Config,
    project_manifest: &PackageManifest,
) -> Result<Vec<String>, InitStateError> {
    let root_manifest_path = config.workspace_dir
        .as_ref()
        .map(|dir| dir.join("package.json"));
    match root_manifest_path {
        Some(ref path) if path != project_manifest.path() => {
            if path.is_file() {
                let root_manifest =
                    PackageManifest::from_path(path.clone()).map_err(InitStateError::Manifest)?;
                apply_resolutions_to_config(config, root_manifest.value())
            } else {
                Ok(Vec::new())
            }
        }
        _ => apply_resolutions_to_config(config, project_manifest.value()),
    }
}

/// Read `resolutions` from a manifest and promote them into `config.overrides`,
/// or return a warning when workspace `overrides` takes precedence.
pub fn apply_resolutions_to_config(
    config: &mut Config,
    root_manifest: &serde_json::Value,
) -> Result<Vec<String>, InitStateError> {
    let resolutions_raw = match root_manifest.get("resolutions") {
        None | Some(serde_json::Value::Null) => return Ok(Vec::new()),
        Some(v) => v,
    };
    let resolutions = match resolutions_raw {
        serde_json::Value::Object(map) if !map.is_empty() => map,
        serde_json::Value::Object(_) => return Ok(Vec::new()),
        other => {
            return Err(InitStateError::InvalidResolutionsType {
                actual_type: json_value_type_name(other),
            });
        }
    };
    validate_resolutions(resolutions)?;
    if config.overrides
        .as_ref()
        .is_some_and(|overrides| !overrides.is_empty())
    {
        Ok(vec![
            r#"The "resolutions" field in package.json is ignored because "overrides" in pnpm-workspace.yaml takes precedence. Remove "resolutions" from package.json."#
                .to_string(),
        ])
    } else {
        let warning = promote_resolutions(config, resolutions, root_manifest)?;
        Ok(vec![warning])
    }
}

fn validate_resolutions(
    resolutions: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), InitStateError> {
    for (key, value) in resolutions {
        if !value.is_string() {
            return Err(InitStateError::InvalidResolutionValue {
                selector: sanitize_for_log(key),
                actual_type: json_value_type_name(value),
            });
        }
    }
    Ok(())
}

fn promote_resolutions(
    config: &mut Config,
    resolutions: &serde_json::Map<String, serde_json::Value>,
    root_manifest: &serde_json::Value,
) -> Result<String, InitStateError> {
    let pairs: Vec<(String, String, String)> = resolutions
        .iter()
        .map(|(k, v)| {
            let original = v.as_str().unwrap();
            let resolved = resolve_version_reference(original, root_manifest)?;
            Ok((k.clone(), original.to_owned(), resolved))
        })
        .collect::<Result<_, _>>()?;
    let overrides: IndexMap<String, String> = pairs
        .iter()
        .map(|(k, _, resolved)| (k.clone(), resolved.clone()))
        .collect();
    if !overrides.is_empty() {
        config.overrides = Some(overrides);
    }
    let entries: Vec<String> = pairs
        .iter()
        .map(|(selector, original, resolved)| {
            let selector = sanitize_for_log(selector);
            let original = sanitize_for_log(original);
            let resolved = sanitize_for_log(resolved);
            if original == resolved {
                format!("  {selector}: {resolved}")
            } else {
                format!("  {selector}: {original} -> {resolved}")
            }
        })
        .collect();
    Ok(format_migration_warning(&entries))
}

const MAX_DISPLAYED_ENTRIES: usize = 10;

fn format_migration_warning(entries: &[String]) -> String {
    let mut displayed: Vec<String> = entries
        .iter()
        .take(MAX_DISPLAYED_ENTRIES)
        .cloned()
        .collect();
    if entries.len() > MAX_DISPLAYED_ENTRIES {
        displayed.push(format!("  ...and {} more", entries.len() - MAX_DISPLAYED_ENTRIES));
    }
    format!(
        r#"The "resolutions" field in package.json is deprecated. We attempted to migrate your resolutions to pnpm overrides. Please verify:
{}
Use the "overrides" field in pnpm-workspace.yaml instead."#,
        displayed.join("\n"),
    )
}

fn resolve_version_reference(
    spec: &str,
    manifest: &serde_json::Value,
) -> Result<String, InitStateError> {
    if !spec.starts_with('$') || spec.starts_with("${") {
        return Ok(spec.to_owned());
    }
    let dep_name = &spec[1..];
    let dep_version = ["optionalDependencies", "dependencies", "devDependencies"]
        .iter()
        .find_map(|field| {
            manifest
                .get(*field)
                .and_then(|v| v.get(dep_name))
                .and_then(|v| v.as_str())
        });
    match dep_version {
        Some(v) => Ok(v.to_owned()),
        None => Err(InitStateError::CannotResolveOverrideVersion {
            spec: sanitize_for_log(spec),
            dep_name: sanitize_for_log(dep_name),
        }),
    }
}

fn json_value_type_name(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_owned(),
        serde_json::Value::Bool(_) => "boolean".to_owned(),
        serde_json::Value::Number(_) => "number".to_owned(),
        serde_json::Value::String(_) => "string".to_owned(),
        serde_json::Value::Array(_) => "array".to_owned(),
        serde_json::Value::Object(_) => "object".to_owned(),
    }
}

/// Replace control characters (C0, DEL, and C1) with `'?'`.
pub fn sanitize_for_log(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_control() { '?' } else { ch })
        .collect()
}

#[cfg(test)]
mod tests;
