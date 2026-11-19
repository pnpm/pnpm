use crate::state::InitStateError;
use indexmap::IndexMap;
use pnpm_config::Config;
use pnpm_network::redact_url_credentials;
use pnpm_package_manifest::safe_read_project_manifest_from_dir;
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pnpm_text_sanitize::sanitize_inline;
use std::path::Path;

/// Validate and promote root resolutions for `prepare_root_config`.
/// `config_root` is the root project's directory.
pub fn prepare_root_resolutions<ReporterType: Reporter>(
    config: &mut Config,
    config_root: &Path,
) -> miette::Result<()> {
    let Some(manifest) =
        safe_read_project_manifest_from_dir(config_root).map_err(InitStateError::Manifest)?
    else {
        return Ok(());
    };
    let warnings = apply_resolutions_to_config(config, &manifest)?;
    emit_resolutions_warnings(ReporterType::emit, config_root, warnings);
    Ok(())
}

/// Apply root resolutions for the repeat-install fast path started in `dir`.
/// `None` means the manifest or its resolutions are invalid, which the fast
/// path leaves to the full install to report.
pub fn apply_fast_path_resolutions(dir: &Path, config: &mut Config) -> Option<Vec<String>> {
    let root_dir = config.root_project_manifest_dir(dir).to_path_buf();
    let Some(manifest) = safe_read_project_manifest_from_dir(&root_dir).ok()? else {
        return Some(Vec::new());
    };
    apply_resolutions_to_config(config, &manifest).ok()
}

pub fn emit_resolutions_warnings(emit: fn(&LogEvent), dir: &Path, warnings: Vec<String>) {
    let prefix = dir.to_string_lossy().to_string();
    for message in warnings {
        emit(&LogEvent::Pnpm(PnpmLog { level: LogLevel::Warn, message, prefix: prefix.clone() }));
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
    let mut overrides = IndexMap::new();
    let mut entries = Vec::new();
    let mut skipped = Vec::new();
    for (yarn_selector, spec) in resolutions {
        let Some(selector) = to_override_selector(yarn_selector) else {
            skipped.push(sanitize_for_log(yarn_selector));
            continue;
        };
        let original = spec.as_str().unwrap_or_default();
        let resolved = resolve_version_reference(original, root_manifest)?;
        entries.push(format_migrated_entry(&selector, original, &resolved));
        overrides.insert(selector, resolved);
    }
    if !overrides.is_empty() {
        config.overrides = Some(overrides);
    }
    Ok(format_migration_warning(&entries, &skipped))
}

fn format_migrated_entry(selector: &str, original: &str, resolved: &str) -> String {
    let selector = sanitize_for_log(selector);
    let original = sanitize_for_log(original);
    let resolved = sanitize_for_log(resolved);
    if original == resolved {
        format!("  {selector}: {resolved}")
    } else {
        format!("  {selector}: {original} -> {resolved}")
    }
}

/// Convert a Yarn `resolutions` selector to a pnpm override selector:
/// `**/foo` becomes `foo` and `parent/foo` becomes `parent>foo`. `None` for a
/// path pnpm overrides cannot express, such as `a/**/foo` or `a/b/foo`. A
/// selector in pnpm's own `parent>child` syntax is kept.
fn to_override_selector(yarn_selector: &str) -> Option<String> {
    if yarn_selector.contains('>') {
        return Some(yarn_selector.to_owned());
    }
    let mut path = yarn_selector.trim();
    while let Some(rest) = path.strip_prefix("**/") {
        path = rest;
    }
    let mut packages: Vec<String> = Vec::new();
    let mut segments = path.split('/');
    while let Some(segment) = segments.next() {
        let package = match segment.strip_prefix('@') {
            Some(_) => format!("{segment}/{}", segments.next()?),
            None => segment.to_owned(),
        };
        if package.is_empty() || package == "**" {
            return None;
        }
        packages.push(package);
    }
    match packages.as_slice() {
        [name] => Some(name.clone()),
        [parent, name] => Some(format!("{parent}>{name}")),
        _ => None,
    }
}

const MAX_DISPLAYED_ENTRIES: usize = 10;

fn format_migration_warning(entries: &[String], skipped: &[String]) -> String {
    let mut displayed: Vec<String> = entries
        .iter()
        .take(MAX_DISPLAYED_ENTRIES)
        .cloned()
        .collect();
    if entries.len() > MAX_DISPLAYED_ENTRIES {
        displayed.push(format!("  ...and {} more", entries.len() - MAX_DISPLAYED_ENTRIES));
    }
    let skipped_note = if skipped.is_empty() {
        String::new()
    } else {
        format!(
            "\nThese resolutions have no pnpm override equivalent and were skipped: {}",
            skipped.join(", "),
        )
    };
    format!(
        r#"The "resolutions" field in package.json is deprecated. We attempted to migrate your resolutions to pnpm overrides. Please verify:
{}{skipped_note}
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

/// Make manifest text safe to print on one line: strip control and
/// formatting characters, then inline URL credentials.
fn sanitize_for_log(text: &str) -> String {
    redact_url_credentials(&sanitize_inline(text))
}

#[cfg(test)]
mod tests;
