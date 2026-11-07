use pnpr_error::RegistryError;

/// Reject empty, whitespace-containing, control-containing, and semver-range tag names.
pub fn validate_dist_tag(tag: &str) -> Result<(), RegistryError> {
    if node_semver::Range::parse(tag).is_ok()
        || tag.is_empty()
        || tag
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(RegistryError::BadRequest { reason: "invalid dist-tag name".to_string() });
    }
    Ok(())
}
