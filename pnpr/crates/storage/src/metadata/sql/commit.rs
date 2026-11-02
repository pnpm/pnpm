//! Committing a package document: writing only the rows the document
//! changes.

use super::{
    Expected, PackageKey,
    adapter::Connection,
    batch::Batch,
    packument::{Interpretation, ORDINARY, PackumentRows, Readme, VersionRows, decompose},
    tags::{TagMove, record_tag_move},
    to_column, to_generation,
};
use crate::DocumentWrite;
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::Ecosystem;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// The stored row of a package.
pub(super) struct PackageRow {
    pub(super) generation: u64,
    pub(super) state: Vec<u8>,
}

pub(super) fn package_row(
    connection: &dyn Connection,
    key: &PackageKey,
) -> Result<Option<PackageRow>> {
    let (catalog, ecosystem, name) = key.params();
    let row = connection.query_optional(
        "SELECT generation, state FROM packages
         WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3",
        params![catalog, ecosystem, name],
    )?;
    row.map(|mut row| Ok(PackageRow { generation: to_generation(row.get(0)?), state: row.get(1)? }))
        .transpose()
}

/// One package's writes within a [`Batch`], under the batch's generation.
pub(super) struct Commit<'batch> {
    pub(super) transaction: &'batch dyn Connection,
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
    /// `expected` names, recording `principal` with every tag it moves.
    /// Reports whether the document changed any row, or `None` on a
    /// compare-and-set conflict.
    pub(super) fn write_document(
        &self,
        bytes: &[u8],
        expected: Expected,
        principal: Option<&str>,
    ) -> Result<Option<bool>> {
        let rows = match self.key.ecosystem {
            Ecosystem::Npm => Some(decompose(bytes)?),
            Ecosystem::Cargo | Ecosystem::Pypi | Ecosystem::Oci => None,
        };
        let current = package_row(self.transaction, self.key)?;
        if !expected.admits(current.as_ref().map(|row| row.generation)) {
            return Ok(None);
        }
        let changed = match rows {
            Some(rows) => apply_packument(self, current.as_ref(), rows, principal)?,
            None => apply_whole_document(self, current.as_ref(), bytes)?,
        };
        Ok(Some(changed))
    }
}

