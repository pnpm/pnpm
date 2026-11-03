//! The notes a publish carries, stored in its transaction with the digest of
//! the text each was published with.

use super::{NoteSubject, VERSION, write_note};
use crate::metadata::{
    ArtifactMember, PublishNotes, VersionNote, note_digest,
    sql::{PackageKey, adapter::Query, batch::Batch},
    validate_note,
};
use pnpr_error::{RegistryError, Result};

/// Store the notes a publish carries, in its transaction, each with the
/// digest of the text it was published with. A note is published only with
/// what it describes: a version note needs its version to be a member of
/// the publication this commit records, and a publication note needs that
/// publication. Anything else is refused, so a note never claims a
/// publish-time digest for a version an earlier commit introduced. A version
/// note for a version that already had one is refused too: a later upload
/// never replaces it.
pub(in crate::metadata::sql) fn store_publish_notes(
    batch: &Batch<'_>,
    notes: &PublishNotes,
    publication: Option<(u64, &[ArtifactMember])>,
    noted: &[bool],
    principal: Option<&str>,
) -> Result<()> {
    let mut written = std::collections::HashSet::new();
    for (note, noted) in notes.versions.iter().zip(noted) {
        validate_note(&note.text)?;
        refuse_unpublished_subject(note, publication)?;
        // A request that notes one version twice is refused as a later
        // upload would be.
        if *noted || !written.insert((note.ecosystem, &note.name, &note.version)) {
            return Err(already_noted(&note.version));
        }
        let key = PackageKey {
            catalog: batch.catalog().to_string(),
            ecosystem: note.ecosystem,
            name: note.name.clone(),
        };
        let subject = NoteSubject::version(&key, &note.version);
        write_initial(batch, &subject, &note.text, principal)?;
    }
    let Some(text) = &notes.publication else { return Ok(()) };
    validate_note(text)?;
    let (id, _) = publication.ok_or_else(|| RegistryError::BadRequest {
        reason: "a publication note needs a publish that introduces a version".to_string(),
    })?;
    // The publication is this batch's own, so it has no note yet.
    write_initial(batch, &NoteSubject::publication(batch.catalog(), id), text, principal)
}

/// The read that finds a row when `note`'s version already has a note, for
/// [`store_publish_notes`].
pub(in crate::metadata::sql) fn version_note_query<'query>(
    catalog: &'query str,
    note: &'query VersionNote,
) -> Query<'query> {
    query!(
        "SELECT 1 FROM notes
         WHERE catalog = ?1 AND kind = ?2 AND ecosystem = ?3 AND name = ?4 AND subject = ?5",
        catalog,
        VERSION,
        note.ecosystem.as_str(),
        &note.name,
        &note.version,
    )
}

/// Refuse a version note whose version this commit's publication does not
/// introduce. An npm member names its version; another ecosystem's member
/// names its file, so its package stands for the version.
fn refuse_unpublished_subject(
    note: &VersionNote,
    publication: Option<(u64, &[ArtifactMember])>,
) -> Result<()> {
    let introduced = publication.is_some_and(|(_, members)| {
        members
            .iter()
            .any(|member| {
                member.ecosystem == note.ecosystem
                    && member.name == note.name
                    && (member.version == note.version || member.version.is_empty())
            })
    });
    if introduced {
        return Ok(());
    }
    Err(RegistryError::AdminConflict {
        reason: format!(
            "{}@{} is not published by this request, so it takes no publish-time note; edit its \
             note instead",
            note.name, note.version,
        ),
    })
}

fn already_noted(subject: &str) -> RegistryError {
    RegistryError::AdminConflict {
        reason: format!("version {subject} already has a note; edit it instead"),
    }
}

fn write_initial(
    batch: &Batch<'_>,
    subject: &NoteSubject,
    text: &str,
    principal: Option<&str>,
) -> Result<()> {
    write_note(batch.transaction(), subject, Some(text), principal, Some(&note_digest(text)))
}
