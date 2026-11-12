//! Committing a package document: writing only the rows the document
//! changes.

use super::{
    Expected, PackageKey,
    adapter::{Connection, Query, Row},
    batch::Batch,
    cargo_rows::{self, CARGO_STATE_VERSION, StoredCargo},
    oci_rows::{self, OCI_STATE_VERSION, StoredOci},
    packument::{PackumentRows, VersionRows, decompose},
    pypi_rows::{self, PYPI_STATE_VERSION, StoredPypi},
    removed::{tombstone_owners_from, tombstone_owners_query},
    tags::{TagMove, record_tag_move, stored_tags_query, tags_from_rows, validate_packument_tags},
    to_column, to_generation,
};
use crate::{
    DocumentWrite,
    metadata::{ArtifactMember, ReleaseRecords},
    publish::PACKUMENT_FORMAT_VERSION,
};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::Ecosystem;
use pnpr_sql::DialectSql;
use serde_json::{Map, Value};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
};

pub(super) const fn supported_state_version(ecosystem: Ecosystem) -> u32 {
    match ecosystem {
        Ecosystem::Npm => PACKUMENT_FORMAT_VERSION,
        Ecosystem::Cargo => CARGO_STATE_VERSION,
        Ecosystem::Pypi => PYPI_STATE_VERSION,
        Ecosystem::Oci => OCI_STATE_VERSION,
    }
}

/// The stored row of a package.
pub(super) struct PackageRow {
    pub(super) generation: u64,
    pub(super) state_version: u32,
    pub(super) state: Vec<u8>,
}

pub(super) fn package_row_query(key: &PackageKey) -> Query<'_> {
    let (namespace, ecosystem, name) = key.params();
    query!(
        "SELECT generation, state_version, state FROM packages
         WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3",
        namespace,
        ecosystem,
        name,
    )
}

pub(super) fn package_row_from(key: &PackageKey, rows: Vec<Row>) -> Result<Option<PackageRow>> {
    rows.into_iter()
        .next()
        .map(|mut row| {
            let generation = to_generation(row.get(0)?);
            let raw_version: i64 = row.get(1)?;
            let state_version = validate_stored_state_version(key, raw_version)?;
            Ok(PackageRow { generation, state_version, state: row.get(2)? })
        })
        .transpose()
}

fn validate_stored_state_version(key: &PackageKey, raw_version: i64) -> Result<u32> {
    let supported = supported_state_version(key.ecosystem);
    if raw_version > i64::from(supported) {
        return Err(RegistryError::DocumentFormatTooNew {
            document: key.name.clone(),
            format_version: u32::try_from(raw_version).unwrap_or(u32::MAX),
            supported_version: supported,
        });
    }
    if raw_version < 1 {
        return Err(RegistryError::Internal {
            reason: format!("invalid state_version {raw_version} for {}", key.name),
        });
    }
    Ok(u32::try_from(raw_version).unwrap_or(supported))
}

/// What a write of a package's document compares the document with, read
/// before it changes anything.
pub(super) struct StoredPackage {
    row: Option<PackageRow>,
    tombstone_owners: Option<Vec<String>>,
    ecosystem: StoredEcosystem,
}

enum StoredEcosystem {
    Npm {
        versions: BTreeMap<String, StoredVersion>,
        tags: BTreeMap<String, String>,
        removed: BTreeSet<String>,
    },
    Cargo(StoredCargo),
    Pypi(StoredPypi),
    Oci(StoredOci),
}

