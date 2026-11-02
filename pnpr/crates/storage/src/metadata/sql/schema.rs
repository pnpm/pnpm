//! The metadata database's tables.
//!
//! A package is keyed by `(catalog, ecosystem, name)`: the organization
//! namespace it is hosted in, its ecosystem, and its canonical name. Every key
//! column is `NOT NULL`, so no nullable column can weaken a uniqueness
//! constraint. `revision_key` is `ordinary` for a version outside the registry
//! revision protocol.

use super::{adapter::Connection, batch::WriteTransaction};
use pnpr_error::Result;

/// The layout this pnpr reads. pnpr is pre-1.0: a change to the layout
/// replaces it in place rather than migrating.
pub(super) const SCHEMA_VERSION: i64 = 2;

const TABLES: &str = "
CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL) STRICT;

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

/// Create the tables in a new database, and report the schema version the
/// database holds.
pub(super) fn prepare(connection: &dyn Connection) -> Result<Option<i64>> {
    let transaction = WriteTransaction::begin(connection)?;
    for statement in statements(TABLES) {
        transaction.execute(&statement, params![])?;
    }
    let stored = stored_version(&*transaction)?;
    if stored.is_none() {
        transaction.execute(
            "INSERT INTO schema_version (version) VALUES (?1)",
            params![SCHEMA_VERSION],
        )?;
    }
    transaction.commit()?;
    Ok(stored.or(Some(SCHEMA_VERSION)))
}

/// The schema version stored in the database, `None` in a new one.
fn stored_version(connection: &dyn Connection) -> Result<Option<i64>> {
    connection
        .query_optional("SELECT version FROM schema_version", params![])?
        .map(|mut row| row.get(0))
        .transpose()
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
