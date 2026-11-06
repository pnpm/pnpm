//! The rows of the hosted store's blobs and of the files mapped to them (see
//! [`crate::metadata::BlobRow`]), as a publish reads them.

use super::{
    PackageKey,
    adapter::{Query, Row},
    to_generation,
};
use crate::{
    BlobId,
    metadata::{BlobLookup, BlobRow, BlobState, MappedArtifact},
};
use pnpr_error::{RegistryError, Result};
use std::collections::BTreeMap;

/// The current time in seconds since the Unix epoch, as the database tells
/// it, so every instance reads one clock.
pub(super) const NOW: &str = "CAST(strftime('%s', 'now') AS INTEGER)";

/// The rows of the blobs a JSON array of identities names, in one namespace.
pub(super) const ROWS: &str = "SELECT integrity, incarnation, state, size FROM blobs
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
