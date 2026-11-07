use crate::{CoreMetadata, Yanked, file_url};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// One distribution file of a project, in the PEP 691 key spelling.
///
/// `url` is the file's origin URL as an upstream index published it, and is
/// absent in a hosted document, where the server derives every file's URL
/// from its own address at render time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(perfectionist::too_many_struct_fields, reason = "PEP 691 file entry fields")
)]
pub struct ProjectFile {
    pub filename: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Lowercase hash algorithm name to lowercase hex digest. A file must
    /// carry a `sha256`.
    #[serde(default)]
    pub hashes: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires_python: Option<String>,
    #[serde(default)]
    pub yanked: Yanked,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upload_time: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::core_metadata::deserialize_optional_metadata"
    )]
    pub core_metadata: Option<CoreMetadata>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::core_metadata::deserialize_optional_metadata"
    )]
    pub dist_info_metadata: Option<CoreMetadata>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::core_metadata::deserialize_optional_metadata"
    )]
    pub data_dist_info_metadata: Option<CoreMetadata>,
}

impl ProjectFile {
    pub(super) fn render_json(&self, file_base: &str) -> Value {
        let mut entry = serde_json::to_value(self).expect("project file serializes");
        entry["url"] = json!(file_url(file_base, &self.filename));
        for legacy_key in ["dist-info-metadata", "data-dist-info-metadata"] {
            entry
                .as_object_mut()
                .expect("project file is an object")
                .remove(legacy_key);
        }
        if let Some(metadata) = self.metadata() {
            entry["core-metadata"] = json!(metadata);
        }
        entry
    }

    /// The file's lowercase hex SHA-256, when the index published one.
    #[must_use]
    pub fn sha256(&self) -> Option<&str> {
        self.hashes.get("sha256").map(String::as_str)
    }
}