/// Commit `bytes` as the document of `key` in a transaction of its own. A
/// document that changes nothing commits nothing. `principal` is recorded
/// with every tag the document moves.
pub(super) fn write_document(
    connection: &dyn Connection,
    key: &PackageKey,
    bytes: &[u8],
    expected: Expected,
    principal: Option<&str>,
) -> Result<DocumentWrite> {
    let mut batch = Batch::begin(connection, &key.catalog)?;
    let Some(changed) = batch.package(key).write_document(bytes, expected, principal)? else {
        return Ok(DocumentWrite::Conflict);
    };
    if changed {
        batch.touch(key);
        // A version accepted outside a publish request still gets its
        // initial publication, published by whoever wrote the document.
        super::publications::record(&batch, None, principal, &[])?;
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
    principal: Option<&str>,
) -> Result<bool> {
    let state = serde_json::to_vec(&rows.state)?;
    let mut changed = current.is_none_or(|row| row.state != state);
    if changed {
        commit.write_state(&state)?;
    }
    changed |= apply_versions(commit, rows.versions)?;
    changed |= apply_tags(commit, &rows.tags, principal)?;
    Ok(changed)
}

/// What the store holds for one accepted version, short of its manifest.
struct StoredVersion {
    accepted_digest: String,
    deprecated: Option<Value>,
}

/// Each accepted version's digest and deprecation. A version has one
/// accepted manifest, under the profile it was accepted under.
fn stored_versions(commit: &Commit<'_>) -> Result<BTreeMap<String, StoredVersion>> {
    let (catalog, ecosystem, name) = commit.key.params();
    let rows = commit.transaction.query(
        "SELECT v.version, m.accepted_digest, a.deprecated
         FROM versions v
         JOIN accepted_manifests m USING (catalog, ecosystem, name, version, revision_key)
         LEFT JOIN annotations a USING (catalog, ecosystem, name, version)
         WHERE v.catalog = ?1 AND v.ecosystem = ?2 AND v.name = ?3 AND v.revision_key = ?4",
        params![catalog, ecosystem, name, ORDINARY],
    )?;
    let mut stored = BTreeMap::new();
    for mut row in rows {
        let deprecated = parse_optional_json(row.get::<Option<String>>(2)?.as_deref())?;
        stored.insert(row.get(0)?, StoredVersion { accepted_digest: row.get(1)?, deprecated });
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
    let restated = existing.accepted_digest == accepted_digest(incoming)? || {
        let accepted = accepted_rows(commit, version)?;
        accepted.manifest == incoming.manifest
            && accepted.readme == incoming.readme
            && accepted.interpretation == incoming.interpretation
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

/// What a version was accepted as.
struct AcceptedRows {
    manifest: Map<String, Value>,
    readme: Readme,
    interpretation: Interpretation,
}

/// The accepted manifest, README, and interpretation of `version`, for a
/// restatement whose digest differs, such as one with its fields in another
/// order.
fn accepted_rows(commit: &Commit<'_>, version: &str) -> Result<AcceptedRows> {
    let (catalog, ecosystem, name) = commit.key.params();
    let mut row = commit.transaction.query_one(
        "SELECT m.manifest, r.readme, r.readme_filename, m.profile, m.inputs
         FROM accepted_manifests m
         LEFT JOIN readmes r USING (catalog, ecosystem, name, version, revision_key)
         WHERE m.catalog = ?1 AND m.ecosystem = ?2 AND m.name = ?3 AND m.version = ?4
           AND m.revision_key = ?5",
        params![catalog, ecosystem, name, version, ORDINARY],
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

/// Remove a version, keeping its identity so it is never accepted again.
fn delete_version(commit: &Commit<'_>, version: &str) -> Result<()> {
    let (catalog, ecosystem, name) = commit.key.params();
    commit.transaction.execute(
        "INSERT INTO removed_versions (catalog, ecosystem, name, version, revision_key, generation)
         SELECT catalog, ecosystem, name, version, revision_key, ?5 FROM versions
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
/// reference.
const VERSION_TABLES: [&str; 4] = ["accepted_manifests", "readmes", "annotations", "versions"];

/// The tables holding a package's rows other than its history, each before
/// the tables its rows reference.
const PACKAGE_TABLES: [&str; 6] =
    ["accepted_manifests", "readmes", "annotations", "versions", "tags", "packages"];

/// Move every tag the document points elsewhere, recording each move with
/// `principal`.
fn apply_tags(
    commit: &Commit<'_>,
    incoming: &BTreeMap<String, String>,
    principal: Option<&str>,
) -> Result<bool> {
    let stored = super::tags::stored_tags(commit.transaction, commit.key)?;
    let mut changed = false;
    for (tag, target) in incoming {
        let previous = stored.get(tag).map(String::as_str);
        if previous != Some(target.as_str()) {
            let change = TagMove { tag, previous, target: Some(target), principal };
            record_tag_move(commit, &change)?;
            changed = true;
        }
    }
    for (tag, previous) in &stored {
        if !incoming.contains_key(tag) {
            let change = TagMove { tag, previous: Some(previous), target: None, principal };
            record_tag_move(commit, &change)?;
            changed = true;
        }
    }
    Ok(changed)
}

/// Remove every row of the package except its tag history and the identities
/// of its versions, which stay removed, under a new generation.
pub(super) fn remove_package(connection: &dyn Connection, key: &PackageKey) -> Result<bool> {
    let mut batch = Batch::begin(connection, &key.catalog)?;
    let (catalog, ecosystem, name) = key.params();
    batch.transaction().execute(
        "INSERT INTO removed_versions (catalog, ecosystem, name, version, revision_key, generation)
         SELECT catalog, ecosystem, name, version, revision_key, ?4 FROM versions
         WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3
         ON CONFLICT DO NOTHING",
        params![catalog, ecosystem, name, to_column(batch.generation())],
    )?;
    let mut removed = 0;
    for table in PACKAGE_TABLES {
        removed = batch
            .transaction()
            .execute(
                &format!("DELETE FROM {table} WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3"),
                params![catalog, ecosystem, name],
            )?;
    }
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
