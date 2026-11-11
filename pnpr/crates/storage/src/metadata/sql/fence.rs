//! A commit's fence (see [`crate::metadata::BlobFence`]): what it checks in
//! its first round trip, and the blob rows and mappings it writes with its
//! `COMMIT`.

use super::{
    adapter::{Connection, Query, Row},
    blobs::{ROWS, id_from, json_array, rows_from},
    to_column,
};
use crate::{
    BlobId,
    metadata::{ArtifactBlob, BlobFence, BlobRow, BlobState},
};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::Ecosystem;
use pnpr_sql::{Dialect, DialectSql};
use std::collections::{BTreeMap, BTreeSet};

/// The files of one package a JSON array names that are mapped, with the
/// blobs they are mapped to.
const MAPPED: DialectSql = DialectSql {
    sqlite: "SELECT artifact, blob_id FROM artifact_blobs
             WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3
               AND artifact IN (SELECT value FROM json_each(?4))",
    postgres: "SELECT artifact, blob_id FROM artifact_blobs
               WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3
                 AND artifact IN (SELECT value FROM json_array_elements_text(CAST(?4 AS JSON)) AS elements(value))",
    mysql: "SELECT artifact, blob_id FROM artifact_blobs
            WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3
              AND artifact IN (SELECT JSON_UNQUOTE(value) AS value FROM JSON_TABLE(?4, '$[*]' COLUMNS (value JSON PATH '$')) AS elements)",
};

/// How a commit depends on an incarnation of a blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dependency {
    /// The live incarnation, in place already.
    Live,
    /// The next one, which the commit makes live.
    Creates,
}

/// What a commit adds to its first round trip to check its fence, and what
/// it writes with its `COMMIT`.
pub(super) struct FenceRead {
    fence: BlobFence,
    /// The identities the fence depends on in each ecosystem, as [`ROWS`]
    /// takes them.
    ids: Vec<(Ecosystem, String)>,
    /// The referenced files of each package, as [`MAPPED`] takes them.
    referenced: Vec<((Ecosystem, String), String)>,
}

/// The blobs a commit creates.
pub(super) struct FenceCheck {
    created: Vec<(Ecosystem, ArtifactBlob)>,
}

impl FenceRead {
    /// `None` for a fence with no files, which needs no read.
    pub(super) fn new(fence: &BlobFence) -> Option<Self> {
        if fence.artifacts.is_empty() && fence.references.is_empty() {
            return None;
        }
        let all = fence.artifacts.iter().chain(&fence.references);
        let mut ids: BTreeMap<&str, (Ecosystem, BTreeSet<&str>)> = BTreeMap::new();
        for artifact in all {
            let entry = ids
                .entry(artifact.ecosystem.as_str())
                .or_insert_with(|| (artifact.ecosystem, BTreeSet::new()));
            entry.1.insert(artifact.blob.id.as_str());
        }
        let mut referenced: BTreeMap<(&str, &str), (Ecosystem, Vec<&str>)> = BTreeMap::new();
        for reference in &fence.references {
            let key = (reference.ecosystem.as_str(), reference.name.as_str());
            let entry = referenced
                .entry(key)
                .or_insert_with(|| (reference.ecosystem, Vec::new()));
            entry.1.push(reference.artifact.as_str());
        }
        Some(Self {
            fence: fence.clone(),
            ids: ids
                .into_values()
                .map(|(ecosystem, ids)| (ecosystem, json_array(&Vec::from_iter(ids))))
                .collect(),
            referenced: referenced
                .into_iter()
                .map(|((_, name), (ecosystem, files))| {
                    ((ecosystem, name.to_string()), json_array(&files))
                })
                .collect(),
        })
    }

    pub(super) fn queries<'read>(
        &'read self,
        dialect: Dialect,
        namespace: &'read str,
    ) -> Vec<Query<'read>> {
        let (rows_sql, mapped_sql) = (ROWS.get(dialect), MAPPED.get(dialect));
        let rows = self.ids
            .iter()
            .map(|(ecosystem, ids)| query!(rows_sql, namespace, ecosystem.as_str(), ids.as_str()));
        let mapped = self.referenced
            .iter()
            .map(|((ecosystem, name), files)| {
                query!(mapped_sql, namespace, ecosystem.as_str(), name.as_str(), files.as_str())
            });
        rows.chain(mapped).collect()
    }

    /// Check the fence against the rows of [`Self::queries`], taken from
    /// `results`. A file the commit maps depends on the live incarnation of
    /// its blob or on the next one, which the commit creates. A file it
    /// references must still be mapped to its blob, whose incarnation must
    /// be live: a reference whose file a deletion unmapped since it was read
    /// is not brought back. Anything else is refused with
    /// [`RegistryError::ArchiveReclaimed`].
    pub(super) fn check(&self, results: &mut impl Iterator<Item = Vec<Row>>) -> Result<FenceCheck> {
        let mut rows: BTreeMap<(&str, BlobId), BlobRow> = BTreeMap::new();
        for (ecosystem, _) in &self.ids {
            for (id, row) in rows_from(results.next().unwrap_or_default())? {
                rows.insert((ecosystem.as_str(), id), row);
            }
        }
        let mapped = self.mapped_references(results)?;
        let row_of = |artifact: &ArtifactBlob| {
            rows.get(&(artifact.ecosystem.as_str(), artifact.blob.id.clone()))
                .copied()
        };
        for reference in &self.fence.references {
            let key = (reference.ecosystem, reference.name.clone(), reference.artifact.clone());
            let still_mapped = mapped.get(&key) == Some(&reference.blob.id);
            if !still_mapped
                || depends(row_of(reference).as_ref(), reference.blob.incarnation)
                    != Some(Dependency::Live)
            {
                return Err(reclaimed(reference));
            }
        }
        let mut created = Vec::new();
        for artifact in &self.fence.artifacts {
            match depends(row_of(artifact).as_ref(), artifact.blob.incarnation) {
                Some(Dependency::Live) => {}
                Some(Dependency::Creates) => created.push((artifact.ecosystem, artifact.clone())),
                None => return Err(reclaimed(artifact)),
            }
        }
        Ok(FenceCheck { created })
    }

    /// The blob each referenced file is mapped to, from the rows of
    /// [`MAPPED`] taken from `results`.
    fn mapped_references(
        &self,
        results: &mut impl Iterator<Item = Vec<Row>>,
    ) -> Result<BTreeMap<(Ecosystem, String, String), BlobId>> {
        let mut mapped = BTreeMap::new();
        for ((ecosystem, name), _) in &self.referenced {
            for mut row in results.next().unwrap_or_default() {
                let artifact: String = row.get(0)?;
                let id = id_from(&row.get::<String>(1)?)?;
                mapped.insert((*ecosystem, name.clone(), artifact), id);
            }
        }
        Ok(mapped)
    }

    /// Create the blobs `check` found to be new, and map every file the
    /// commit stored to its blob, in the commit's transaction.
    pub(super) fn write(
        &self,
        transaction: &dyn Connection,
        namespace: &str,
        check: &FenceCheck,
    ) -> Result<()> {
        for (ecosystem, artifact) in &check.created {
            write_live(transaction, (namespace, *ecosystem), artifact)?;
        }
        for artifact in &self.fence.artifacts {
            write_mapping(transaction, namespace, artifact)?;
        }
        Ok(())
    }
}

