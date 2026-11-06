//! Mapping the files of a package to their blobs outside a publication
//! commit: an image blob's upload maps it, and a reclaim of image blobs no
//! manifest reaches drops the mapping.

use super::{
    PackageKey, SqlMetadata,
    adapter::{Connection, Query},
    batch::WriteTransaction,
    blobs::{id_from, json_array},
    commit::{package_row_from, package_row_query},
    fence::FenceRead,
    to_generation,
    transactions::Freshness,
};
use crate::{
    BlobId,
    metadata::{ArtifactBlob, BlobFence},
};
use pnpr_error::Result;
use pnpr_package_name::Ecosystem;
use std::{sync::Arc, time::SystemTime};

/// A file of a package mapped to a blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mapping {
    pub artifact: String,
    pub id: BlobId,
    /// When the file was mapped.
    pub mapped_at: SystemTime,
    /// What names this one mapping of the file, so a removal of what a scan
    /// found never removes a mapping the file got again since.
    pub mapping: String,
}

/// The mapping of a file to remove: any current one, or only the one a
/// scan found.
#[derive(Debug, Clone)]
pub struct Unmapping {
    pub artifact: String,
    pub mapping: Option<String>,
}

impl From<String> for Unmapping {
    fn from(artifact: String) -> Self {
        Self { artifact, mapping: None }
    }
}

impl From<Mapping> for Unmapping {
    fn from(mapping: Mapping) -> Self {
        Self { artifact: mapping.artifact, mapping: Some(mapping.mapping) }
    }
}

