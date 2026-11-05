//! The rows of the hosted store's blobs and of the files mapped to them (see
//! [`crate::metadata::BlobRow`]), as a publish reads them and a commit
//! checks and writes them.

use super::{
    PackageKey,
    adapter::{Connection, Query, Row},
    to_column, to_generation,
};
use crate::{
    BlobId,
    metadata::{ArtifactBlob, BlobFence, BlobLookup, BlobRow, BlobState, MappedArtifact},
};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::Ecosystem;
use std::collections::BTreeMap;

/// The current time in seconds since the Unix epoch, as the database tells
/// it, so every instance reads one clock.
pub(super) const NOW: &str = "CAST(strftime('%s', 'now') AS INTEGER)";

/// The rows of the blobs a JSON array of identities names, in one namespace.
const ROWS: &str = "SELECT integrity, incarnation, state, size FROM blobs
     WHERE catalog = ?1 AND ecosystem = ?2
       AND integrity IN (SELECT value FROM json_each(?3))";

/// The mapped files of one package a JSON array of file names names, with
/// the rows of their blobs.
const MAPPED: &str = "SELECT a.artifact, a.version, a.integrity, b.incarnation, b.state, b.size
     FROM artifact_blobs a
     JOIN blobs b
       ON b.catalog = a.catalog AND b.ecosystem = a.ecosystem AND b.integrity = a.integrity
     WHERE a.catalog = ?1 AND a.ecosystem = ?2 AND a.name = ?3
       AND a.artifact IN (SELECT value FROM json_each(?4))";

/// The blob a file of a package is mapped to, while it is live.
pub(super) const ARTIFACT: &str = "SELECT a.integrity, b.incarnation
     FROM artifact_blobs a
     JOIN blobs b
       ON b.catalog = a.catalog AND b.ecosystem = a.ecosystem AND b.integrity = a.integrity
     WHERE a.catalog = ?1 AND a.ecosystem = ?2 AND a.name = ?3 AND a.artifact = ?4
       AND b.state = 'live'";

/// The identity a column holds.
pub(super) fn id_from(text: &str) -> Result<BlobId> {
    BlobId::parse(text)
        .ok_or_else(|| RegistryError::Internal {
            reason: format!("the metadata names the blob {text:?}, which is no identity"),
        })
}

/// The identities in the first column of `rows`.
pub(super) fn ids_from_rows(rows: Vec<Row>) -> Result<Vec<BlobId>> {
    rows.into_iter()
        .map(|mut row| id_from(&row.get::<String>(0)?))
        .collect()
}

/// The blob row in the columns of `row` from `first` on: incarnation,
/// state, size.
pub(super) fn row_from(row: &mut Row, first: usize) -> Result<BlobRow> {
    let state: String = row.get(first + 1)?;
    let state = BlobState::parse(&state)
        .ok_or_else(|| RegistryError::Internal {
            reason: format!("a blob is in the unknown state {state:?}"),
        })?;
    Ok(BlobRow {
        incarnation: to_generation(row.get(first)?),
        state,
        size: to_generation(row.get(first + 2)?),
    })
}

/// The rows of [`ROWS`], by identity.
pub(super) fn rows_from(rows: Vec<Row>) -> Result<BTreeMap<BlobId, BlobRow>> {
    rows.into_iter()
        .map(|mut row| Ok((id_from(&row.get::<String>(0)?)?, row_from(&mut row, 1)?)))
        .collect()
}

/// `values` as the JSON array a query takes.
pub(super) fn json_array<Item: serde::Serialize>(values: &[Item]) -> String {
    serde_json::to_string(values).expect("strings serialize")
}

/// What a [`BlobLookup`] found: the row of each identity, `None` for bytes
/// never stored, and the mapped files.
pub(super) type LookupFound = (BTreeMap<BlobId, Option<BlobRow>>, Vec<MappedArtifact>);

