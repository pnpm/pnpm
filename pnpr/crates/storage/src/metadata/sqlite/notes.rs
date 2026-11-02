//! Version and publication notes, with the history of every change.
//!
//! A note is keyed by its catalog, its kind, and its subject: a package
//! version (`ecosystem`, `name`, `subject` = version) or a publication
//! (`ecosystem` and `name` empty, `subject` = its id).

use super::{PackageKey, batch::Batch, commit::package_row};
use crate::{
    metadata::{Note, NoteChange, PublishNotes, VersionNote, note_digest, validate_note},
    publish::now_iso,
};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::CanonicalPackageName;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

const VERSION: &str = "version";
const PUBLICATION: &str = "publication";

/// What a note is attached to.
#[derive(Debug, Clone)]
pub(super) struct NoteSubject {
    catalog: String,
    kind: &'static str,
    ecosystem: String,
    name: String,
    subject: String,
}

impl NoteSubject {
    pub(super) fn version(key: &PackageKey, version: &str) -> Self {
        Self {
            catalog: key.catalog.clone(),
            kind: VERSION,
            ecosystem: key.ecosystem.as_str().to_string(),
            name: key.name.clone(),
            subject: version.to_string(),
        }
    }

    pub(super) fn publication(catalog: &str, id: u64) -> Self {
        Self {
            catalog: catalog.to_string(),
            kind: PUBLICATION,
            ecosystem: String::new(),
            name: String::new(),
            subject: id.to_string(),
        }
    }
}

pub(super) fn read_note(connection: &Connection, subject: &NoteSubject) -> Result<Option<Note>> {
    let note = connection
        .query_row(
            "SELECT text, modified_at, principal, initial_digest FROM notes
             WHERE catalog = ?1 AND kind = ?2 AND ecosystem = ?3 AND name = ?4 AND subject = ?5",
            params![
                subject.catalog,
                subject.kind,
                subject.ecosystem,
                subject.name,
                subject.subject
            ],
            note_row,
        )
        .optional()?;
    Ok(note)
}

fn note_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Note> {
    Ok(Note {
        text: row.get(0)?,
        modified: row.get(1)?,
        principal: row.get(2)?,
        initial_digest: row.get(3)?,
    })
}

/// Write or delete a note after its subject was published, recording the
/// change. Answers `false` when the subject does not exist: notes are never
/// created ahead of what they describe.
pub(super) fn change_note(
    connection: &mut Connection,
    subject: &NoteSubject,
    text: Option<&str>,
    principal: &str,
) -> Result<bool> {
    if let Some(text) = text {
        validate_note(text)?;
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if !subject_exists(&transaction, subject)? {
        return Ok(false);
    }
    let previous = read_note(&transaction, subject)?;
    let initial_digest = previous.and_then(|note| note.initial_digest);
    write_note(&transaction, subject, text, Some(principal), initial_digest.as_deref())?;
    transaction.commit()?;
    Ok(true)
}

fn subject_exists(transaction: &Transaction<'_>, subject: &NoteSubject) -> Result<bool> {
    if subject.kind == PUBLICATION {
        let exists: Option<i64> = transaction
            .query_row(
                "SELECT 1 FROM publications WHERE catalog = ?1 AND id = ?2",
                params![subject.catalog, subject.subject.parse::<i64>().unwrap_or(-1)],
                |row| row.get(0),
            )
            .optional()?;
        return Ok(exists.is_some());
    }
    let ecosystem = pnpr_package_name::Ecosystem::all()
        .find(|known| known.as_str() == subject.ecosystem)
        .ok_or_else(|| RegistryError::BadRequest { reason: "unknown ecosystem".to_string() })?;
    let key =
        PackageKey { catalog: subject.catalog.clone(), ecosystem, name: subject.name.clone() };
    if ecosystem != pnpr_package_name::Ecosystem::Npm {
        return Ok(package_row(transaction, &key)?.is_some());
    }
    let exists: Option<i64> = transaction
        .query_row(
            "SELECT 1 FROM versions WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3
             AND version = ?4 LIMIT 1",
            params![key.catalog, subject.ecosystem, key.name, subject.subject],
            |row| row.get(0),
        )
        .optional()?;
    Ok(exists.is_some())
}

/// Store a note's text, `None` for a deletion, keeping its history.
fn write_note(
    transaction: &Transaction<'_>,
    subject: &NoteSubject,
    text: Option<&str>,
    principal: Option<&str>,
    initial_digest: Option<&str>,
) -> Result<()> {
    let now = now_iso();
    transaction.execute(
        "INSERT INTO note_history (catalog, kind, ecosystem, name, subject, changed_at, principal,
                                   previous, next)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7,
           (SELECT text FROM notes WHERE catalog = ?1 AND kind = ?2 AND ecosystem = ?3
              AND name = ?4 AND subject = ?5), ?8)",
        params![
            subject.catalog,
            subject.kind,
            subject.ecosystem,
            subject.name,
            subject.subject,
            now,
            principal,
            text
        ],
    )?;
    transaction.execute(
        "INSERT INTO notes (catalog, kind, ecosystem, name, subject, text, modified_at, principal,
                            initial_digest)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT (catalog, kind, ecosystem, name, subject) DO UPDATE SET
           text = excluded.text, modified_at = excluded.modified_at,
           principal = excluded.principal, initial_digest = excluded.initial_digest",
        params![
            subject.catalog,
            subject.kind,
            subject.ecosystem,
            subject.name,
            subject.subject,
            text,
            now,
            principal,
            initial_digest
        ],
    )?;
    Ok(())
}

