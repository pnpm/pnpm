use serde::{Deserialize, Serialize};
use ssri::Integrity;

#[derive(Debug, Default, Clone, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageDistribution {
    pub integrity: Option<Integrity>,
    pub shasum: Option<String>,
    pub tarball: String,
    pub file_count: Option<usize>,
    pub unpacked_size: Option<usize>,
}

impl PartialEq for PackageDistribution {
    fn eq(&self, other: &Self) -> bool {
        self.integrity == other.integrity
    }
}