/// The read of a [`BlobLookup`], made with the package's document.
pub(super) struct LookupRead {
    key: PackageKey,
    requested: Vec<BlobId>,
    ids: Option<String>,
    artifacts: Option<String>,
}

impl LookupRead {
    pub(super) fn new(key: PackageKey, lookup: &BlobLookup) -> Self {
        let ids: Vec<&str> = lookup.ids
            .iter()
            .map(BlobId::as_str)
            .collect();
        Self {
            key,
            requested: lookup.ids.clone(),
            ids: (!ids.is_empty()).then(|| json_array(&ids)),
            artifacts: (!lookup.artifacts.is_empty()).then(|| json_array(&lookup.artifacts)),
        }
    }

    pub(super) fn queries(&self) -> Vec<Query<'_>> {
        let (catalog, ecosystem, name) = self.key.params();
        let mut queries = Vec::new();
        if let Some(ids) = &self.ids {
            queries.push(query!(ROWS, catalog, ecosystem, ids.as_str()));
        }
        if let Some(artifacts) = &self.artifacts {
            queries.push(query!(MAPPED, catalog, ecosystem, name, artifacts.as_str()));
        }
        queries
    }

    /// What the rows of [`Self::queries`], taken from `results`, found.
    pub(super) fn answer(
        &self,
        results: &mut impl Iterator<Item = Vec<Row>>,
    ) -> Result<LookupFound> {
        let mut found = match &self.ids {
            Some(_) => rows_from(results.next().unwrap_or_default())?,
            None => BTreeMap::new(),
        };
        let blobs = self.requested
            .iter()
            .map(|id| (id.clone(), found.remove(id)))
            .collect();
        let artifacts = match &self.artifacts {
            Some(_) => results
                .next()
                .unwrap_or_default()
                .into_iter()
                .map(mapped_from)
                .collect::<Result<_>>()?,
            None => Vec::new(),
        };
        Ok((blobs, artifacts))
    }
}

fn mapped_from(mut row: Row) -> Result<MappedArtifact> {
    Ok(MappedArtifact {
        artifact: row.get(0)?,
        version: row.get(1)?,
        id: id_from(&row.get::<String>(2)?)?,
        row: row_from(&mut row, 3)?,
    })
}

/// What a commit adds to its first round trip to check its fence, and what
/// it writes with its `COMMIT`.
pub(super) struct FenceRead {
    artifacts: Vec<ArtifactBlob>,
    /// The identities the fence depends on in each ecosystem, as [`ROWS`]
    /// takes them.
    ids: Vec<(Ecosystem, String)>,
}

/// The blobs a commit found in place and those it creates.
pub(super) struct FenceCheck {
    created: Vec<(Ecosystem, ArtifactBlob)>,
}

impl FenceRead {
    /// `None` for a fence with no files, which needs no read.
    pub(super) fn new(fence: &BlobFence) -> Option<Self> {
        if fence.artifacts.is_empty() {
            return None;
        }
        let mut by_ecosystem: BTreeMap<&str, (Ecosystem, Vec<&str>)> = BTreeMap::new();
        for artifact in &fence.artifacts {
            let entry = by_ecosystem
                .entry(artifact.ecosystem.as_str())
                .or_insert_with(|| (artifact.ecosystem, Vec::new()));
            entry.1.push(artifact.blob.id.as_str());
        }
        let ids = by_ecosystem
            .into_values()
            .map(|(ecosystem, ids)| (ecosystem, json_array(&ids)))
            .collect();
        Some(Self { artifacts: fence.artifacts.clone(), ids })
    }

    pub(super) fn queries<'read>(&'read self, catalog: &'read str) -> Vec<Query<'read>> {
        self.ids
            .iter()
            .map(|(ecosystem, ids)| query!(ROWS, catalog, ecosystem.as_str(), ids.as_str()))
            .collect()
    }