/// Store the notes a publish carries, in its transaction, each with the
/// digest of the text it was published with. A note is published only with
/// what it describes: a version note needs its version to be a member of
/// the publication this commit records, and a publication note needs that
/// publication. Anything else is refused, so a note never claims a
/// publish-time digest for a version an earlier commit introduced. A version
/// note for a version that already had one is refused too: a later upload
/// never replaces it.
pub(super) fn store_publish_notes(
    batch: &Batch<'_>,
    notes: &PublishNotes,
    publication: Option<u64>,
    principal: Option<&str>,
) -> Result<()> {
    for note in &notes.versions {
        validate_note(&note.text)?;
        refuse_unpublished_subject(batch, note, publication)?;
        let key = PackageKey {
            catalog: batch.catalog().to_string(),
            ecosystem: note.ecosystem,
            name: note.name.clone(),
        };
        let subject = NoteSubject::version(&key, &note.version);
        store_initial(batch, &subject, &note.text, principal)?;
    }
    let Some(text) = &notes.publication else { return Ok(()) };
    validate_note(text)?;
    let id = publication.ok_or_else(|| RegistryError::BadRequest {
        reason: "a publication note needs a publish that introduces a version".to_string(),
    })?;
    store_initial(batch, &NoteSubject::publication(batch.catalog(), id), text, principal)
}

