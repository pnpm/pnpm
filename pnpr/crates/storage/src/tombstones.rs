use super::{CanonicalPackageName, Result, Storage};
use serde::{Deserialize, Serialize};

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

impl Storage {
    pub async fn read_tombstone(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Option<PackageTombstone>> {
        self.metadata.read_tombstone(name).await
    }
}
