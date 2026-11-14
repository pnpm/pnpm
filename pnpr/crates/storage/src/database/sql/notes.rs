//! Version and publication notes.
//!
//! A version note is keyed by its package and version, a publication note by
//! its namespace and publication id.

pub(super) use publish_time::{store_publish_notes, version_note_query};

use super::{
    PackageKey,
    adapter::{Connection, Query, Row},
    batch::WriteTransaction,
    commit::package_row_query,
    to_column,
    transactions::Freshness,
};
use crate::{
    database::{Note, NoteChange, validate_note},
    publish::{iso_from_unix_millis, now_millis},
};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use pnpr_sql::DialectSql;

mod publish_time;

/// What a note is attached to.
#[derive(Debug, Clone)]
pub(super) enum NoteSubject {
    Version { key: PackageKey, version: String },
    Publication { namespace: String, id: i64 },
}

impl NoteSubject {
    pub(super) fn version(key: &PackageKey, version: &str) -> Self {
        Self::Version { key: key.clone(), version: version.to_string() }
    }

    pub(super) fn publication(namespace: &str, id: u64) -> Self {
        Self::Publication { namespace: namespace.to_string(), id: to_column(id) }
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
    match subject {
        NoteSubject::Version { key, version } => {
            let (namespace, ecosystem, name) = key.params();
            query!(
                "SELECT text, modified_at, principal, initial_digest FROM version_notes
                 WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3 AND version = ?4",
                namespace,
                ecosystem,
                name,
                version,
            )
        }
        NoteSubject::Publication { namespace, id } => query!(
            "SELECT text, modified_at, principal, initial_digest FROM publication_notes
             WHERE namespace = ?1 AND publication = ?2",
            namespace,
            id,
        ),
    }
}

fn note_row(mut row: Row) -> Result<Note> {
    Ok(Note {
        text: row.get(0)?,
        modified: iso_from_unix_millis(row.get(1)?),
        principal: row.get(2)?,
        initial_digest: row.get(3)?,
    })
}

/// Write or delete a note after its subject was published. Answers `false`
/// when the subject does not exist.
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
    let reads = [subject_query(subject), note_query(subject)];
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

/// The read that finds a row when the note's subject exists: the
/// publication, the npm version, or for another ecosystem, whose versions
/// have no rows of their own, its package.
fn subject_query(subject: &NoteSubject) -> Query<'_> {
    match subject {
        NoteSubject::Publication { namespace, id } => {
            query!("SELECT 1 FROM publications WHERE namespace = ?1 AND id = ?2", namespace, id,)
        }
        NoteSubject::Version { key, .. } if key.ecosystem != Ecosystem::Npm => {
            package_row_query(key)
        }
        NoteSubject::Version { key, version } => {
            let (namespace, ecosystem, name) = key.params();
            query!(
                "SELECT 1 FROM versions WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3
                 AND version = ?4 LIMIT 1",
                namespace,
                ecosystem,
                name,
                version,
            )
        }
    }
}

const WRITE_VERSION_NOTE: DialectSql = DialectSql::with_mysql(
    "INSERT INTO version_notes (namespace, ecosystem, name, version, text, modified_at, principal,
                                initial_digest)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
     ON CONFLICT (namespace, ecosystem, name, version) DO UPDATE SET
       text = excluded.text, modified_at = excluded.modified_at,
       principal = excluded.principal, initial_digest = excluded.initial_digest",
    "INSERT INTO version_notes (namespace, ecosystem, name, version, text, modified_at, principal,
                                initial_digest)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
     AS excluded ON DUPLICATE KEY UPDATE text = excluded.text, modified_at = excluded.modified_at,
       principal = excluded.principal, initial_digest = excluded.initial_digest",
);

const WRITE_PUBLICATION_NOTE: DialectSql = DialectSql::with_mysql(
    "INSERT INTO publication_notes (namespace, publication, text, modified_at, principal,
                                    initial_digest)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
     ON CONFLICT (namespace, publication) DO UPDATE SET
       text = excluded.text, modified_at = excluded.modified_at,
       principal = excluded.principal, initial_digest = excluded.initial_digest",
    "INSERT INTO publication_notes (namespace, publication, text, modified_at, principal,
                                    initial_digest)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
     AS excluded ON DUPLICATE KEY UPDATE text = excluded.text, modified_at = excluded.modified_at,
       principal = excluded.principal, initial_digest = excluded.initial_digest",
);

/// Store a note's text, `None` for a deletion.
fn write_note(
    transaction: &dyn Connection,
    subject: &NoteSubject,
    text: Option<&str>,
    principal: Option<&str>,
    initial_digest: Option<&str>,
) -> Result<()> {
    let now = now_millis();
    let dialect = transaction.dialect();
    match subject {
        NoteSubject::Version { key, version } => {
            let (namespace, ecosystem, name) = key.params();
            transaction.execute(
                WRITE_VERSION_NOTE.get(dialect),
                params![namespace, ecosystem, name, version, text, now, principal, initial_digest],
            )
        }
        NoteSubject::Publication { namespace, id } => transaction.execute(
            WRITE_PUBLICATION_NOTE.get(dialect),
            params![namespace, *id, text, now, principal, initial_digest],
        ),
    }
}

