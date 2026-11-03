//! The metadata database's tables.
//!
//! A package is keyed by `(catalog, ecosystem, name)`: the organization
//! namespace it is hosted in, its ecosystem, and its canonical name. Every key
//! column is `NOT NULL`, so no nullable column can weaken a uniqueness
//! constraint. `revision_key` is `ordinary` for a version outside the registry
//! revision protocol.

use super::{
    adapter::{Connection, Value},
    batch::WriteTransaction,
};
use pnpr_error::Result;
use std::fmt;

/// The version of the metadata layout in [`TABLES`], stored as text in the
/// [`MARKER`].
///
/// pnpr does not migrate the metadata before 1.0. Until then the version is
/// `0.x`: any change to the metadata layout, a table, column, constraint, or
/// index, bumps its minor part (`0.2`, `0.3`, ...). A database that holds
/// another version is refused, and the operator wipes pnpr's metadata tables
/// by hand so that pnpr creates them afresh. With 1.0 the schema moves to
/// numbered migrations.
pub(super) const SCHEMA_VERSION: &str = "0.1";

/// The table that marks a database as pnpr's metadata. Its one row is the
/// [`SCHEMA_VERSION`] of the layout that created the tables.
pub(super) const MARKER: &str = "schema_version";

/// The statement that creates the [`MARKER`] table.
pub(super) const MARKER_TABLE: &str = "CREATE TABLE schema_version (version TEXT NOT NULL) STRICT";

/// The tables and indexes of the metadata. A change to a statement here,
/// other than to its comments or whitespace, bumps [`SCHEMA_VERSION`].
pub(super) const TABLES: &str = "
-- The current generation of each hosted catalog.
CREATE TABLE IF NOT EXISTS catalogs (
    catalog TEXT NOT NULL PRIMARY KEY,
    generation INTEGER NOT NULL
) STRICT;

-- One row per package. `generation` is the catalog generation of the last
-- commit that changed it. `state` holds what no other table does: for npm,
-- the package-level fields of the packument; for any other ecosystem, its
-- whole document.
CREATE TABLE IF NOT EXISTS packages (
    catalog TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    generation INTEGER NOT NULL,
    state BLOB NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name)
) STRICT, WITHOUT ROWID;

-- Insert-only: a version's artifact integrity. `accepted_generation` is the
-- catalog generation that accepted it, which is the id of the version's
-- initial publication.
CREATE TABLE IF NOT EXISTS versions (
    catalog TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    revision_key TEXT NOT NULL,
    integrity TEXT NOT NULL,
    published_at TEXT,
    accepted_generation INTEGER NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name, version, revision_key),
    FOREIGN KEY (catalog, ecosystem, name)
        REFERENCES packages (catalog, ecosystem, name) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

-- Insert-only: the manifest accepted for a version under an interpretation
-- profile. `inputs` is a JSON object of the digests of the raw inputs the
-- profile derived it from, such as the archive's package.json; it is empty
-- for the `submitted` profile, whose input is the manifest itself.
-- `accepted_digest` covers the manifest, README, profile, and inputs, which a
-- restatement of the version must match.
CREATE TABLE IF NOT EXISTS accepted_manifests (
    catalog TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    revision_key TEXT NOT NULL,
    profile TEXT NOT NULL,
    manifest TEXT NOT NULL,
    inputs TEXT NOT NULL,
    accepted_digest TEXT NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name, version, revision_key, profile),
    FOREIGN KEY (catalog, ecosystem, name, version, revision_key)
        REFERENCES versions (catalog, ecosystem, name, version, revision_key) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

-- Insert-only: the exact bytes of a version's release record, as accepted
-- under `profile` and served unchanged at its record route. `digest` is their
-- SHA-512, `sha512-<base64>`, which the record's log entry commits to, and
-- `log_index` the index of that entry in its catalog's log.
CREATE TABLE IF NOT EXISTS release_records (
    catalog TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    revision_key TEXT NOT NULL,
    profile TEXT NOT NULL,
    record BLOB NOT NULL,
    digest TEXT NOT NULL,
    log_index INTEGER NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name, version, revision_key, profile),
    FOREIGN KEY (catalog, ecosystem, name, version, revision_key, profile)
        REFERENCES accepted_manifests (catalog, ecosystem, name, version, revision_key, profile)
        ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

-- Insert-only: the transparency log of each catalog's records of one
-- ecosystem, one row per entry, sequenced in the transaction that accepts the
-- record. `idx` counts from 0 without gaps. Kept when a version or package is
-- removed: a log only grows.
CREATE TABLE IF NOT EXISTS log_entries (
    catalog TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    idx INTEGER NOT NULL,
    entry BLOB NOT NULL,
    PRIMARY KEY (catalog, ecosystem, idx)
) STRICT, WITHOUT ROWID;

