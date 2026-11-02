//! Committing a package document: writing only the rows the document
//! changes.

use super::{
    Expected, PackageKey,
    batch::Batch,
    packument::{ORDINARY, PackumentRows, Readme, SUBMITTED_PROFILE, VersionRows, decompose},
    tags::{TagMove, record_tag_move},
    to_column, to_generation,
};
use crate::DocumentWrite;
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::Ecosystem;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// The stored row of a package.
pub(super) struct PackageRow {
    pub(super) generation: u64,
    pub(super) state: Vec<u8>,
}

pub(super) fn package_row(
    connection: &Connection,
    key: &PackageKey,
) -> rusqlite::Result<Option<PackageRow>> {
    connection
        .query_row(
            "SELECT generation, state FROM packages
             WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3",
            key.params(),
            |row| Ok(PackageRow { generation: to_generation(row.get(0)?), state: row.get(1)? }),
        )
        .optional()
}

/// One package's writes within a [`Batch`], under the batch's generation.
pub(super) struct Commit<'batch> {
    pub(super) transaction: &'batch Transaction<'batch>,
    pub(super) key: &'batch PackageKey,
    pub(super) generation: u64,
}

impl Commit<'_> {
    pub(super) fn write_state(&self, state: &[u8]) -> Result<()> {
        let (catalog, ecosystem, name) = self.key.params();
        self.transaction.execute(
            "INSERT INTO packages (catalog, ecosystem, name, generation, state)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (catalog, ecosystem, name) DO UPDATE SET state = excluded.state",
            params![catalog, ecosystem, name, to_column(self.generation), state],
        )?;
        Ok(())
    }

    /// Commit `bytes` as the package's document when the stored one is what
    /// `expected` names. Reports whether the document changed any row, or
    /// `None` on a compare-and-set conflict.
    pub(super) fn write_document(&self, bytes: &[u8], expected: Expected) -> Result<Option<bool>> {
        let rows = match self.key.ecosystem {
            Ecosystem::Npm => Some(decompose(bytes)?),
            Ecosystem::Cargo | Ecosystem::Pypi | Ecosystem::Oci => None,
        };
        let current = package_row(self.transaction, self.key)?;
        if !expected.admits(current.as_ref().map(|row| row.generation)) {
            return Ok(None);
        }
        let changed = match rows {
            Some(rows) => apply_packument(self, current.as_ref(), rows)?,
            None => apply_whole_document(self, current.as_ref(), bytes)?,
        };
        Ok(Some(changed))
    }
}

/// Commit `bytes` as the document of `key` in a transaction of its own. A
/// document that changes nothing commits nothing.
pub(super) fn write_document(
    connection: &mut Connection,
    key: &PackageKey,
    bytes: &[u8],
    expected: Expected,
) -> Result<DocumentWrite> {
    let mut batch = Batch::begin(connection, &key.catalog)?;
    let Some(changed) = batch.package(key).write_document(bytes, expected)? else {
        return Ok(DocumentWrite::Conflict);
    };
    if changed {
        batch.touch(key);
    }
    batch.finish()?;
    Ok(DocumentWrite::Written)
}

fn apply_whole_document(
    commit: &Commit<'_>,
    current: Option<&PackageRow>,
    bytes: &[u8],
) -> Result<bool> {
    if current.is_some_and(|row| row.state == bytes) {
        return Ok(false);
    }
    commit.write_state(bytes)?;
    Ok(true)
}

fn apply_packument(
    commit: &Commit<'_>,
    current: Option<&PackageRow>,
    rows: PackumentRows,
) -> Result<bool> {
    let state = serde_json::to_vec(&rows.state)?;
    let mut changed = current.is_none_or(|row| row.state != state);
    if changed {
        commit.write_state(&state)?;
    }
    changed |= apply_versions(commit, rows.versions)?;
    changed |= apply_tags(commit, &rows.tags)?;
    Ok(changed)
}

/// What the store holds for one accepted version, short of its manifest.
struct StoredVersion {
    input_digest: String,
    deprecated: Option<Value>,
}

fn stored_versions(commit: &Commit<'_>) -> Result<BTreeMap<String, StoredVersion>> {
    let mut statement = commit.transaction.prepare_cached(
        "SELECT v.version, m.input_digest, a.deprecated
         FROM versions v
         JOIN accepted_manifests m USING (catalog, ecosystem, name, version, revision_key)
         LEFT JOIN annotations a USING (catalog, ecosystem, name, version)
         WHERE v.catalog = ?1 AND v.ecosystem = ?2 AND v.name = ?3
           AND v.revision_key = ?4 AND m.profile = ?5",
    )?;
    let (catalog, ecosystem, name) = commit.key.params();
    let mut rows = statement.query(params![catalog, ecosystem, name, ORDINARY, SUBMITTED_PROFILE])?;
    let mut stored = BTreeMap::new();
    while let Some(row) = rows.next()? {
        let deprecated = parse_optional_json(row.get::<_, Option<String>>(2)?.as_deref())?;
        stored.insert(row.get(0)?, StoredVersion { input_digest: row.get(1)?, deprecated });
    }
    Ok(stored)
}

