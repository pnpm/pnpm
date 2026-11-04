//! Version and publication notes, with the history of every change.
//!
//! A note is keyed by its catalog, its kind, and its subject: a package
//! version (`ecosystem`, `name`, `subject` = version) or a publication
//! (`ecosystem` and `name` empty, `subject` = its id).

pub(super) use publish_time::{store_publish_notes, version_note_query};

use super::{
    PackageKey,
    adapter::{Connection, Query, Row},
    batch::WriteTransaction,
    commit::package_row_query,
    transactions::Freshness,
};
use crate::{
    metadata::{Note, NoteChange, validate_note},
    publish::now_iso,
};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::CanonicalPackageName;

mod publish_time;

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

pub(super) fn read_note(
    connection: &dyn Connection,
    subject: &NoteSubject,
) -> Result<Option<Note>> {
    let query = note_query(subject);
    connection
        .query_optional(query.sql, &query.params)?
        .map(note_row)
        .transpose()
}

fn note_query(subject: &NoteSubject) -> Query<'_> {
    query!(
        "SELECT text, modified_at, principal, initial_digest FROM notes
         WHERE catalog = ?1 AND kind = ?2 AND ecosystem = ?3 AND name = ?4 AND subject = ?5",
        &subject.catalog,
        subject.kind,
        &subject.ecosystem,
        &subject.name,
        &subject.subject,
    )
}

fn note_row(mut row: Row) -> Result<Note> {
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
    connection: &dyn Connection,
    subject: &NoteSubject,
    text: Option<&str>,
    principal: &str,
) -> Result<bool> {
    if let Some(text) = text {
        validate_note(text)?;
    }
    let transaction = WriteTransaction::begin(connection)?;
    let key = subject_key(subject)?;
    let reads = [subject_query(subject, key.as_ref()), note_query(subject)];
    let [found, previous] = <[Vec<Row>; 2]>::try_from(transaction.query_all(&reads)?)
        .map_err(|_| RegistryError::Internal {
            reason: "a metadata batch did not answer every read of a note".to_string(),
        })?;
    if found.is_empty() {
        return Ok(false);
    }
    let previous = previous
        .into_iter()
        .next()
        .map(note_row)
        .transpose()?;
    let initial_digest = previous.and_then(|note| note.initial_digest);
    write_note(&*transaction, subject, text, Some(principal), initial_digest.as_deref())?;
    transaction.commit()?;
    Ok(true)
}

/// The package a version note's subject belongs to; `None` for a
/// publication note.
fn subject_key(subject: &NoteSubject) -> Result<Option<PackageKey>> {
    if subject.kind == PUBLICATION {
        return Ok(None);
    }
    let ecosystem = pnpr_package_name::Ecosystem::all()
        .find(|known| known.as_str() == subject.ecosystem)
        .ok_or_else(|| RegistryError::BadRequest { reason: "unknown ecosystem".to_string() })?;
    Ok(Some(PackageKey { catalog: subject.catalog.clone(), ecosystem, name: subject.name.clone() }))
}

/// The read that finds a row when the note's subject exists: the
/// publication, the npm version, or for another ecosystem, whose versions
/// have no rows of their own, its package.
fn subject_query<'query>(
    subject: &'query NoteSubject,
    key: Option<&'query PackageKey>,
) -> Query<'query> {
    match key {
        None => Query {
            sql: "SELECT 1 FROM publications WHERE catalog = ?1 AND id = ?2",
            params: vec![
                super::adapter::Param::Text(&subject.catalog),
                super::adapter::Param::Integer(subject.subject.parse::<i64>().unwrap_or(-1)),
            ],
        },
        Some(key) if key.ecosystem != pnpr_package_name::Ecosystem::Npm => package_row_query(key),
        Some(key) => query!(
            "SELECT 1 FROM versions WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3
             AND version = ?4 LIMIT 1",
            &key.catalog,
            &subject.ecosystem,
            &key.name,
            &subject.subject,
        ),
    }
}

/// Store a note's text, `None` for a deletion, keeping its history.
fn write_note(
    transaction: &dyn Connection,
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

impl super::SqlMetadata {
    /// The note of `version` of `name` in `org`.
    pub async fn version_note(
        self: &std::sync::Arc<Self>,
        name: &CanonicalPackageName,
        org: &str,
        version: &str,
    ) -> Result<Option<Note>> {
        let subject = NoteSubject::version(&package_key(name, org), version);
        self.read(org.to_string(), Freshness::Bounded, move |connection, _| {
            read_note(connection, &subject)
        })
        .await
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
        self.read(org.to_string(), Freshness::Bounded, move |connection, _| {
            let (catalog, ecosystem, name) = key.params();
            connection
                .query(
                    "SELECT n.subject FROM notes n
                     WHERE n.catalog = ?1 AND n.kind = 'version' AND n.ecosystem = ?2
                       AND n.name = ?3 AND (n.text IS NOT NULL OR n.initial_digest IS NOT NULL)
                       AND (n.ecosystem != 'npm' OR EXISTS (
                         SELECT 1 FROM versions v WHERE v.catalog = n.catalog
                           AND v.ecosystem = n.ecosystem AND v.name = n.name
                           AND v.version = n.subject))",
                    params![catalog, ecosystem, name],
                )?
                .into_iter()
                .map(|mut row| row.get(0))
                .collect()
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
        self.read(org.to_string(), Freshness::Bounded, move |connection, _| {
            read_note(connection, &subject)
        })
        .await
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

impl super::SqlMetadata {
    /// Whether `version` of `name` in `org` is accepted and not removed.
    pub async fn has_version(
        self: &std::sync::Arc<Self>,
        name: &CanonicalPackageName,
        org: &str,
        version: &str,
    ) -> Result<bool> {
        let (key, version) = (package_key(name, org), version.to_string());
        self.read(org.to_string(), Freshness::Bounded, move |connection, _| {
            let (catalog, ecosystem, name) = key.params();
            let found = connection.query_optional(
                "SELECT 1 FROM versions
                 WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3 AND version = ?4",
                params![catalog, ecosystem, name, version],
            )?;
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
        self.read(org.to_string(), Freshness::Bounded, move |connection, _| {
            let (catalog, ecosystem, name) = key.params();
            let rows = connection.query(
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
                params![catalog, ecosystem, name],
            )?;
            rows.into_iter()
                .map(|mut row| Ok((row.get(0)?, super::to_generation(row.get(1)?))))
                .collect()
        })
        .await
    }
}
