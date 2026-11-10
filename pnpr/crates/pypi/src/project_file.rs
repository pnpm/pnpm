use crate::{CoreMetadata, Yanked, escape_html, file_url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, fmt::Write as _};

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
    /// Validated hosted distribution metadata, retained verbatim for sidecar requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hosted_metadata: Option<String>,
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
        serde_json::to_value(SimpleProjectFile {
            filename: &self.filename,
            url: file_url(file_base, &self.filename),
            hashes: &self.hashes,
            requires_python: self.requires_python.as_deref(),
            yanked: &self.yanked,
            size: self.size,
            upload_time: self.upload_time.as_deref(),
            core_metadata: self.metadata(),
        })
        .expect("project file serializes")
    }

    /// The file's lowercase hex SHA-256, when the index published one.
    #[must_use]
    pub fn sha256(&self) -> Option<&str> {
        self.hashes.get("sha256").map(String::as_str)
    }

    #[must_use]
    pub fn metadata(&self) -> Option<&CoreMetadata> {
        self.core_metadata
            .as_ref()
            .or(self.dist_info_metadata.as_ref())
            .or(self.data_dist_info_metadata.as_ref())
            .filter(|metadata| metadata.is_supported())
    }

    pub(super) fn render_metadata_attributes(&self, html: &mut String) {
        let Some(value) = self.metadata().and_then(CoreMetadata::html_value) else { return };
        for attribute in ["data-core-metadata", "data-dist-info-metadata"] {
            let _ = write!(html, r#" {attribute}="{}""#, escape_html(&value));
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct SimpleProjectFile<'file> {
    filename: &'file str,
    url: String,
    hashes: &'file BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    requires_python: Option<&'file str>,
    yanked: &'file Yanked,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    upload_time: Option<&'file str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    core_metadata: Option<&'file CoreMetadata>,
}
