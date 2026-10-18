use super::{
    COMMIT_DOCUMENT_WRITE_RETRIES, CanonicalPackageName, DocumentWrite, Result, Storage,
    wait_after_document_write_conflict,
};
use serde::{Deserialize, Serialize};

/// Reserved namespace for package tombstones.
pub const TOMBSTONES_NAMESPACE: &str = ".tombstones/v0";

/// Record of an unpublished package, retaining its former maintainers and
/// unpublished version numbers so deleted packages cannot be squatted and
/// unpublished versions cannot be republished.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageTombstone {
    pub package: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unpublished_by: Option<String>,
    pub unpublished_at: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub maintainers: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unpublished_versions: Vec<String>,
}

fn tombstone_key(name: &CanonicalPackageName) -> String {
    format!("{}.json", crate::layout::encode_segment(name.as_str()))
}

fn merge_tombstones(
    mut current: PackageTombstone,
    incoming: &PackageTombstone,
) -> PackageTombstone {
    if !incoming.maintainers.is_empty() {
        current.maintainers.clone_from(&incoming.maintainers);
    }
    for version in &incoming.unpublished_versions {
        if !current.unpublished_versions
            .iter()
            .any(|ver| ver == version)
        {
            current.unpublished_versions.push(version.clone());
        }
    }
    if incoming.unpublished_by.is_some() {
        current.unpublished_by.clone_from(&incoming.unpublished_by);
    }
    current.unpublished_at.clone_from(&incoming.unpublished_at);
    current
}

fn parse_tombstone(bytes: Option<&[u8]>) -> Result<Option<PackageTombstone>> {
    let Some(bytes) = bytes else {
        return Ok(None);
    };
    serde_json::from_slice(bytes).map(Some).map_err(pnpr_error::RegistryError::Json)
}

async fn write_tombstone_record(
    storage: &Storage,
    key: &str,
    current: Option<&[u8]>,
    updated: &[u8],
) -> Result<DocumentWrite> {
    match current {
        None => {
            if storage.hosted.create_record(TOMBSTONES_NAMESPACE, key, updated).await? {
                Ok(DocumentWrite::Written)
            } else {
                Ok(DocumentWrite::Conflict)
            }
        }
        Some(prev) => {
            storage.hosted.replace_record_if_current(TOMBSTONES_NAMESPACE, key, prev, updated).await
        }
    }
}

impl Storage {
    pub async fn read_tombstone(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Option<PackageTombstone>> {
        let key = tombstone_key(name);
        let Some(bytes) = self.hosted.read_record(TOMBSTONES_NAMESPACE, &key).await? else {
            return Ok(None);
        };
        let tombstone: PackageTombstone =
            serde_json::from_slice(&bytes).map_err(pnpr_error::RegistryError::Json)?;
        Ok(Some(tombstone))
    }

    pub async fn update_tombstone<Mutate>(
        &self,
        name: &CanonicalPackageName,
        mutate: Mutate,
    ) -> Result<PackageTombstone>
    where
        Mutate: Fn(Option<PackageTombstone>) -> PackageTombstone,
    {
        let key = tombstone_key(name);
        for attempt in 0..COMMIT_DOCUMENT_WRITE_RETRIES {
            let current = self.hosted.read_record(TOMBSTONES_NAMESPACE, &key).await?;
            let current_tombstone = parse_tombstone(current.as_deref())?;
            let updated = mutate(current_tombstone);
            let updated_bytes =
                serde_json::to_vec_pretty(&updated).map_err(pnpr_error::RegistryError::Json)?;

            if write_tombstone_record(self, &key, current.as_deref(), &updated_bytes).await?
                == DocumentWrite::Written
            {
                return Ok(updated);
            }

            if attempt + 1 < COMMIT_DOCUMENT_WRITE_RETRIES {
                wait_after_document_write_conflict(attempt).await;
            }
        }
        Err(pnpr_error::RegistryError::DocumentWriteConflict { package: name.as_str().to_string() })
    }

    pub async fn write_tombstone(&self, tombstone: &PackageTombstone) -> Result<()> {
        let name =
            CanonicalPackageName::parse(&tombstone.package, pnpr_package_name::Ecosystem::Npm)?;
        let incoming = tombstone.clone();
        self.update_tombstone(&name, move |existing| match existing {
            Some(current) => merge_tombstones(current, &incoming),
            None => incoming.clone(),
        })
        .await?;
        Ok(())
    }

    pub async fn remove_tombstone(&self, name: &CanonicalPackageName) -> Result<bool> {
        let key = tombstone_key(name);
        self.hosted.remove_record(TOMBSTONES_NAMESPACE, &key).await
    }
}
