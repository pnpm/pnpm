//! Committing a package document: writing only the rows the document
//! changes.

use super::{
    Expected, PackageKey,
    adapter::{Connection, Query, Row},
    batch::Batch,
    packument::{Interpretation, ORDINARY, PackumentRows, Readme, VersionRows, decompose},
    tags::{TagMove, record_tag_move, stored_tags_query, tags_from_rows},
    to_column, to_generation,
};
use crate::{
    BlobId, DocumentWrite,
    metadata::{ArtifactMember, ReleaseRecords},
};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::Ecosystem;
use serde_json::{Map, Value};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
};

/// The stored row of a package.
pub(super) struct PackageRow {
    pub(super) generation: u64,
    pub(super) state: Vec<u8>,
}

pub(super) fn package_row_query(key: &PackageKey) -> Query<'_> {
    let (catalog, ecosystem, name) = key.params();
    query!(
        "SELECT generation, state FROM packages
         WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3",
        catalog,
        ecosystem,
        name,
    )
}

pub(super) fn package_row_from(rows: Vec<Row>) -> Result<Option<PackageRow>> {
    rows.into_iter()
        .next()
        .map(|mut row| {
            Ok(PackageRow { generation: to_generation(row.get(0)?), state: row.get(1)? })
        })
        .transpose()
}

/// What a write of a package's document compares the document with, read
/// before it changes anything.
pub(super) struct StoredPackage {
    row: Option<PackageRow>,
    /// Each accepted version's digest and deprecation.
    versions: BTreeMap<String, StoredVersion>,
    tags: BTreeMap<String, String>,
    /// The versions removed from the package, never to be accepted again.
    removed: BTreeSet<String>,
}

impl StoredPackage {
    /// The reads of a package of `key`'s ecosystem, for [`Self::from_rows`].
    pub(super) fn queries(key: &PackageKey) -> Vec<Query<'_>> {
        let mut queries = vec![package_row_query(key)];
        if key.ecosystem == Ecosystem::Npm {
            let (catalog, ecosystem, name) = key.params();
            queries.push(query!(
                "SELECT v.version, m.accepted_digest, a.deprecated
                 FROM versions v
                 JOIN accepted_manifests m USING (catalog, ecosystem, name, version, revision_key)
                 LEFT JOIN annotations a USING (catalog, ecosystem, name, version)
                 WHERE v.catalog = ?1 AND v.ecosystem = ?2 AND v.name = ?3
                   AND v.revision_key = ?4",
                catalog,
                ecosystem,
                name,
                ORDINARY,
            ));
            queries.push(stored_tags_query(key));
            queries.push(query!(
                "SELECT version FROM removed_versions
                 WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3 AND revision_key = ?4",
                catalog,
                ecosystem,
                name,
                ORDINARY,
            ));
        }
        queries
    }

    /// The package from the rows of [`Self::queries`], taken from `results`.
    pub(super) fn from_rows(
        key: &PackageKey,
        results: &mut impl Iterator<Item = Vec<Row>>,
    ) -> Result<Self> {
        let mut next = || {
            results
                .next()
                .ok_or_else(|| RegistryError::Internal {
                    reason: "a metadata batch did not answer every read of a package".to_string(),
                })
        };
        let row = package_row_from(next()?)?;
        let mut stored = Self {
            row,
            versions: BTreeMap::new(),
            tags: BTreeMap::new(),
            removed: BTreeSet::new(),
        };
        if key.ecosystem != Ecosystem::Npm {
            return Ok(stored);
        }
        for mut row in next()? {
            let deprecated = parse_optional_json(row.get::<Option<String>>(2)?.as_deref())?;
            stored.versions.insert(
                row.get(0)?,
                StoredVersion { accepted_digest: row.get(1)?, deprecated },
            );
        }
        stored.tags = tags_from_rows(next()?)?;
        stored.removed = next()?
            .into_iter()
            .map(|mut row| row.get(0))
            .collect::<Result<_>>()?;
        Ok(stored)
    }
}