/// A fresh token naming one mapping of a file.
pub(super) fn new_mapping_token() -> String {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).expect("OS CSPRNG must be available");
    bytes
        .iter()
        .fold(String::with_capacity(32), |mut hex, byte| {
            use std::fmt::Write as _;
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

impl SqlMetadata {
    /// Map `artifact`'s file to its blob, unless the file is mapped already,
    /// creating the blob's incarnation when it is the next one. Refused with
    /// [`pnpr_error::RegistryError::ArchiveReclaimed`] as a commit that
    /// depends on it would be.
    pub async fn map_artifact(
        self: &Arc<Self>,
        catalog: &str,
        artifact: ArtifactBlob,
    ) -> Result<()> {
        let catalog = catalog.to_string();
        let fence = BlobFence { artifacts: vec![artifact], references: Vec::new() };
        self.write(move |connection| {
            let read = FenceRead::new(&fence).expect("a fence of one file");
            let transaction = WriteTransaction::begin(connection)?;
            let results = transaction.query_all(&read.queries(&catalog))?;
            let check = read.check(&mut results.into_iter())?;
            read.write(&*transaction, &catalog, &check)?;
            transaction.commit()
        })
        .await
    }

    /// The packages of `catalog`'s `ecosystem` with a mapped file.
    pub async fn mapped_packages(
        self: &Arc<Self>,
        catalog: &str,
        ecosystem: Ecosystem,
    ) -> Result<Vec<String>> {
        let catalog = catalog.to_string();
        self.read(catalog.clone(), Freshness::Current, move |connection, _| {
            connection
                .query(
                    "SELECT DISTINCT name FROM artifact_blobs
                     WHERE catalog = ?1 AND ecosystem = ?2 ORDER BY name",
                    params![catalog, ecosystem.as_str()],
                )?
                .into_iter()
                .map(|mut row| row.get(0))
                .collect()
        })
        .await
    }

    /// Every mapped file of `name` in `catalog`'s `ecosystem`, by file name,
    /// with the package's generation, `None` when it has no metadata, read
    /// in one snapshot.
    pub async fn package_mappings(
        self: &Arc<Self>,
        catalog: &str,
        ecosystem: Ecosystem,
        name: &str,
    ) -> Result<(Option<u64>, Vec<Mapping>)> {
        let key = PackageKey { catalog: catalog.to_string(), ecosystem, name: name.to_string() };
        self.read(catalog.to_string(), Freshness::Current, move |connection, _| {
            let (catalog, ecosystem, name) = key.params();
            let mapped = query!(
                "SELECT artifact, integrity, mapped_at, mapping FROM artifact_blobs
                 WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3 ORDER BY artifact",
                catalog,
                ecosystem,
                name,
            );
            let mut results = connection
                .query_all(&[package_row_query(&key), mapped])?
                .into_iter();
            let generation =
                package_row_from(results.next().unwrap_or_default())?.map(|row| row.generation);
            let mappings = results
                .next()
                .unwrap_or_default()
                .into_iter()
                .map(|mut row| {
                    let seconds = to_generation(row.get(2)?);
                    Ok(Mapping {
                        artifact: row.get(0)?,
                        id: id_from(&row.get::<String>(1)?)?,
                        mapped_at: SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(seconds),
                        mapping: row.get(3)?,
                    })
                })
                .collect::<Result<_>>()?;
            Ok((generation, mappings))
        })
        .await
    }

    /// Drop the mappings of `artifacts` of `name`, while the package is still
    /// at `expected`, the generation they were found unreferenced at, or
    /// still without metadata for `None`. Returns the blobs they were mapped
    /// to, `None` when the package moved on.
    pub async fn unmap_artifacts(
        self: &Arc<Self>,
        catalog: &str,
        ecosystem: Ecosystem,
        name: &str,
        (expected, unmappings): (Option<u64>, Vec<Unmapping>),
    ) -> Result<Option<Vec<BlobId>>> {
        let key = PackageKey { catalog: catalog.to_string(), ecosystem, name: name.to_string() };
        let (any, exact): (Vec<_>, Vec<_>) =
            unmappings.into_iter().partition(|unmapping| unmapping.mapping.is_none());
        let any: Vec<String> = any
            .into_iter()
            .map(|unmapping| unmapping.artifact)
            .collect();
        let exact: Vec<(String, Option<String>)> = exact
            .into_iter()
            .map(|unmapping| (unmapping.artifact, unmapping.mapping))
            .collect();
        let scanned = (json_array(&any), json_array(&exact));
        self.write(move |connection| unmap(connection, &key, &scanned, expected)).await
    }
}

/// The mappings of a package that `?4`, a JSON array of files whose current
/// mapping goes, or `?5`, a JSON array of `[file, mapping]` that goes only
/// while the file still has that mapping, names. Each list is read once
/// into a lookup the package's rows are checked against.
const SCANNED_MAPPINGS: &str = "catalog = ?1 AND ecosystem = ?2 AND name = ?3
     AND (artifact IN (SELECT value FROM json_each(?4))
          OR (artifact, mapping) IN (
              SELECT json_extract(value, '$[0]'), json_extract(value, '$[1]')
              FROM json_each(?5)))";

/// Drop the mappings `scanned` names while `key` is still at `expected`.
fn unmap(
    connection: &dyn Connection,
    key: &PackageKey,
    (any, exact): &(String, String),
    expected: Option<u64>,
) -> Result<Option<Vec<BlobId>>> {
    let (catalog, ecosystem, name) = key.params();
    let transaction = WriteTransaction::begin(connection)?;
    let select = format!("SELECT DISTINCT integrity FROM artifact_blobs WHERE {SCANNED_MAPPINGS}");
    let mapped = Query::new(&select, params![catalog, ecosystem, name, any, exact]);
    let mut results = transaction
        .query_all(&[package_row_query(key), mapped])?
        .into_iter();
    let generation =
        package_row_from(results.next().unwrap_or_default())?.map(|row| row.generation);
    if generation != expected {
        return Ok(None);
    }
    let ids = super::blobs::ids_from_rows(results.next().unwrap_or_default())?;
    let delete = format!("DELETE FROM artifact_blobs WHERE {SCANNED_MAPPINGS}");
    transaction.execute(&delete, params![catalog, ecosystem, name, any, exact])?;
    transaction.commit()?;
    Ok(Some(ids))
}

impl super::PackageMetadata {
    /// The live incarnation of the blob `artifact` of `name` is mapped to,
    /// `None` when the package maps no such file.
    pub(crate) async fn artifact_blob(
        &self,
        name: &pnpr_package_name::CanonicalPackageName,
        artifact: &str,
    ) -> Result<Option<crate::metadata::BlobRef>> {
        let key = self.key(name);
        let artifact = artifact.to_string();
        self.database.read(self.catalog.clone(), Freshness::Bounded, move |connection, _| {
            let (catalog, ecosystem, name) = key.params();
            let row = connection.query_optional(
                super::blobs::ARTIFACT,
                params![catalog, ecosystem, name, artifact],
            )?;
            row.map(|mut row| {
                Ok(crate::metadata::BlobRef {
                    id: id_from(&row.get::<String>(0)?)?,
                    incarnation: to_generation(row.get(1)?),
                })
            })
            .transpose()
        })
        .await
    }

    /// The blobs the files of `versions` of `name` are mapped to, read from
    /// the primary: what removing those versions may leave unmapped.
    pub(crate) async fn version_blobs(
        &self,
        name: &pnpr_package_name::CanonicalPackageName,
        versions: &[String],
    ) -> Result<Vec<BlobId>> {
        let key = self.key(name);
        let versions = json_array(versions);
        self.database.read(self.catalog.clone(), Freshness::Current, move |connection, _| {
            let (catalog, ecosystem, name) = key.params();
            let rows = connection.query(
                "SELECT DISTINCT integrity FROM artifact_blobs
                 WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3
                   AND version IN (SELECT value FROM json_each(?4))",
                params![catalog, ecosystem, name, versions],
            )?;
            super::blobs::ids_from_rows(rows)
        })
        .await
    }
}
