//! Release notes sent with a publish: the publication's note at the top of a
//! batch body, and a version note beside each entry, or at the top of a
//! single npm publish document. They are taken out of the body before its
//! ecosystem's publish path sees it, so no ecosystem's format gains a field,
//! and stored in the publish's own transaction.

use super::{RegistryError, Value};
use crate::server::AppState;
use pnpr_package_name::CanonicalPackageName;
use pnpr_registry::Ecosystem;
use pnpr_storage::metadata::{PublishNotes, VersionNote, validate_note};

const NOTES: &str = "notes";

/// Take a body's top-level `notes` out of it.
pub(in super::super) fn take_note(body: &mut Value) -> Result<Option<String>, RegistryError> {
    let Some(note) = body
        .as_object_mut()
        .and_then(|body| body.remove(NOTES))
    else {
        return Ok(None);
    };
    let Value::String(text) = note else {
        return Err(RegistryError::BadRequest { reason: "`notes` must be a string".to_string() });
    };
    validate_note(&text)?;
    Ok(Some(text))
}

/// Take the version note out of a publish entry of `ecosystem`, as the note
/// of the one version the entry publishes.
pub(in super::super) fn take_version_note(
    entry: &mut Value,
    ecosystem: Ecosystem,
) -> Result<Option<VersionNote>, RegistryError> {
    let Some(text) = take_note(entry)? else { return Ok(None) };
    let (name, version) = entry_version(entry, ecosystem)
        .ok_or_else(|| RegistryError::BadRequest {
            reason: format!(
                "a {ecosystem} entry with `notes` must name the one version it publishes",
            ),
        })?;
    let name = CanonicalPackageName::parse(&name, ecosystem)?;
    Ok(Some(VersionNote { ecosystem, name: name.as_str().to_string(), version, text }))
}

/// The package and version a publish entry publishes.
fn entry_version(entry: &Value, ecosystem: Ecosystem) -> Option<(String, String)> {
    let text = |value: Option<&Value>| value.and_then(Value::as_str).map(str::to_string);
    match ecosystem {
        Ecosystem::Npm => {
            let versions = entry.get("versions")?.as_object()?;
            let [version] = <[&String; 1]>::try_from(versions.keys().collect::<Vec<_>>()).ok()?;
            Some((text(entry.get("name"))?, version.clone()))
        }
        Ecosystem::Cargo => {
            let metadata = entry.get("metadata")?;
            Some((text(metadata.get("name"))?, text(metadata.get("vers"))?))
        }
        Ecosystem::Pypi => Some((text(entry.get("name"))?, text(entry.get("version"))?)),
        Ecosystem::Oci => None,
    }
}

/// Refuse notes a store without `SQLite` metadata cannot keep.
pub(in super::super) fn check_note_storage(
    state: &AppState,
    notes: &PublishNotes,
) -> Result<(), RegistryError> {
    let has_notes = notes.publication.is_some() || !notes.versions.is_empty();
    if has_notes && state.inner.storage.sqlite_metadata().is_none() {
        return Err(RegistryError::BadRequest {
            reason: "release notes need `metadata: { backend: sqlite }`".to_string(),
        });
    }
    Ok(())
}
