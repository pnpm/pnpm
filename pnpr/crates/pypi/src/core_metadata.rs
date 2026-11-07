use crate::{ProjectFile, escape_html};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt::Write as _};

/// Availability and optional digests of a PEP 658 metadata sidecar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CoreMetadata {
    Available(bool),
    Hashes(BTreeMap<String, String>),
}

impl CoreMetadata {
    #[must_use]
    pub fn is_available(&self) -> bool {
        !matches!(self, Self::Available(false))
    }

    pub(super) fn html_value(&self) -> Option<String> {
        match self {
            Self::Available(false) => None,
            Self::Available(true) => Some("true".to_string()),
            Self::Hashes(hashes) => Some(
                hashes
                    .get_key_value("sha256")
                    .or_else(|| hashes.first_key_value())
                    .map_or_else(
                        || "true".to_string(),
                        |(algorithm, digest)| format!("{algorithm}={digest}"),
                    ),
            ),
        }
    }
}

impl ProjectFile {
    #[must_use]
    pub fn metadata(&self) -> Option<&CoreMetadata> {
        self.core_metadata.as_ref().or(self.dist_info_metadata.as_ref())
    }

    pub(super) fn render_metadata_attributes(&self, html: &mut String) {
        for (attribute, metadata) in [
            ("data-core-metadata", &self.core_metadata),
            ("data-dist-info-metadata", &self.dist_info_metadata),
        ] {
            if let Some(value) = metadata.as_ref().and_then(CoreMetadata::html_value) {
                let _ = write!(html, r#" {attribute}="{}""#, escape_html(&value));
            }
        }
    }
}
