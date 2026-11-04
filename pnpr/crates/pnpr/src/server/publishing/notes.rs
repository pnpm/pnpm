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
        // The normalized version, as the published file records it.
        Ecosystem::Pypi => {
            let version = pnpr_pypi::normalize_version(&text(entry.get("version"))?).ok()?;
            Some((text(entry.get("name"))?, version))
        }
        Ecosystem::Oci => None,
    }
}

/// Refuse a Python version note whose version the project already has a file
/// of: a publish that adds another distribution of a published version does
/// not publish the version, so it takes no publish-time note. Runs under the
/// package locks, before staging. An npm or Cargo version is published once,
/// so the commit's own check covers it.
pub(in super::super) async fn refuse_notes_of_published_python_versions(
    state: &AppState,
    org: &str,
    notes: &PublishNotes,
) -> Result<(), RegistryError> {
    let storage = state.inner.storage.for_hosted(org, Ecosystem::Pypi);
    for note in notes.versions
        .iter()
        .filter(|note| note.ecosystem == Ecosystem::Pypi)
    {
        let name = CanonicalPackageName::parse(&note.name, Ecosystem::Pypi)?;
        let Some(bytes) = storage.read_hosted_document(&name).await? else { continue };
        let project = pnpr_pypi::ProjectDocument::parse(&bytes)?;
        let published = project.files
            .iter()
            .any(|file| {
                pnpr_pypi::parse_distribution_filename(&file.filename)
                    .ok()
                    .and_then(|distribution| {
                        pnpr_pypi::normalize_version(&distribution.version).ok()
                    })
                    .is_some_and(|version| version == note.version)
            });
        if published {
            return Err(RegistryError::AdminConflict {
                reason: format!(
                    "{}@{} is already published, so it takes no publish-time note; edit its \
                     note instead",
                    note.name, note.version,
                ),
            });
        }
    }
    Ok(())
}