impl StoredPackage {
    /// The reads of a package of `key`'s ecosystem, for [`Self::from_rows`].
    pub(super) fn queries(key: &PackageKey) -> Vec<Query<'_>> {
        let mut queries = vec![package_row_query(key), tombstone_owners_query(key)];
        match key.ecosystem {
            Ecosystem::Npm => {
                let (namespace, ecosystem, name) = key.params();
                queries.push(query!(
                    "SELECT v.version, m.accepted_digest, a.deprecated
                     FROM versions v
                     JOIN accepted_manifests m USING (namespace, ecosystem, name, version)
                     LEFT JOIN annotations a USING (namespace, ecosystem, name, version)
                     WHERE v.namespace = ?1 AND v.ecosystem = ?2 AND v.name = ?3",
                    namespace,
                    ecosystem,
                    name,
                ));
                queries.push(stored_tags_query(key));
                queries.push(query!(
                    "SELECT version FROM removed_versions
                     WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3",
                    namespace,
                    ecosystem,
                    name,
                ));
            }
            Ecosystem::Cargo => queries.extend(cargo_rows::queries(key)),
            Ecosystem::Pypi => queries.extend(pypi_rows::queries(key)),
            Ecosystem::Oci => queries.extend(oci_rows::queries(key)),
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
        let row = package_row_from(key, next()?)?;
        let tombstone_owners = tombstone_owners_from(next()?)?;
        let ecosystem = match key.ecosystem {
            Ecosystem::Npm => npm_from_rows(&mut next)?,
            Ecosystem::Cargo => StoredEcosystem::Cargo(cargo_rows::from_rows(next()?, next()?)?),
            Ecosystem::Pypi => StoredEcosystem::Pypi(pypi_rows::from_rows(next()?, next()?)?),
            Ecosystem::Oci => StoredEcosystem::Oci(oci_rows::from_rows(next()?, next()?)?),
        };
        Ok(Self { row, tombstone_owners, ecosystem })
    }
}

fn npm_from_rows(next: &mut impl FnMut() -> Result<Vec<Row>>) -> Result<StoredEcosystem> {
    let mut versions = BTreeMap::new();
    for mut row in next()? {
        let deprecated = parse_optional_json(row.get::<Option<String>>(2)?.as_deref())?;
        versions.insert(row.get(0)?, StoredVersion { accepted_digest: row.get(1)?, deprecated });
    }
    let tags = tags_from_rows(next()?)?;
    let removed = next()?
        .into_iter()
        .map(|mut row| row.get(0))
        .collect::<Result<_>>()?;
    Ok(StoredEcosystem::Npm { versions, tags, removed })
}

/// One package's writes within a [`Batch`], under the batch's generation.
pub(super) struct Commit<'batch> {
    pub(super) transaction: &'batch dyn Connection,
    pub(super) key: &'batch PackageKey,
    pub(super) generation: u64,
    /// The npm versions the batch accepted so far.
    pub(super) accepted: &'batch RefCell<Vec<ArtifactMember>>,
}

const WRITE_STATE: DialectSql = DialectSql::with_mysql(
    "INSERT INTO packages (namespace, ecosystem, name, generation, state_version, state)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
     ON CONFLICT (namespace, ecosystem, name) DO UPDATE SET
       state_version = excluded.state_version, state = excluded.state",
    "INSERT INTO packages (namespace, ecosystem, name, generation, state_version, state)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
     AS excluded ON DUPLICATE KEY UPDATE state_version = excluded.state_version, state = excluded.state",
);

