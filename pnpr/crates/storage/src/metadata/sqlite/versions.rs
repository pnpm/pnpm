//! The insert-only rows of an accepted version, and its deprecation.

use super::{
    commit::Commit,
    packument::{ORDINARY, Readme, SUBMITTED_PROFILE, VersionRows},
    to_column,
};
use pnpr_error::Result;
use rusqlite::params;
use serde_json::Value;

/// Insert the rows of a version the store has not accepted yet.
pub(super) fn insert_version(commit: &Commit<'_>, version: &str, rows: &VersionRows) -> Result<()> {
    let (catalog, ecosystem, name) = commit.key.params();
    let generation = to_column(commit.generation);
    commit.transaction.execute(
        "INSERT INTO versions (catalog, ecosystem, name, version, revision_key, integrity,
                               published_at, accepted_generation)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            catalog,
            ecosystem,
            name,
            version,
            ORDINARY,
            rows.integrity(),
            rows.published_at,
            generation,
        ],
    )?;
    let manifest = serde_json::to_string(&rows.manifest)?;
    let input_digest = super::commit::input_digest(rows)?;
    commit.transaction.execute(
        "INSERT INTO accepted_manifests (catalog, ecosystem, name, version, revision_key, profile,
                                         manifest, input_digest)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            catalog,
            ecosystem,
            name,
            version,
            ORDINARY,
            SUBMITTED_PROFILE,
            manifest,
            input_digest,
        ],
    )?;
    insert_readme(commit, version, &rows.readme)?;
    write_deprecation(commit, version, rows.deprecated.as_ref())
}

fn insert_readme(commit: &Commit<'_>, version: &str, readme: &Readme) -> Result<()> {
    if readme.is_empty() {
        return Ok(());
    }
    let (catalog, ecosystem, name) = commit.key.params();
    commit.transaction.execute(
        "INSERT INTO readmes (catalog, ecosystem, name, version, revision_key, readme,
                              readme_filename)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            catalog,
            ecosystem,
            name,
            version,
            ORDINARY,
            json_text(readme.text.as_ref())?,
            json_text(readme.filename.as_ref())?,
        ],
    )?;
    Ok(())
}

/// Set or clear a version's deprecation under the commit's generation.
pub(super) fn write_deprecation(
    commit: &Commit<'_>,
    version: &str,
    deprecated: Option<&Value>,
) -> Result<()> {
    let (catalog, ecosystem, name) = commit.key.params();
    let Some(deprecated) = deprecated else {
        commit.transaction.execute(
            "DELETE FROM annotations
             WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3 AND version = ?4",
            params![catalog, ecosystem, name, version],
        )?;
        return Ok(());
    };
    commit.transaction.execute(
        "INSERT INTO annotations (catalog, ecosystem, name, version, deprecated, generation)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (catalog, ecosystem, name, version)
         DO UPDATE SET deprecated = excluded.deprecated, generation = excluded.generation",
        params![
            catalog,
            ecosystem,
            name,
            version,
            serde_json::to_string(deprecated)?,
            to_column(commit.generation),
        ],
    )?;
    Ok(())
}

fn json_text(value: Option<&Value>) -> Result<Option<String>> {
    Ok(value.map(serde_json::to_string).transpose()?)
}
