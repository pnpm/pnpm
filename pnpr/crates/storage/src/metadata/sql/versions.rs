//! The insert-only rows of an accepted version, and its deprecation.

use super::{
    PackageKey,
    adapter::Connection,
    commit::{Commit, parse_json, parse_optional_json},
    packument::{Interpretation, Readme, VersionRows},
    to_column,
};
use crate::metadata::ArtifactMember;
use pnpm_release_record::{Purl, RecordIdentity, StoredRecord};
use pnpr_error::{RegistryError, Result};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

/// Insert the rows of a version the store has not accepted yet, with the
/// release record it is accepted with, if any, unless it is one of the
/// `removed` versions of the package.
pub(super) fn insert_version(
    commit: &Commit<'_>,
    removed: &BTreeSet<String>,
    version: &str,
    rows: &VersionRows,
    record: Option<&[u8]>,
) -> Result<()> {
    refuse_removed(commit, removed, version)?;
    let (catalog, ecosystem, name) = commit.key.params();
    commit.transaction.execute(
        "INSERT INTO versions (catalog, ecosystem, name, version, integrity,
                               published_at, accepted_generation)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            catalog,
            ecosystem,
            name,
            version,
            rows.integrity(),
            rows.published_at,
            to_column(commit.generation),
        ],
    )?;
    insert_accepted_manifest(commit, version, rows)?;
    insert_readme(commit, version, &rows.readme)?;
    if let Some(record) = record {
        insert_release_record(commit, version, &rows.interpretation.profile, record)?;
    }
    // A new version has no annotation to clear.
    if let Some(deprecated) = &rows.deprecated {
        write_deprecation(commit, version, Some(deprecated))?;
    }
    commit.accepted
        .borrow_mut()
        .push(ArtifactMember {
            ecosystem: commit.key.ecosystem,
            name: commit.key.name.clone(),
            version: version.to_string(),
            artifact: rows.integrity().to_string(),
        });
    Ok(())
}

fn insert_accepted_manifest(commit: &Commit<'_>, version: &str, rows: &VersionRows) -> Result<()> {
    let (catalog, ecosystem, name) = commit.key.params();
    let manifest = serde_json::to_string(&rows.manifest)?;
    let inputs = serde_json::to_string(&rows.interpretation.inputs)?;
    let accepted_digest = accepted_digest(rows)?;
    commit.transaction.execute(
        "INSERT INTO accepted_manifests (catalog, ecosystem, name, version, profile,
                                         manifest, inputs, accepted_digest)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            catalog,
            ecosystem,
            name,
            version,
            rows.interpretation.profile,
            manifest,
            inputs,
            accepted_digest,
        ],
    )?;
    Ok(())
}

/// What a version was accepted as.
pub(super) struct AcceptedRows {
    pub(super) manifest: Map<String, Value>,
    pub(super) readme: Readme,
    pub(super) interpretation: Interpretation,
}

/// The accepted manifest, README, and interpretation of `version`, for a
/// restatement whose digest differs, such as one with its fields in another
/// order.
pub(super) fn accepted_rows(commit: &Commit<'_>, version: &str) -> Result<AcceptedRows> {
    let (catalog, ecosystem, name) = commit.key.params();
    let mut row = commit.transaction.query_one(
        "SELECT m.manifest, r.readme, r.readme_filename, m.profile, m.inputs
         FROM accepted_manifests m
         LEFT JOIN readmes r USING (catalog, ecosystem, name, version)
         WHERE m.catalog = ?1 AND m.ecosystem = ?2 AND m.name = ?3 AND m.version = ?4",
        params![catalog, ecosystem, name, version],
    )?;
    let manifest: String = row.get(0)?;
    let text: Option<String> = row.get(1)?;
    let filename: Option<String> = row.get(2)?;
    let profile: String = row.get(3)?;
    let inputs: String = row.get(4)?;
    let readme = Readme {
        text: parse_optional_json(text.as_deref())?,
        filename: parse_optional_json(filename.as_deref())?,
    };
    let interpretation = Interpretation { profile, inputs: parse_json(&inputs)? };
    Ok(AcceptedRows { manifest: parse_json(&manifest)?, readme, interpretation })
}

/// The digest of what a version's insert-only rows hold: its manifest,
/// README, and interpretation.
pub(super) fn accepted_digest(rows: &VersionRows) -> Result<String> {
    let input = serde_json::to_vec(&(
        &rows.manifest,
        &rows.readme.text,
        &rows.readme.filename,
        &rows.interpretation,
    ))?;
    Ok(ssri::Integrity::from(input).to_string())
}