    /// Check every file against the rows of [`Self::queries`], taken from
    /// `results`: its incarnation must be live, or be the next one, which
    /// the commit creates. Anything else is refused with
    /// [`RegistryError::ArchiveReclaimed`].
    pub(super) fn check(&self, results: &mut impl Iterator<Item = Vec<Row>>) -> Result<FenceCheck> {
        let mut rows: BTreeMap<(&str, BlobId), BlobRow> = BTreeMap::new();
        for (ecosystem, _) in &self.ids {
            for (id, row) in rows_from(results.next().unwrap_or_default())? {
                rows.insert((ecosystem.as_str(), id), row);
            }
        }
        let mut created: Vec<(Ecosystem, ArtifactBlob)> = Vec::new();
        for artifact in &self.artifacts {
            let row = rows.get(&(artifact.ecosystem.as_str(), artifact.blob.id.clone()));
            match depends(row, artifact.blob.incarnation) {
                Some(false) => {}
                Some(true) => created.push((artifact.ecosystem, artifact.clone())),
                None => {
                    return Err(RegistryError::ArchiveReclaimed {
                        package: artifact.name.clone(),
                        artifact: artifact.artifact.clone(),
                    });
                }
            }
        }
        Ok(FenceCheck { created })
    }

    /// Create the blobs `check` found to be new, and map every file to its
    /// blob, in the commit's transaction.
    pub(super) fn write(
        &self,
        transaction: &dyn Connection,
        catalog: &str,
        check: &FenceCheck,
    ) -> Result<()> {
        for (ecosystem, artifact) in &check.created {
            write_live(transaction, (catalog, *ecosystem), artifact)?;
        }
        for artifact in &self.artifacts {
            write_mapping(transaction, catalog, artifact)?;
        }
        Ok(())
    }
}

/// Whether a commit may depend on `incarnation` of a blob with `row`, and
/// whether it creates it: `Some(false)` for the live one, `Some(true)` for
/// the next one after one that is not live, or the first of a blob without a
/// row, and `None` for any other.
fn depends(row: Option<&BlobRow>, incarnation: u64) -> Option<bool> {
    match row {
        Some(row) if row.state == BlobState::Live => {
            (row.incarnation == incarnation).then_some(false)
        }
        Some(row) => (row.incarnation + 1 == incarnation).then_some(true),
        None => (incarnation == 1).then_some(true),
    }
}

/// Make `artifact`'s incarnation the live one of its blob.
fn write_live(
    transaction: &dyn Connection,
    (catalog, ecosystem): (&str, Ecosystem),
    artifact: &ArtifactBlob,
) -> Result<()> {
    let sql = format!(
        "INSERT INTO blobs (catalog, ecosystem, integrity, incarnation, state, size, changed_at)
         VALUES (?1, ?2, ?3, ?4, 'live', ?5, {NOW})
         ON CONFLICT (catalog, ecosystem, integrity) DO UPDATE
         SET incarnation = excluded.incarnation, state = 'live', size = excluded.size,
             changed_at = excluded.changed_at",
    );
    transaction.execute(
        &sql,
        params![
            catalog,
            ecosystem.as_str(),
            artifact.blob.id.as_str(),
            to_column(artifact.blob.incarnation),
            to_column(artifact.size),
        ],
    )
}

/// Map `artifact`'s file to its blob, unless the file is mapped already.
fn write_mapping(
    transaction: &dyn Connection,
    catalog: &str,
    artifact: &ArtifactBlob,
) -> Result<()> {
    let sql = format!(
        "INSERT INTO artifact_blobs
             (catalog, ecosystem, name, artifact, version, integrity, mapped_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, {NOW})
         ON CONFLICT DO NOTHING",
    );
    transaction.execute(
        &sql,
        params![
            catalog,
            artifact.ecosystem.as_str(),
            artifact.name,
            artifact.artifact,
            artifact.version,
            artifact.blob.id.as_str(),
        ],
    )
}