-- The tree each log's tiles and checkpoint were integrated to: its size and
-- its RFC 6962 root. Integration checks the stored tiles against it before it
-- extends a log, so altered tiles are never signed over, and only moves it to
-- a larger tree.
CREATE TABLE IF NOT EXISTS log_heads (
    catalog TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    size INTEGER NOT NULL,
    root BLOB NOT NULL,
    PRIMARY KEY (catalog, ecosystem)
) STRICT, WITHOUT ROWID;

-- Kept out of the manifest so a README never inflates resolution metadata.
CREATE TABLE IF NOT EXISTS readmes (
    catalog TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    revision_key TEXT NOT NULL,
    readme TEXT,
    readme_filename TEXT,
    PRIMARY KEY (catalog, ecosystem, name, version, revision_key),
    FOREIGN KEY (catalog, ecosystem, name, version, revision_key)
        REFERENCES versions (catalog, ecosystem, name, version, revision_key) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS tags (
    catalog TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    tag TEXT NOT NULL,
    version TEXT NOT NULL,
    generation INTEGER NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name, tag),
    FOREIGN KEY (catalog, ecosystem, name)
        REFERENCES packages (catalog, ecosystem, name) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

-- Insert-only: every move of a tag, so a release-age or downgrade policy can
-- see when a tag moved, not only where it points. Kept when the package is
-- removed.
CREATE TABLE IF NOT EXISTS tag_history (
    catalog TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    tag TEXT NOT NULL,
    generation INTEGER NOT NULL,
    previous TEXT,
    target TEXT,
    principal TEXT,
    changed_at TEXT NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name, tag, generation)
) STRICT, WITHOUT ROWID;

-- Insert-only: the identity of every version removed from a package, by
-- unpublishing it or the whole package. Kept when the package is removed, so
-- a removed version is never accepted again and a version and revision always
-- name the same bytes.
CREATE TABLE IF NOT EXISTS removed_versions (
    catalog TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    revision_key TEXT NOT NULL,
    generation INTEGER NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name, version, revision_key)
) STRICT, WITHOUT ROWID;

-- Mutable annotations of a version. `deprecated` is its JSON value.
-- `generation` is the generation that last changed the row.
CREATE TABLE IF NOT EXISTS annotations (
    catalog TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    deprecated TEXT NOT NULL,
    generation INTEGER NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name, version),
    FOREIGN KEY (catalog, ecosystem, name)
        REFERENCES packages (catalog, ecosystem, name) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

-- A commit that introduced artifacts. `id` is the catalog generation it
-- committed. `name` is the publisher's, unique within the catalog.
CREATE TABLE IF NOT EXISTS publications (
    catalog TEXT NOT NULL,
    id INTEGER NOT NULL,
    name TEXT,
    principal TEXT,
    committed_at TEXT NOT NULL,
    PRIMARY KEY (catalog, id)
) STRICT;
-- The versions a commit accepted, which are its publication's members.
CREATE INDEX IF NOT EXISTS versions_by_accepted_generation
    ON versions (catalog, accepted_generation);
CREATE UNIQUE INDEX IF NOT EXISTS publications_by_name
    ON publications (catalog, name) WHERE name IS NOT NULL;

-- Insert-only: each artifact belongs to the publication that introduced it.
-- Kept when the package is removed.
CREATE TABLE IF NOT EXISTS publication_members (
    catalog TEXT NOT NULL,
    publication INTEGER NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    artifact TEXT NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name, version, artifact),
    FOREIGN KEY (catalog, publication) REFERENCES publications (catalog, id)
) STRICT, WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS publication_members_by_publication
    ON publication_members (catalog, publication);

-- The outcome of a committed publication request, keyed by the principal
-- that sent it and its idempotency key, for the request's retries.
CREATE TABLE IF NOT EXISTS publication_outcomes (
    catalog TEXT NOT NULL,
    principal TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    request_digest TEXT NOT NULL,
    generation INTEGER NOT NULL,
    publication INTEGER,
    committed_at INTEGER NOT NULL,
    PRIMARY KEY (catalog, principal, idempotency_key)
) STRICT, WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS publication_outcomes_by_key
    ON publication_outcomes (principal, idempotency_key);

-- Release notes of a package version (`kind` = 'version', `subject` = the
-- version) or of a publication (`kind` = 'publication', `ecosystem` and
-- `name` empty, `subject` = its id). `text` is NULL once deleted.
-- `initial_digest` is the digest of the text published with the subject.
CREATE TABLE IF NOT EXISTS notes (
    catalog TEXT NOT NULL,
    kind TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    subject TEXT NOT NULL,
    text TEXT,
    modified_at TEXT NOT NULL,
    principal TEXT,
    initial_digest TEXT,
    PRIMARY KEY (catalog, kind, ecosystem, name, subject)
) STRICT, WITHOUT ROWID;