impl super::SqlDatabase {
    /// The note of `version` of `name` in `namespace`.
    pub async fn version_note(
        self: &std::sync::Arc<Self>,
        name: &CanonicalPackageName,
        namespace: &str,
        version: &str,
    ) -> Result<Option<Note>> {
        let subject = NoteSubject::version(&package_key(name, namespace), version);
        self.read(namespace.to_string(), Freshness::Bounded, move |connection, _| {
            read_note(connection, &subject)
        })
        .await
    }

    /// The current versions of `name` in `namespace` whose note holds text or
    /// the digest it was published with, without the text. A note added later
    /// and then deleted holds neither.
    pub async fn noted_versions(
        self: &std::sync::Arc<Self>,
        name: &CanonicalPackageName,
        namespace: &str,
    ) -> Result<Vec<String>> {
        let key = package_key(name, namespace);
        self.read(namespace.to_string(), Freshness::Bounded, move |connection, _| {
            let (namespace, ecosystem, name) = key.params();
            connection
                .query(
                    "SELECT n.version FROM version_notes n
                     WHERE n.namespace = ?1 AND n.ecosystem = ?2 AND n.name = ?3
                       AND (n.text IS NOT NULL OR n.initial_digest IS NOT NULL)
                       AND (n.ecosystem != 'npm' OR EXISTS (
                         SELECT 1 FROM versions v WHERE v.namespace = n.namespace
                           AND v.ecosystem = n.ecosystem AND v.name = n.name
                           AND v.version = n.version))",
                    params![namespace, ecosystem, name],
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
        namespace: &str,
        version: &str,
        change: NoteChange,
    ) -> Result<bool> {
        let subject = NoteSubject::version(&package_key(name, namespace), version);
        self.write(move |connection| {
            change_note(connection, &subject, change.text.as_deref(), &change.principal)
        })
        .await
    }

    /// The note of publication `id` of `namespace`.
    pub async fn publication_note(
        self: &std::sync::Arc<Self>,
        namespace: &str,
        id: u64,
    ) -> Result<Option<Note>> {
        let subject = NoteSubject::publication(namespace, id);
        self.read(namespace.to_string(), Freshness::Bounded, move |connection, _| {
            read_note(connection, &subject)
        })
        .await
    }

    /// Replace the note of publication `id`, or delete it with `None`.
    /// Answers `false` when the publication does not exist.
    pub async fn set_publication_note(
        self: &std::sync::Arc<Self>,
        namespace: &str,
        id: u64,
        change: NoteChange,
    ) -> Result<bool> {
        let subject = NoteSubject::publication(namespace, id);
        self.write(move |connection| {
            change_note(connection, &subject, change.text.as_deref(), &change.principal)
        })
        .await
    }
}

fn package_key(name: &CanonicalPackageName, namespace: &str) -> PackageKey {
    PackageKey {
        namespace: namespace.to_string(),
        ecosystem: name.ecosystem(),
        name: name.as_str().to_string(),
    }
}

impl super::SqlDatabase {
    /// Whether `version` of `name` in `namespace` is accepted and not removed.
    pub async fn has_version(
        self: &std::sync::Arc<Self>,
        name: &CanonicalPackageName,
        namespace: &str,
        version: &str,
    ) -> Result<bool> {
        let (key, version) = (package_key(name, namespace), version.to_string());
        self.read(namespace.to_string(), Freshness::Bounded, move |connection, _| {
            let (namespace, ecosystem, name) = key.params();
            let found = connection.query_optional(
                "SELECT 1 FROM versions
                 WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3 AND version = ?4",
                params![namespace, ecosystem, name, version],
            )?;
            Ok(found.is_some())
        })
        .await
    }

    /// The initial publication of every current version of `name` in
    /// `namespace` whose publication note holds text or its publish-time
    /// digest, as `(version, publication id)`. A removed version's publication
    /// stays recorded but is not listed.
    pub async fn noted_publications(
        self: &std::sync::Arc<Self>,
        name: &CanonicalPackageName,
        namespace: &str,
    ) -> Result<Vec<(String, u64)>> {
        let key = package_key(name, namespace);
        self.read(namespace.to_string(), Freshness::Bounded, move |connection, _| {
            let (namespace, ecosystem, name) = key.params();
            let rows = connection.query(
                "SELECT p.version, p.publication FROM (
                   SELECT m.version, MIN(m.publication) AS publication FROM publication_members m
                   WHERE m.namespace = ?1 AND m.ecosystem = ?2 AND m.name = ?3
                     AND EXISTS (
                       SELECT 1 FROM versions v WHERE v.namespace = m.namespace
                         AND v.ecosystem = m.ecosystem AND v.name = m.name
                         AND v.version = m.version)
                   GROUP BY m.version
                 ) p
                 WHERE EXISTS (
                   SELECT 1 FROM publication_notes n
                   WHERE n.namespace = ?1 AND n.publication = p.publication
                     AND (n.text IS NOT NULL OR n.initial_digest IS NOT NULL))",
                params![namespace, ecosystem, name],
            )?;
            rows.into_iter()
                .map(|mut row| Ok((row.get(0)?, super::to_generation(row.get(1)?))))
                .collect()
        })
        .await
    }
}
