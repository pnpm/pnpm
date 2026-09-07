use crate::{Digest, media_type};
use derive_more::{Display, Error};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The schema every manifest pnpr accepts declares. Docker's schema 1 is a
/// different document with signatures instead of a config, and is long
/// deprecated; refusing it here keeps one shape downstream.
const SCHEMA_VERSION: u64 = 2;

#[derive(Debug, Display, Error, Clone, PartialEq, Eq)]
pub enum ManifestError {
    #[display("manifest is not valid JSON: {reason}")]
    Malformed { reason: String },
    #[display("manifest schemaVersion {version} is not supported, expected {SCHEMA_VERSION}")]
    UnsupportedSchemaVersion { version: u64 },
    #[display("manifest media type {media_type:?} is not supported")]
    UnsupportedMediaType { media_type: String },
    #[display("an image manifest must carry a config descriptor")]
    MissingConfig,
    #[display("an image referrer must carry artifactType or a config mediaType")]
    MissingArtifactType,
}

/// One reference from a manifest to bytes the registry must already hold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Descriptor {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    pub digest: Digest,
    pub size: u64,
}

/// A pushed image manifest or image index, parsed down to what the registry
/// has to act on: what it claims to be, and what it points at.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    #[serde(default)]
    schema_version: u64,
    #[serde(default)]
    media_type: Option<String>,
    #[serde(default)]
    config: Option<Descriptor>,
    #[serde(default)]
    layers: Vec<Descriptor>,
    #[serde(default)]
    manifests: Vec<Descriptor>,
    #[serde(default)]
    subject: Option<Descriptor>,
    #[serde(default)]
    artifact_type: Option<String>,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
}

impl Manifest {
    /// Parse and validate `bytes`, resolving the manifest's media type
    /// against `content_type` from the request when the document declares
    /// none of its own.
    pub fn parse(bytes: &[u8], content_type: Option<&str>) -> Result<Self, ManifestError> {
        let mut manifest: Self = serde_json::from_slice(bytes)
            .map_err(|error| ManifestError::Malformed { reason: error.to_string() })?;
        if manifest.schema_version != SCHEMA_VERSION {
            return Err(ManifestError::UnsupportedSchemaVersion {
                version: manifest.schema_version,
            });
        }
        if manifest.media_type.is_none() {
            manifest.media_type = content_type.map(ToString::to_string);
        }
        let media_type =
            manifest.media_type.get_or_insert_with(|| media_type::DEFAULT_MANIFEST.to_string());
        if !crate::MANIFEST_MEDIA_TYPES.contains(&media_type.as_str()) {
            return Err(ManifestError::UnsupportedMediaType { media_type: media_type.clone() });
        }
        if !media_type::is_index(media_type) && manifest.config.is_none() {
            return Err(ManifestError::MissingConfig);
        }
        if manifest.subject.is_some()
            && !media_type::is_index(manifest.media_type())
            && manifest.artifact_type().is_none()
        {
            return Err(ManifestError::MissingArtifactType);
        }
        Ok(manifest)
    }

    /// The media type the manifest is stored and served as. Always set once
    /// [`Self::parse`] has run.
    #[must_use]
    pub fn media_type(&self) -> &str {
        self.media_type.as_deref().unwrap_or(media_type::DEFAULT_MANIFEST)
    }

    #[must_use]
    pub fn referrer_metadata(&self) -> crate::ReferrerMetadata {
        let subject = self.subject.as_ref().map(|subject| subject.digest.clone());
        let artifact_type_digest = subject
            .as_ref()
            .and_then(|_| self.artifact_type())
            .map(|value| Digest::of(value.as_bytes()));
        crate::ReferrerMetadata { subject, artifact_type_digest }
    }

    /// An image without an artifact type uses its config media type. An index
    /// has no fallback artifact type.
    #[must_use]
    pub fn artifact_type(&self) -> Option<&str> {
        self.artifact_type.as_deref().filter(|value| !value.is_empty()).or_else(|| {
            if media_type::is_index(self.media_type()) {
                return None;
            }
            self.config
                .as_ref()
                .and_then(|config| config.media_type.as_deref())
                .filter(|value| !value.is_empty())
        })
    }

    #[must_use]
    pub fn annotations(&self) -> &BTreeMap<String, String> {
        &self.annotations
    }

    /// Every digest that must already be in the store for this manifest to
    /// be a complete release: an index's child manifests, or an image's
    /// config and layers. A `subject` is deliberately absent, because the
    /// spec lets a referrer name one the registry does not hold.
    pub fn references(&self) -> impl Iterator<Item = &Descriptor> {
        self.manifests.iter().chain(self.config.iter()).chain(self.layers.iter())
    }
}
