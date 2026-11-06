use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use serde_json::Value;

/// The stored document's content identity, before response projections and overlays.
pub(in super::super) fn revision(document: &Value) -> String {
    let bytes = serde_json::to_vec(document).expect("JSON values serialize");
    super::super::ecosystem::sha256_hex(&bytes)
}

pub(in super::super) fn check(
    name: &CanonicalPackageName,
    hosted: &Value,
    submitted: Option<&str>,
) -> Result<(), RegistryError> {
    if submitted == Some(revision(hosted).as_str()) {
        return Ok(());
    }
    Err(RegistryError::DocumentWriteConflict { package: name.as_str().to_string() })
}

/// Limit the merge to existing versions, whose only mutable field is `deprecated`.
pub(in super::super) fn prepare_deprecation(
    name: &CanonicalPackageName,
    hosted: &Value,
    incoming: &mut Value,
) -> Result<(), RegistryError> {
    check(name, hosted, incoming.get("_rev").and_then(Value::as_str))?;
    if let Some(object) = incoming.as_object_mut() {
        object.retain(|key, _| key == "versions");
    }
    Ok(())
}