impl Commit<'_> {
    pub(super) fn write_state(&self, state_version: u32, state: &[u8]) -> Result<()> {
        let (namespace, ecosystem, name) = self.key.params();
        self.transaction.execute(
            WRITE_STATE.get(self.transaction.dialect()),
            params![
                namespace,
                ecosystem,
                name,
                to_column(self.generation),
                i64::from(state_version),
                state,
            ],
        )?;
        Ok(())
    }

    pub(super) fn write_envelope(
        &self,
        current: Option<&PackageRow>,
        state_version: u32,
        envelope: &impl serde::Serialize,
    ) -> Result<bool> {
        let state = serde_json::to_vec(envelope)?;
        let changed =
            current.is_none_or(|row| row.state_version != state_version || row.state != state);
        if changed {
            self.write_state(state_version, &state)?;
            self.write_owners(current, &state)?;
        }
        Ok(changed)
    }

    pub(super) fn delete_state(&self) -> Result<()> {
        let (namespace, ecosystem, name) = self.key.params();
        for table in ["package_owners", "packages"] {
            self.transaction.execute(
                &format!(
                    "DELETE FROM {table} WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3",
                ),
                params![namespace, ecosystem, name],
            )?;
        }
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
        expected: &Expected,
        principal: Option<&str>,
        records: &ReleaseRecords,
    ) -> Result<Option<bool>> {
        let npm_rows = match self.key.ecosystem {
            Ecosystem::Npm => Some(decompose(bytes)?),
            Ecosystem::Cargo | Ecosystem::Pypi | Ecosystem::Oci => None,
        };
        if !expected.admits(
            stored.row.as_ref().map(|row| row.generation),
            stored.tombstone_owners.as_deref(),
        ) {
            return Ok(None);
        }
        let changed = match stored.ecosystem {
            StoredEcosystem::Npm { versions, tags, removed } => apply_packument(
                self,
                stored.row.as_ref(),
                (versions, tags, removed),
                npm_rows.expect("npm decomposed"),
                principal,
                records,
            )?,
            StoredEcosystem::Cargo(cargo) => {
                cargo_rows::apply(self, stored.row.as_ref(), cargo, bytes, principal)?
            }
            StoredEcosystem::Pypi(pypi) => {
                pypi_rows::apply(self, stored.row.as_ref(), pypi, bytes, principal)?
            }
            StoredEcosystem::Oci(oci) => {
                oci_rows::apply(self, stored.row.as_ref(), oci, bytes, principal)?
            }
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
    expected: &Expected,
    principal: Option<&str>,
) -> Result<DocumentWrite> {
    let (mut batch, results) =
        Batch::begin_with(connection, &key.namespace, &StoredPackage::queries(key))?;
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

fn apply_packument(
    commit: &Commit<'_>,
    current_row: Option<&PackageRow>,
    (stored_versions, stored_tags, removed): (
        BTreeMap<String, StoredVersion>,
        BTreeMap<String, String>,
        BTreeSet<String>,
    ),
    rows: PackumentRows,
    principal: Option<&str>,
    records: &ReleaseRecords,
) -> Result<bool> {
    validate_packument_tags(&stored_tags, &rows)?;
    if stored_versions
        .keys()
        .any(|version| !rows.versions.contains_key(version))
        && let Some(row) = current_row
    {
        super::removed::record_package(commit.transaction, commit.key, &row.state, principal)?;
    }
    let mut changed = if !stored_versions.is_empty() && rows.versions.is_empty() {
        commit.delete_state()?;
        true
    } else {
        commit.write_envelope(current_row, npm_state_version(&rows.state), &rows.state)?
    };
    changed |= apply_versions(commit, stored_versions, &removed, rows.versions, records)?;
    changed |= apply_tags(commit, &stored_tags, &rows.tags, principal)?;
    Ok(changed)
}

fn npm_state_version(state: &Map<String, Value>) -> u32 {
    state
        .get("formatVersion")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|&value| value >= 1 && value <= PACKUMENT_FORMAT_VERSION)
        .unwrap_or(PACKUMENT_FORMAT_VERSION)
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
        super::versions::delete_version(commit, version)?;
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
    let restated = existing.accepted_digest == super::versions::accepted_digest(incoming)? || {
        let accepted = super::versions::accepted_rows(commit, version)?;
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

pub(super) fn parse_json<Parsed: serde::de::DeserializeOwned>(text: &str) -> Result<Parsed> {
    Ok(serde_json::from_str(text)?)
}

pub(super) fn parse_optional_json(text: Option<&str>) -> Result<Option<Value>> {
    text.map(parse_json).transpose()
}