/// Accept new versions, refuse a changed one, apply deprecations, and drop
/// the versions the document no longer lists.
fn apply_versions(commit: &Commit<'_>, versions: BTreeMap<String, VersionRows>) -> Result<bool> {
    let mut stored = stored_versions(commit)?;
    let mut changed = false;
    for (version, incoming) in versions {
        match stored.remove(&version) {
            None => {
                super::versions::insert_version(commit, &version, &incoming)?;
                changed = true;
            }
            Some(existing) => {
                changed |= reconcile_version(commit, &version, &existing, &incoming)?;
            }
        }
    }
    for version in stored.keys() {
        delete_version(commit, version)?;
        changed = true;
    }
    Ok(changed)
}

/// Check that `incoming` restates the accepted version, and apply its
/// deprecation.
fn reconcile_version(
    commit: &Commit<'_>,
    version: &str,
    existing: &StoredVersion,
    incoming: &VersionRows,
) -> Result<bool> {
    let restated = existing.input_digest == input_digest(incoming)? || {
        let (manifest, readme) = accepted_rows(commit, version)?;
        manifest == incoming.manifest && readme == incoming.readme
    };
    if !restated {
        return Err(RegistryError::VersionAlreadyPublished {
            package: commit.key.name.clone(),
            version: version.to_string(),
        });
    }
    if existing.deprecated == incoming.deprecated {
        return Ok(false);
    }
    super::versions::write_deprecation(commit, version, incoming.deprecated.as_ref())?;
    Ok(true)
}

/// The accepted manifest and README of `version`, for a restatement whose
/// digest differs, such as one with its fields in another order.
fn accepted_rows(commit: &Commit<'_>, version: &str) -> Result<(Map<String, Value>, Readme)> {
    let (catalog, ecosystem, name) = commit.key.params();
    let (manifest, text, filename) = commit.transaction.query_row(
        "SELECT m.manifest, r.readme, r.readme_filename
         FROM accepted_manifests m
         LEFT JOIN readmes r USING (catalog, ecosystem, name, version, revision_key)
         WHERE m.catalog = ?1 AND m.ecosystem = ?2 AND m.name = ?3 AND m.version = ?4
           AND m.revision_key = ?5 AND m.profile = ?6",
        params![catalog, ecosystem, name, version, ORDINARY, SUBMITTED_PROFILE],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        },
    )?;
    let readme = Readme {
        text: parse_optional_json(text.as_deref())?,
        filename: parse_optional_json(filename.as_deref())?,
    };
    Ok((parse_json(&manifest)?, readme))
}

/// The digest of what a version's insert-only rows hold: its manifest and
/// README.
pub(super) fn input_digest(rows: &VersionRows) -> Result<String> {
    let input = serde_json::to_vec(&(&rows.manifest, &rows.readme.text, &rows.readme.filename))?;
    Ok(ssri::Integrity::from(input).to_string())
}

fn delete_version(commit: &Commit<'_>, version: &str) -> Result<()> {
    let (catalog, ecosystem, name) = commit.key.params();
    commit.transaction.execute(
        "DELETE FROM versions WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3 AND version = ?4",
        params![catalog, ecosystem, name, version],
    )?;
    super::versions::write_deprecation(commit, version, None)?;
    Ok(())
}

/// Move every tag the document points elsewhere, recording each move.
fn apply_tags(commit: &Commit<'_>, incoming: &BTreeMap<String, String>) -> Result<bool> {
    let stored = super::tags::stored_tags(commit.transaction, commit.key)?;
    let mut changed = false;
    for (tag, target) in incoming {
        let previous = stored.get(tag).map(String::as_str);
        if previous != Some(target.as_str()) {
            let change = TagMove { tag, previous, target: Some(target), principal: None };
            record_tag_move(commit, &change)?;
            changed = true;
        }
    }
    for (tag, previous) in &stored {
        if !incoming.contains_key(tag) {
            let change = TagMove { tag, previous: Some(previous), target: None, principal: None };
            record_tag_move(commit, &change)?;
            changed = true;
        }
    }
    Ok(changed)
}

/// Remove every row of the package except its tag history, under a new
/// generation.
pub(super) fn remove_package(connection: &mut Connection, key: &PackageKey) -> Result<bool> {
    let mut batch = Batch::begin(connection, &key.catalog)?;
    let (catalog, ecosystem, name) = key.params();
    let removed = batch
        .transaction()
        .execute(
            "DELETE FROM packages WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3",
            params![catalog, ecosystem, name],
        )?;
    if removed == 0 {
        return Ok(false);
    }
    batch.touch(key);
    batch.finish()?;
    Ok(true)
}

pub(super) fn parse_json<Parsed: serde::de::DeserializeOwned>(text: &str) -> Result<Parsed> {
    Ok(serde_json::from_str(text)?)
}

pub(super) fn parse_optional_json(text: Option<&str>) -> Result<Option<Value>> {
    text.map(parse_json).transpose()
}
