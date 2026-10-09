use serde_json::Value;

/// A blank name is no name: an SBOM would otherwise carry it as the nameless
/// SPDX actor `Person: `, which strict consumers reject.
#[must_use]
pub fn extract_author(manifest: &Value) -> Option<String> {
    let author = manifest.get("author")?;
    let name = author
        .as_str()
        .or_else(|| author.get("name")?.as_str())?;
    (!name.trim().is_empty()).then(|| name.to_string())
}

/// Extracts the homepage field from a manifest.
#[must_use]
pub fn extract_homepage(manifest: &Value) -> Option<String> {
    manifest
        .get("homepage")
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

/// Extracts the license from either the modern `license` field or the legacy
/// `licenses` field.
#[must_use]
pub fn extract_license(manifest: &Value) -> Option<String> {
    manifest
        .get("license")
        .and_then(extract_license_field)
        .or_else(|| manifest.get("licenses").and_then(extract_license_field))
}

fn extract_license_field(field: &Value) -> Option<String> {
    if let Some(license) = field.as_str() {
        return (!license.is_empty()).then(|| license.to_string());
    }
    if let Some(entries) = field.as_array() {
        let licenses: Vec<&str> = entries
            .iter()
            .filter_map(extract_license_type)
            .collect();
        return match licenses.as_slice() {
            [] => None,
            [license] => Some((*license).to_string()),
            licenses => Some(format!("({})", licenses.join(" OR "))),
        };
    }
    extract_license_type(field).map(ToString::to_string)
}

fn extract_license_type(entry: &Value) -> Option<&str> {
    if let Some(license) = entry
        .as_str()
        .filter(|license| !license.is_empty())
    {
        return Some(license);
    }
    let entry = entry.as_object()?;
    for key in ["type", "name"] {
        if let Some(license) = entry
            .get(key)
            .and_then(Value::as_str)
            .filter(|license| !license.is_empty())
        {
            return Some(license);
        }
    }
    None
}