/// Refuse a version note whose version this commit's publication does not
/// introduce. An npm member names its version; another ecosystem's member
/// names its file, so its package stands for the version.
fn refuse_unpublished_subject(
    batch: &Batch<'_>,
    note: &VersionNote,
    publication: Option<u64>,
) -> Result<()> {
    let introduced = match publication {
        Some(id) => batch
            .transaction()
            .query_row(
                "SELECT 1 FROM publication_members
                 WHERE catalog = ?1 AND publication = ?2 AND ecosystem = ?3 AND name = ?4
                   AND (version = ?5 OR version = '')
                 LIMIT 1",
                params![
                    batch.catalog(),
                    super::to_column(id),
                    note.ecosystem.as_str(),
                    note.name,
                    note.version,
                ],
                |_| Ok(()),
            )
            .optional()?
            .is_some(),
        None => false,
    };
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

fn store_initial(
    batch: &Batch<'_>,
    subject: &NoteSubject,
    text: &str,
    principal: Option<&str>,
) -> Result<()> {
    if read_note(batch.transaction(), subject)?.is_some() {
        return Err(RegistryError::AdminConflict {
            reason: format!("version {} already has a note; edit it instead", subject.subject),
        });
    }
    write_note(batch.transaction(), subject, Some(text), principal, Some(&note_digest(text)))
}

impl super::SqliteMetadata {
    /// The note of `version` of `name` in `org`.
    pub async fn version_note(
        self: &std::sync::Arc<Self>,
        name: &CanonicalPackageName,
        org: &str,
        version: &str,
    ) -> Result<Option<Note>> {
        let subject = NoteSubject::version(&package_key(name, org), version);
        self.read(move |connection, _| read_note(connection, &subject)).await
    }

    /// The current versions of `name` in `org` whose note holds text or the
    /// digest it was published with, without the text. A note added later and
    /// then deleted holds neither.
    pub async fn noted_versions(
        self: &std::sync::Arc<Self>,
        name: &CanonicalPackageName,
        org: &str,
    ) -> Result<Vec<String>> {
        let key = package_key(name, org);
        self.read(move |connection, _| {
            let mut statement = connection.prepare_cached(
                "SELECT n.subject FROM notes n
                 WHERE n.catalog = ?1 AND n.kind = 'version' AND n.ecosystem = ?2 AND n.name = ?3
                   AND (n.text IS NOT NULL OR n.initial_digest IS NOT NULL)
                   AND (n.ecosystem != 'npm' OR EXISTS (
                     SELECT 1 FROM versions v WHERE v.catalog = n.catalog
                       AND v.ecosystem = n.ecosystem AND v.name = n.name
                       AND v.version = n.subject))",
            )?;
            let versions = statement
                .query_map(key.params(), |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()?;
            Ok(versions)
        })
        .await
    }

    /// Replace the note of `version`, or delete it with `None`. Answers
    /// `false` when the version does not exist.
    pub async fn set_version_note(
        self: &std::sync::Arc<Self>,
        name: &CanonicalPackageName,
        org: &str,
        version: &str,
        change: NoteChange,
    ) -> Result<bool> {
        let subject = NoteSubject::version(&package_key(name, org), version);
        self.write(move |connection| {
            change_note(connection, &subject, change.text.as_deref(), &change.principal)
        })
        .await
    }

    /// The note of publication `id` of `org`.
    pub async fn publication_note(
        self: &std::sync::Arc<Self>,
        org: &str,
        id: u64,
    ) -> Result<Option<Note>> {
        let subject = NoteSubject::publication(org, id);
        self.read(move |connection, _| read_note(connection, &subject)).await
    }

    /// Replace the note of publication `id`, or delete it with `None`.
    /// Answers `false` when the publication does not exist.
    pub async fn set_publication_note(
        self: &std::sync::Arc<Self>,
        org: &str,
        id: u64,
        change: NoteChange,
    ) -> Result<bool> {
        let subject = NoteSubject::publication(org, id);
        self.write(move |connection| {
            change_note(connection, &subject, change.text.as_deref(), &change.principal)
        })
        .await
    }
}

fn package_key(name: &CanonicalPackageName, org: &str) -> PackageKey {
    PackageKey {
        catalog: org.to_string(),
        ecosystem: name.ecosystem(),
        name: name.as_str().to_string(),
    }
}

impl super::SqliteMetadata {
    /// Whether `version` of `name` in `org` is accepted and not removed.
    pub async fn has_version(
        self: &std::sync::Arc<Self>,
        name: &CanonicalPackageName,
        org: &str,
        version: &str,
    ) -> Result<bool> {
        let (key, version) = (package_key(name, org), version.to_string());
        self.read(move |connection, _| {
            let (catalog, ecosystem, name) = key.params();
            let found = connection
                .query_row(
                    "SELECT 1 FROM versions
                     WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3 AND version = ?4",
                    rusqlite::params![catalog, ecosystem, name, version],
                    |_| Ok(()),
                )
                .optional()?;
            Ok(found.is_some())
        })
        .await
    }

    /// The initial publication of every current version of `name` in `org`
    /// whose publication note holds text or its publish-time digest, as
    /// `(version, publication id)`. A removed version's publication stays
    /// recorded but is not listed.
    pub async fn noted_publications(
        self: &std::sync::Arc<Self>,
        name: &CanonicalPackageName,
        org: &str,
    ) -> Result<Vec<(String, u64)>> {
        let key = package_key(name, org);
        self.read(move |connection, _| {
            let mut statement = connection.prepare_cached(
                "SELECT p.version, p.publication FROM (
                   SELECT m.version, MIN(m.publication) AS publication FROM publication_members m
                   WHERE m.catalog = ?1 AND m.ecosystem = ?2 AND m.name = ?3
                     AND EXISTS (
                       SELECT 1 FROM versions v WHERE v.catalog = m.catalog
                         AND v.ecosystem = m.ecosystem AND v.name = m.name
                         AND v.version = m.version)
                   GROUP BY m.version
                 ) p
                 WHERE EXISTS (
                   SELECT 1 FROM notes n WHERE n.catalog = ?1 AND n.kind = 'publication'
                     AND n.ecosystem = '' AND n.name = '' AND n.subject = CAST(p.publication AS TEXT)
                     AND (n.text IS NOT NULL OR n.initial_digest IS NOT NULL))",
            )?;
            let rows = statement
                .query_map(key.params(), |row| {
                    Ok((row.get(0)?, super::to_generation(row.get(1)?)))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
    }
}
