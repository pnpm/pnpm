use pnpr_error::{RegistryError, Result};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Reject empty, whitespace-containing, control-containing, and semver-range tag names.
pub fn validate_dist_tag(tag: &str) -> Result<()> {
    if tag.is_empty()
        || tag
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
        || node_semver::Range::parse(tag).is_ok()
    {
        return Err(RegistryError::BadRequest { reason: "invalid dist-tag name".to_string() });
    }
    Ok(())
}

/// Validate tag names, targets, and removal of `latest` against the submitted versions.
pub fn validate_document_dist_tags(hosted: &Value, submitted: &Value) -> Result<()> {
    let tags: BTreeMap<String, String> = serde_json::from_value(
        submitted
            .get("dist-tags")
            .cloned()
            .unwrap_or_else(|| json!({})),
    )
    .map_err(|_| RegistryError::BadRequest {
        reason: "`dist-tags` must be an object mapping tag names to version strings".to_string(),
    })?;
    let versions = submitted
        .get("versions")
        .and_then(Value::as_object)
        .ok_or_else(|| RegistryError::BadRequest {
            reason: "packument must contain an object-valued versions field".to_string(),
        })?;
    for (tag, target) in &tags {
        validate_dist_tag(tag)?;
        validate_dist_tag_target(tag, Some(target), versions.contains_key(target))?;
    }
    if hosted
        .get("dist-tags")
        .and_then(|tags| tags.get("latest"))
        .is_some()
        && !tags.contains_key("latest")
    {
        validate_dist_tag_target("latest", None, !versions.is_empty())?;
    }
    Ok(())
}

pub(crate) fn validate_dist_tag_target(
    tag: &str,
    target: Option<&str>,
    has_version: bool,
) -> Result<()> {
    let reason = match target {
        Some(version) if !has_version => {
            format!("dist-tag {tag:?} target {version:?} is not a published version")
        }
        None if tag == "latest" && has_version => {
            r#"cannot remove dist-tag "latest" while published versions exist"#.to_string()
        }
        _ => return Ok(()),
    };
    Err(RegistryError::BadRequest { reason })
}
