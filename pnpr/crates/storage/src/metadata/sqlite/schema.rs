//! The metadata database's tables.
//!
//! A package is keyed by `(catalog, ecosystem, name)`: the organization
//! namespace it is hosted in, its ecosystem, and its canonical name. Every key
//! column is `NOT NULL`, so no nullable column can weaken a uniqueness
//! constraint. `revision_key` is `ordinary` for a version outside the registry
//! revision protocol.

use rusqlite::{Connection, OptionalExtension};

/// The layout this pnpr reads. pnpr is pre-1.0: a change to the layout
/// replaces it in place rather than migrating.
pub(super) const SCHEMA_VERSION: i64 = 1;

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
-- generation that accepted it.
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
-- profile, and the digest of its input.
CREATE TABLE IF NOT EXISTS accepted_manifests (
    catalog TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    revision_key TEXT NOT NULL,
    profile TEXT NOT NULL,
    manifest TEXT NOT NULL,
    input_digest TEXT NOT NULL,
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
";

/// Configure a connection and create the tables it needs.
pub(super) fn prepare(connection: &Connection) -> rusqlite::Result<()> {
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", "FULL")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    connection.execute_batch(TABLES)?;
    if stored_version(connection)?.is_none() {
        connection.execute("INSERT INTO schema_version (version) VALUES (?1)", [SCHEMA_VERSION])?;
    }
    Ok(())
}

/// The schema version stored in the database, `None` in a new one.
pub(super) fn stored_version(connection: &Connection) -> rusqlite::Result<Option<i64>> {
    connection
        .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
        .optional()
}