/// Insert the exact bytes of a version's release record. They must be a
/// record of this version, revision 0, under the profile its manifest was
/// accepted under, so a record never describes another version than the one
/// it is served for.
fn insert_release_record(
    commit: &Commit<'_>,
    version: &str,
    profile: &str,
    bytes: &[u8],
) -> Result<()> {
    let (catalog, ecosystem, name) = commit.key.params();
    let invalid = |reason: String| RegistryError::Internal {
        reason: format!("the release record of {name}@{version} {reason}"),
    };
    let record =
        StoredRecord::parse(bytes.to_vec()).map_err(|err| invalid(format!("is invalid: {err}")))?;
    let purl = Purl::npm(name, version).map_err(|err| invalid(err.to_string()))?;
    let key = record.key();
    let describes_version = key.identity == RecordIdentity::Package(purl)
        && key.revision == Some(0)
        && key.profile.as_str() == profile;
    if !describes_version {
        return Err(invalid(format!("describes {}", key.subject_name())));
    }
    let entry = record
        .log_entry()
        .map_err(|err| invalid(err.to_string()))?
        .to_bytes();
    sequence_log_entry(commit, &entry)?;
    // The record keeps the index its entry was just sequenced at, the log's
    // last.
    commit.transaction.execute(
        "INSERT INTO release_records (catalog, ecosystem, name, version, profile,
                                      record, digest, log_index)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7,
                 (SELECT MAX(idx) FROM log_entries WHERE catalog = ?1 AND ecosystem = ?2))",
        params![
            catalog,
            ecosystem,
            name,
            version,
            profile,
            record.bytes(),
            record.digest().to_string(),
        ],
    )?;
    Ok(())
}

/// Append `entry` to the log of the commit's catalog and ecosystem, at the
/// next index. The commit holds the database's write lock, so no other
/// commit takes the same index. The index is computed by the statement
/// itself, so sequencing waits for no answer and adds no round trip to a
/// remote primary.
fn sequence_log_entry(commit: &Commit<'_>, entry: &[u8]) -> Result<()> {
    let (catalog, ecosystem, _) = commit.key.params();
    commit.transaction.execute(
        "INSERT INTO log_entries (catalog, ecosystem, idx, entry)
         SELECT ?1, ?2, COALESCE(MAX(idx) + 1, 0), ?3 FROM log_entries
         WHERE catalog = ?1 AND ecosystem = ?2",
        params![catalog, ecosystem, entry],
    )?;
    Ok(())
}

/// The exact bytes of the release record `version` was accepted with.
pub(super) fn read_release_record(
    connection: &dyn Connection,
    key: &PackageKey,
    version: &str,
) -> Result<Option<Vec<u8>>> {
    let (catalog, ecosystem, name) = key.params();
    let row = connection.query_optional(
        "SELECT record FROM release_records
         WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3 AND version = ?4",
        params![catalog, ecosystem, name, version],
    )?;
    row.map(|mut row| row.get(0))
        .transpose()
}

/// Refuse a version removed from the package before: its identity named
/// other bytes once, and a cached fragment may still hold them.
fn refuse_removed(commit: &Commit<'_>, removed: &BTreeSet<String>, version: &str) -> Result<()> {
    if removed.contains(version) {
        return Err(RegistryError::CannotRepublishUnpublishedVersion {
            package: commit.key.name.clone(),
            version: version.to_string(),
        });
    }
    Ok(())
}

fn insert_readme(commit: &Commit<'_>, version: &str, readme: &Readme) -> Result<()> {
    if readme.is_empty() {
        return Ok(());
    }
    let (catalog, ecosystem, name) = commit.key.params();
    commit.transaction.execute(
        "INSERT INTO readmes (catalog, ecosystem, name, version, readme, readme_filename)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            catalog,
            ecosystem,
            name,
            version,
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

/// Remove a version, keeping its identity so it is never accepted again.
pub(super) fn delete_version(commit: &Commit<'_>, version: &str) -> Result<()> {
    let (catalog, ecosystem, name) = commit.key.params();
    commit.transaction.execute(
        "INSERT INTO removed_versions (catalog, ecosystem, name, version, generation)
         SELECT catalog, ecosystem, name, version, ?5 FROM versions
         WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3 AND version = ?4
         ON CONFLICT DO NOTHING",
        params![catalog, ecosystem, name, version, to_column(commit.generation)],
    )?;
    for table in VERSION_TABLES {
        commit.transaction.execute(
            &format!(
                "DELETE FROM {table}
                 WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3 AND version = ?4",
            ),
            params![catalog, ecosystem, name, version],
        )?;
    }
    Ok(())
}

/// The tables holding a version's rows, each before the tables its rows
/// reference. The mappings of its files go with it, which leaves the blobs
/// nothing else maps to be reclaimed.
const VERSION_TABLES: [&str; 6] = [
    "artifact_blobs",
    "release_records",
    "accepted_manifests",
    "readmes",
    "annotations",
    "versions",
];