-- Insert-only: every change of a note, with the text it replaced, so an
-- accidental or unauthorized edit can be restored.
CREATE TABLE IF NOT EXISTS note_history (
    catalog TEXT NOT NULL,
    kind TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    subject TEXT NOT NULL,
    changed_at TEXT NOT NULL,
    principal TEXT,
    previous TEXT,
    next TEXT
) STRICT;
";

/// What a database holds where pnpr expects its metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Stored {
    /// The marker holds this schema version, as text.
    Version(String),
    /// The marker holds an integer schema version, as an older pnpr's marker
    /// does.
    Integer(i64),
    /// The marker holds anything else, or more or fewer than one row.
    Unreadable,
    /// Metadata tables without a marker.
    Unmarked,
    /// The marker names this pnpr's version, but these metadata tables are
    /// missing, as after a wipe that left the marker.
    Incomplete(Vec<String>),
}

impl fmt::Display for Stored {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Version(version) => write!(f, "schema version {version}"),
            Self::Integer(version) => write!(f, "the integer schema version {version}"),
            Self::Unreadable => f.write_str("an unreadable schema marker"),
            Self::Unmarked => f.write_str("metadata tables without a schema marker"),
            Self::Incomplete(missing) => write!(
                f,
                "schema version {SCHEMA_VERSION} without the metadata tables {}",
                missing.join(", "),
            ),
        }
    }
}

/// Create the schema in an empty database, and report what a database that
/// is not empty holds unless it is this pnpr's schema.
///
/// The database is inspected before anything is created, so a database that
/// is refused is left as it was. A database counts as empty when it has no
/// marker and none of the metadata tables; tables of anything else that
/// shares it, such as pnpr's auth tables, do not count.
pub(super) fn prepare(connection: &dyn Connection) -> Result<Option<Stored>> {
    let transaction = WriteTransaction::begin(connection)?;
    let present = transaction
        .query("SELECT name FROM sqlite_master WHERE type = 'table'", params![])?
        .into_iter()
        .map(|mut row| row.get::<String>(0))
        .collect::<Result<Vec<_>>>()?;
    // `SQLite` matches table names without regard to ASCII case.
    let has = |name: &str| {
        present
            .iter()
            .any(|table| table.eq_ignore_ascii_case(name))
    };
    if has(MARKER) {
        return Ok(match stored(&*transaction)? {
            Stored::Version(stored) if stored == SCHEMA_VERSION => incomplete(has),
            stored => Some(stored),
        });
    }
    if table_names(TABLES).iter().any(|table| has(table)) {
        return Ok(Some(Stored::Unmarked));
    }
    for statement in statements(TABLES) {
        transaction.execute(&statement, params![])?;
    }
    transaction.execute(MARKER_TABLE, params![])?;
    transaction.execute(
        &format!("INSERT INTO {MARKER} (version) VALUES (?1)"),
        params![SCHEMA_VERSION],
    )?;
    transaction.commit()?;
    Ok(None)
}

/// The metadata tables a database marked with this pnpr's version lacks, given
/// whether it `has` each table.
fn incomplete(has: impl Fn(&str) -> bool) -> Option<Stored> {
    let missing = table_names(TABLES)
        .into_iter()
        .filter(|table| !has(table))
        .collect::<Vec<_>>();
    (!missing.is_empty()).then_some(Stored::Incomplete(missing))
}

/// What the marker holds. It is read whole, so a marker of any layout is
/// read rather than failing on a column it lacks.
fn stored(connection: &dyn Connection) -> Result<Stored> {
    let mut rows = connection.query(&format!("SELECT * FROM {MARKER}"), params![])?;
    if rows.len() != 1 {
        return Ok(Stored::Unreadable);
    }
    Ok(match rows.remove(0).get::<Value>(0) {
        Ok(Value::Text(version)) => Stored::Version(version),
        Ok(Value::Integer(version)) => Stored::Integer(version),
        _ => Stored::Unreadable,
    })
}

/// The tables `script` creates, however its statements are spaced or cased.
pub(super) fn table_names(script: &str) -> Vec<String> {
    statements(script)
        .filter_map(|statement| {
            let statement = statement
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            // An ASCII case change keeps every byte offset.
            let upper = statement.to_ascii_uppercase();
            let mut start = "CREATE TABLE ".len();
            if !upper.starts_with("CREATE TABLE ") {
                return None;
            }
            if upper[start..].starts_with("IF NOT EXISTS ") {
                start += "IF NOT EXISTS ".len();
            }
            statement[start..]
                .split(|character: char| character.is_whitespace() || character == '(')
                .next()
                .map(str::to_string)
        })
        .collect()
}

/// The statements of `script`, without its comments. Each runs on its own:
/// a libSQL client re-renders the statements of a batch, and its rendering
/// drops the comma between `STRICT` and `WITHOUT ROWID`.
fn statements(script: &str) -> impl Iterator<Item = String> + '_ {
    let code: String = script
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    code.split(';')
        .map(str::trim)
        .filter(|statement| !statement.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>()
        .into_iter()
}