fn reclaimed(artifact: &ArtifactBlob) -> RegistryError {
    RegistryError::ArchiveReclaimed {
        package: artifact.name.clone(),
        artifact: artifact.artifact.clone(),
    }
}

/// How a commit may depend on `incarnation` of a blob with `row`: on the
/// live one, or by creating the next one after one that is not live, or the
/// first of a blob without a row. `None` for any other.
fn depends(row: Option<&BlobRow>, incarnation: u64) -> Option<Dependency> {
    match row {
        Some(row) if row.state == BlobState::Live => {
            (row.incarnation == incarnation).then_some(Dependency::Live)
        }
        Some(row) => (row.incarnation + 1 == incarnation).then_some(Dependency::Creates),
        None => (incarnation == 1).then_some(Dependency::Creates),
    }
}

const WRITE_LIVE: DialectSql = DialectSql::with_mysql(
    "INSERT INTO blobs (namespace, ecosystem, blob_id, incarnation, state, size, changed_at)
     VALUES (?1, ?2, ?3, ?4, 'live', ?5, ?6)
     ON CONFLICT (namespace, ecosystem, blob_id) DO UPDATE
     SET incarnation = excluded.incarnation, state = 'live', size = excluded.size,
         changed_at = excluded.changed_at",
    "INSERT INTO blobs (namespace, ecosystem, blob_id, incarnation, state, size, changed_at)
     VALUES (?1, ?2, ?3, ?4, 'live', ?5, ?6)
     AS excluded ON DUPLICATE KEY UPDATE incarnation = excluded.incarnation, state = 'live', size = excluded.size,
         changed_at = excluded.changed_at",
);

/// Make `artifact`'s incarnation the live one of its blob.
fn write_live(
    transaction: &dyn Connection,
    (namespace, ecosystem): (&str, Ecosystem),
    artifact: &ArtifactBlob,
) -> Result<()> {
    transaction.execute(
        WRITE_LIVE.get(transaction.dialect()),
        params![
            namespace,
            ecosystem.as_str(),
            artifact.blob.id.as_str(),
            to_column(artifact.blob.incarnation),
            to_column(artifact.size),
            crate::publish::now_millis(),
        ],
    )
}

const WRITE_MAPPING: DialectSql = DialectSql::with_mysql(
    "INSERT INTO artifact_blobs
         (namespace, ecosystem, name, artifact, version, blob_id, mapped_at, mapping)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?8, ?7)
     ON CONFLICT (namespace, ecosystem, name, artifact) DO UPDATE
     SET mapped_at = excluded.mapped_at, mapping = excluded.mapping
     WHERE artifact_blobs.blob_id = excluded.blob_id",
    "INSERT INTO artifact_blobs
         (namespace, ecosystem, name, artifact, version, blob_id, mapped_at, mapping)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?8, ?7)
     AS excluded ON DUPLICATE KEY UPDATE
     mapped_at = IF(artifact_blobs.blob_id = excluded.blob_id, excluded.mapped_at, artifact_blobs.mapped_at),
     mapping = IF(artifact_blobs.blob_id = excluded.blob_id, excluded.mapping, artifact_blobs.mapping)",
);

/// Map `artifact`'s file to its blob, unless the file is mapped to other
/// bytes already. A file mapped to these bytes again gets a fresh mapping,
/// so a collection that scanned the earlier one leaves it.
fn write_mapping(
    transaction: &dyn Connection,
    namespace: &str,
    artifact: &ArtifactBlob,
) -> Result<()> {
    transaction.execute(
        WRITE_MAPPING.get(transaction.dialect()),
        params![
            namespace,
            artifact.ecosystem.as_str(),
            artifact.name,
            artifact.artifact,
            artifact.version,
            artifact.blob.id.as_str(),
            super::mappings::new_mapping_token(),
            crate::publish::now_millis(),
        ],
    )
}
