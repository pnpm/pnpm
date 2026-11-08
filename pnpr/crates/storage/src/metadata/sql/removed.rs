use super::{
    PackageKey,
    adapter::Connection,
    batch::Batch,
    commit::{package_row_from, package_row_query},
    to_column,
    transactions::Freshness,
};
use crate::{BlobId, PackageTombstone, metadata::PackageMetadata};
use pnpr_error::Result;
use pnpr_package_name::Ecosystem;
use serde_json::Value;
use std::collections::BTreeSet;

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

/// Remove every row of the package except its tag history and the identities
/// of its versions, which stay removed, under a new generation. Returns the
/// blobs its files were mapped to, `None` when there was no package.
pub(super) fn remove_package(
    connection: &dyn Connection,
    key: &PackageKey,
    principal: Option<&str>,
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
    let Some(row) = package_row_from(results.next().unwrap_or_default())? else {
        return Ok(None);
    };
    let ids = super::blobs::ids_from_rows(results.next().unwrap_or_default())?;
    record_removed_package(batch.transaction(), key, batch.generation(), &row.state, principal)?;
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

pub(super) fn record_package(
    connection: &dyn Connection,
    key: &PackageKey,
    state: &[u8],
    principal: Option<&str>,
) -> Result<()> {
    let maintainers = if key.ecosystem == Ecosystem::Npm {
        let state: Value = serde_json::from_slice(state)?;
        maintainer_ids(&state)
    } else {
        Vec::new()
    };
    let maintainers = serde_json::to_string(&maintainers)?;
    let (catalog, ecosystem, name) = key.params();
    connection.execute(
        "INSERT INTO removed_packages (catalog, ecosystem, name, maintainers, unpublished_at, unpublished_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (catalog, ecosystem, name) DO UPDATE SET
           maintainers = CASE WHEN excluded.maintainers = '[]' THEN removed_packages.maintainers ELSE excluded.maintainers END,
           unpublished_at = excluded.unpublished_at, unpublished_by = excluded.unpublished_by",
        params![catalog, ecosystem, name, maintainers, crate::publish::now_millis(), principal],
    )?;
    Ok(())
}

pub(super) fn record_removed_package(
    connection: &dyn Connection,
    key: &PackageKey,
    generation: u64,
    state: &[u8],
    principal: Option<&str>,
) -> Result<()> {
    record_package(connection, key, state, principal)?;
    record_oci_manifests(connection, key, generation, state, b"")?;
    let (catalog, ecosystem, name) = key.params();
    connection.execute(
        "INSERT INTO removed_versions (catalog, ecosystem, name, version, revision_key, generation)
         SELECT catalog, ecosystem, name, version, revision_key, ?4 FROM versions
         WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3
         ON CONFLICT DO NOTHING",
        params![catalog, ecosystem, name, to_column(generation)],
    )?;
    Ok(())
}

#[derive(serde::Deserialize)]
struct StoredImageManifests {
    #[serde(default)]
    manifests: Vec<StoredManifestDigest>,
}

#[derive(serde::Deserialize)]
struct StoredManifestDigest {
    digest: String,
}

fn oci_manifest_artifacts(bytes: &[u8]) -> Result<BTreeSet<String>> {
    if bytes.is_empty() {
        return Ok(BTreeSet::new());
    }
    let document: StoredImageManifests = serde_json::from_slice(bytes)?;
    Ok(document.manifests
        .into_iter()
        .map(|entry| entry.digest.replacen(':', "-", 1))
        .collect())
}

pub(super) fn record_oci_manifests(
    connection: &dyn Connection,
    key: &PackageKey,
    generation: u64,
    previous: &[u8],
    incoming: &[u8],
) -> Result<()> {
    if key.ecosystem != Ecosystem::Oci {
        return Ok(());
    }
    let before = oci_manifest_artifacts(previous)?;
    let after = oci_manifest_artifacts(incoming)?;
    let (catalog, ecosystem, name) = key.params();
    for artifact in before.difference(&after) {
        connection.execute(
            "INSERT INTO removed_versions
               (catalog, ecosystem, name, version, revision_key, generation)
             VALUES (?1, ?2, ?3, '', ?4, ?5)
             ON CONFLICT (catalog, ecosystem, name, version, revision_key) DO UPDATE SET
               generation = excluded.generation",
            params![catalog, ecosystem, name, artifact, to_column(generation)],
        )?;
    }
    Ok(())
}

fn maintainer_ids(state: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(entries) = state.get("maintainers").and_then(Value::as_array) {
        for entry in entries {
            if let Some(id) = entry.get("userId").and_then(Value::as_str)
                && !ids
                    .iter()
                    .any(|existing| existing == id)
            {
                ids.push(id.to_string());
            }
        }
    }
    ids
}

impl PackageMetadata {
    pub(crate) async fn read_tombstone(
        &self,
        name: &pnpr_package_name::CanonicalPackageName,
    ) -> Result<Option<PackageTombstone>> {
        let key = self.key(name);
        self.database.read(self.catalog.clone(), Freshness::Current, move |connection, _| {
            read_package(connection, &key)
        })
        .await
    }
}

fn read_package(connection: &dyn Connection, key: &PackageKey) -> Result<Option<PackageTombstone>> {
    let (catalog, ecosystem, name) = key.params();
    let Some(mut row) = connection.query_optional(
        "SELECT maintainers, unpublished_at, unpublished_by FROM removed_packages
         WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3",
        params![catalog, ecosystem, name],
    )?
    else {
        return Ok(None);
    };
    let maintainers: String = row.get(0)?;
    let unpublished_versions = connection
        .query(
            "SELECT DISTINCT version FROM removed_versions
         WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3 AND version != '' ORDER BY version",
            params![catalog, ecosystem, name],
        )?
        .into_iter()
        .map(|mut row| row.get(0))
        .collect::<Result<_>>()?;
    Ok(Some(PackageTombstone {
        package: name.to_string(),
        maintainers: serde_json::from_str(&maintainers)?,
        unpublished_at: crate::publish::iso_from_unix_millis(row.get(1)?),
        unpublished_by: row.get(2)?,
        unpublished_versions,
    }))
}
