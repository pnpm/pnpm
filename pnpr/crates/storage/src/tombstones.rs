use super::{CanonicalPackageName, DocumentWrite, Result, Storage};
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
    format!("{}.json", name.as_str())
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

    async fn try_write_tombstone(&self, key: &str, bytes: &[u8]) -> Result<DocumentWrite> {
        let Some(current) = self.hosted.read_record(TOMBSTONES_NAMESPACE, key).await? else {
            if self.hosted.create_record(TOMBSTONES_NAMESPACE, key, bytes).await? {
                return Ok(DocumentWrite::Written);
            }
            return Ok(DocumentWrite::Conflict);
        };
        self.hosted.replace_record_if_current(TOMBSTONES_NAMESPACE, key, &current, bytes).await
    }

    pub async fn write_tombstone(&self, tombstone: &PackageTombstone) -> Result<()> {
        let name =
            CanonicalPackageName::parse(&tombstone.package, pnpr_package_name::Ecosystem::Npm)?;
        let key = tombstone_key(&name);
        let bytes = serde_json::to_vec_pretty(tombstone).map_err(pnpr_error::RegistryError::Json)?;
        for attempt in 0..5 {
            if self.try_write_tombstone(&key, &bytes).await? == DocumentWrite::Written {
                return Ok(());
            }
            if attempt + 1 < 5 {
                tokio::time::sleep(std::time::Duration::from_millis(10 * (attempt + 1) as u64))
                    .await;
            }
        }
        Err(pnpr_error::RegistryError::DocumentWriteConflict { package: tombstone.package.clone() })
    }

    pub async fn remove_tombstone(&self, name: &CanonicalPackageName) -> Result<bool> {
        let key = tombstone_key(name);
        self.hosted.remove_record(TOMBSTONES_NAMESPACE, &key).await
    }
}