/// One package's writes within a [`Batch`], under the batch's generation.
pub(super) struct Commit<'batch> {
    pub(super) transaction: &'batch dyn Connection,
    pub(super) key: &'batch PackageKey,
    pub(super) generation: u64,
    /// The npm versions the batch accepted so far.
    pub(super) accepted: &'batch RefCell<Vec<ArtifactMember>>,
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

    /// Commit `bytes` as the package's document when the stored one, read
    /// as `stored`, is what `expected` names, recording `principal` with
    /// every tag it moves. Reports whether the document changed any row, or
    /// `None` on a compare-and-set conflict.
    pub(super) fn write_document(
        &self,
        stored: StoredPackage,
        bytes: &[u8],
        expected: Expected,
        principal: Option<&str>,
        records: &ReleaseRecords,
    ) -> Result<Option<bool>> {
        let rows = match self.key.ecosystem {
            Ecosystem::Npm => Some(decompose(bytes)?),
            Ecosystem::Cargo | Ecosystem::Pypi | Ecosystem::Oci => None,
        };
        if !expected.admits(stored.row.as_ref().map(|row| row.generation)) {
            return Ok(None);
        }
        let changed = match rows {
            Some(rows) => apply_packument(self, stored, rows, principal, records)?,
            None => apply_whole_document(self, stored.row.as_ref(), bytes)?,
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
    let (mut batch, results) =
        Batch::begin_with(connection, &key.catalog, &StoredPackage::queries(key))?;
    let stored = StoredPackage::from_rows(key, &mut results.into_iter())?;
    let records = ReleaseRecords::new();
    let commit = batch.package(key);
    let Some(changed) = commit.write_document(stored, bytes, expected, principal, &records)? else {
        return Ok(DocumentWrite::Conflict);
    };
    if changed {
        batch.touch(key);
        // A version accepted outside a publish request still gets its
        // initial publication, published by whoever wrote the document.
        super::publications::record(&batch, None, principal, &[], false)?;
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
    stored: StoredPackage,
    rows: PackumentRows,
    principal: Option<&str>,
    records: &ReleaseRecords,
) -> Result<bool> {
    let state = serde_json::to_vec(&rows.state)?;
    let mut changed = stored.row
        .as_ref()
        .is_none_or(|row| row.state != state);
    if changed {
        commit.write_state(&state)?;
    }
    changed |= apply_versions(commit, stored.versions, &stored.removed, rows.versions, records)?;
    changed |= apply_tags(commit, &stored.tags, &rows.tags, principal)?;
    Ok(changed)
}

/// What the store holds for one accepted version, short of its manifest.
struct StoredVersion {
    accepted_digest: String,
    deprecated: Option<Value>,
}

/// Accept new versions, each with its release record when `records` has
/// one, refuse a changed one, apply deprecations, and drop the versions the
/// document no longer lists.
fn apply_versions(
    commit: &Commit<'_>,
    mut stored: BTreeMap<String, StoredVersion>,
    removed: &BTreeSet<String>,
    versions: BTreeMap<String, VersionRows>,
    records: &ReleaseRecords,
) -> Result<bool> {
    let mut changed = false;
    for (version, incoming) in versions {
        match stored.remove(&version) {
            None => {
                let record = records.get(&version).map(Vec::as_slice);
                super::versions::insert_version(commit, removed, &version, &incoming, record)?;
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

/// The tables holding a package's rows other than its history, each before
/// the tables its rows reference.
const PACKAGE_TABLES: [&str; 8] = [
    "artifact_blobs",
    "release_records",
    "accepted_manifests",
    "readmes",
    "annotations",
    "versions",
    "tags",
    "packages",
];

/// Move every tag the document points elsewhere, recording each move with
/// `principal`.
fn apply_tags(
    commit: &Commit<'_>,
    stored: &BTreeMap<String, String>,
    incoming: &BTreeMap<String, String>,
    principal: Option<&str>,
) -> Result<bool> {
    let mut changed = false;
    for (tag, target) in incoming {
        let previous = stored.get(tag).map(String::as_str);
        if previous != Some(target.as_str()) {
            let change = TagMove { tag, previous, target: Some(target), principal };
            record_tag_move(commit, &change)?;
            changed = true;
        }
    }
    for (tag, previous) in stored {
        if !incoming.contains_key(tag) {
            let change = TagMove { tag, previous: Some(previous), target: None, principal };
            record_tag_move(commit, &change)?;
            changed = true;
        }
    }
    Ok(changed)
}

/// Remove every row of the package except its tag history and the identities
/// of its versions, which stay removed, under a new generation. Returns the
/// blobs its files were mapped to, `None` when there was no package.
pub(super) fn remove_package(
    connection: &dyn Connection,
    key: &PackageKey,
) -> Result<Option<Vec<BlobId>>> {
    let (catalog, ecosystem, name) = key.params();
    let mapped = query!(
        "SELECT DISTINCT integrity FROM artifact_blobs
         WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3",
        catalog,
        ecosystem,
        name,
    );
    let (mut batch, results) =
        Batch::begin_with(connection, &key.catalog, &[package_row_query(key), mapped])?;
    let mut results = results.into_iter();
    if results.next().is_none_or(|rows| rows.is_empty()) {
        return Ok(None);
    }
    let ids = super::blobs::ids_from_rows(results.next().unwrap_or_default())?;
    batch.transaction().execute(
        "INSERT INTO removed_versions (catalog, ecosystem, name, version, revision_key, generation)
         SELECT catalog, ecosystem, name, version, revision_key, ?4 FROM versions
         WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3
         ON CONFLICT DO NOTHING",
        params![catalog, ecosystem, name, to_column(batch.generation())],
    )?;
    for table in PACKAGE_TABLES {
        batch
            .transaction()
            .execute(
                &format!("DELETE FROM {table} WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3"),
                params![catalog, ecosystem, name],
            )?;
    }
    batch.touch(key);
    batch.finish()?;
    Ok(Some(ids))
}

pub(super) fn parse_json<Parsed: serde::de::DeserializeOwned>(text: &str) -> Result<Parsed> {
    Ok(serde_json::from_str(text)?)
}

pub(super) fn parse_optional_json(text: Option<&str>) -> Result<Option<Value>> {
    text.map(parse_json).transpose()
}
