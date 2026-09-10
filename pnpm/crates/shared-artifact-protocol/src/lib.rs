//! Open wire types and validation for pnpm's shared artifact protocol.
//!
//! The signed payload is transported as base64-encoded JSON bytes. Signatures
//! cover those exact bytes, so implementations do not need to agree on a JSON
//! canonicalization algorithm before they can interoperate.

pub use compatibility::{
    CompatibilityScopes, compatibility_rank, compatibility_rank_prevalidated, compatibility_scopes,
    linux_glibc_supported_tags, linux_glibc_tag, macos_supported_tags, macos_tag,
    platform_fingerprint, validate_supported_tags, windows_supported_tags, windows_tag,
};
pub use validation::{blob_id, validate_manifest_path, verify_blob};

use std::collections::{BTreeMap, BTreeSet, HashSet};

use base64::engine::general_purpose::STANDARD as BASE64;
use derive_more::{Display, Error};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Sha512};

pub const DEPENDENCY_SIDE_EFFECTS_ARTIFACT_KIND: &str = "dependency-side-effects:v1";
pub const DEPENDENCY_SIDE_EFFECTS_INPUT_KEY_PREFIX: &str = "dependency-side-effects:v1:";
pub const WORKSPACE_TASK_ARTIFACT_KIND: &str = "workspace-task:v1";
pub const WORKSPACE_TASK_INPUT_KEY_PREFIX: &str = "workspace-task:v1:";
pub const ARTIFACT_KIND: &str = DEPENDENCY_SIDE_EFFECTS_ARTIFACT_KIND;
pub const INPUT_KEY_PREFIX: &str = DEPENDENCY_SIDE_EFFECTS_INPUT_KEY_PREFIX;
pub const COMPATIBILITY_TAG_SCHEMA: &str = "pnpm:v1";
pub const SIGNATURE_ALGORITHM: &str = "ecdsa-p256-sha256";
pub const MAX_CANDIDATES: usize = 2_048;
pub const MAX_VARIANTS_PER_CANDIDATE: usize = 8;
pub const MAX_MANIFEST_FILES: usize = 10_000;
pub const MAX_FILE_SIZE: u64 = 64 * 1024 * 1024;
pub const MAX_ARTIFACT_SIZE: u64 = 64 * 1024 * 1024;
pub const MAX_ENCODED_FILE_SIZE: usize = (MAX_FILE_SIZE as usize).div_ceil(3) * 4;
pub const MAX_SIGNED_PAYLOAD_SIZE: usize = 2 * 1024 * 1024;
/// A canonical DER-encoded P-256 signature is a SEQUENCE of two INTEGERs of at
/// most 33 content bytes each, so it never exceeds 72 bytes.
pub const MAX_SIGNATURE_SIZE: usize = 72;
pub const MAX_ENCODED_SIGNED_PAYLOAD_SIZE: usize = MAX_SIGNED_PAYLOAD_SIZE.div_ceil(3) * 4;
pub const MAX_ENCODED_SIGNATURE_SIZE: usize = MAX_SIGNATURE_SIZE.div_ceil(3) * 4;
pub const MAX_RESOLVE_RESPONSE_SIZE: usize = 16 * 1024 * 1024;
const COMPATIBILITY_FLOOR_RANK_OFFSET: u64 = 64;
const COMPATIBILITY_FLOOR_RANK_STRIDE: u64 = 1_000_000_000_000;

#[derive(Debug, Display, Error)]
pub enum ArtifactProtocolError {
    #[display("invalid artifact envelope: {_0}")]
    InvalidEnvelope(#[error(not(source))] String),
    #[display("invalid artifact manifest: {_0}")]
    InvalidManifest(#[error(not(source))] String),
    #[display("artifact signature verification failed")]
    InvalidSignature,
    #[display("invalid artifact blob integrity: {_0}")]
    InvalidBlobIntegrity(#[error(not(source))] String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum OwnerScope {
    Organization { name: String },
    Publisher { package: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageIdentity {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ArtifactSubject {
    #[serde(rename = "dependency-side-effects")]
    DependencySideEffects {
        package: PackageIdentity,
        #[serde(rename = "sourceIntegrity")]
        source_integrity: String,
    },
    #[serde(rename = "workspace-task")]
    WorkspaceTask { project: String, task: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinuxGlibcPlatform<'a> {
    pub architecture: &'a str,
    pub node_major: u32,
    pub glibc_major: u32,
    pub glibc_minor: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacOsPlatform<'a> {
    pub architecture: &'a str,
    pub node_major: u32,
    pub macos_major: u32,
    pub macos_minor: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsPlatform<'a> {
    pub architecture: &'a str,
    pub node_major: u32,
    pub windows_major: u32,
    pub windows_minor: u32,
    pub windows_build: u32,
}

impl OwnerScope {
    #[must_use]
    pub fn organization(name: impl Into<String>) -> Self {
        Self::Organization { name: name.into() }
    }

    #[must_use]
    pub fn namespace(&self) -> String {
        match self {
            Self::Organization { name } => format!("organization:{name}"),
            Self::Publisher { package } => format!("publisher:{package}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CompatibilityConstraints {
    Universal,
    Tagged { tags: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderProfile {
    pub image_digest: Option<String>,
    pub architecture_baseline: String,
    pub environment: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactFile {
    pub path: String,
    pub integrity: String,
    pub mode: u32,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactManifest {
    pub added: Vec<ArtifactFile>,
    pub deleted: Vec<String>,
}

impl ArtifactManifest {
    /// Whether the artifact names no added and no deleted files, so
    /// restoring it would change nothing inside the package directory.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.deleted.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactPayload {
    pub kind: String,
    pub subject: ArtifactSubject,
    pub input_key: String,
    pub owner: OwnerScope,
    pub builder_id: String,
    pub builder_profile: BuilderProfile,
    pub compatibility: CompatibilityConstraints,
    pub manifest: ArtifactManifest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedArtifactEnvelope {
    pub algorithm: String,
    pub key_id: String,
    pub payload: String,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactCandidate {
    pub key: String,
    pub subject: ArtifactSubject,
    pub owner: OwnerScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveArtifactsRequest {
    pub candidates: Vec<ArtifactCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactVariant {
    pub envelope: SignedArtifactEnvelope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedArtifact {
    pub key: String,
    pub variants: Vec<ArtifactVariant>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveArtifactsResponse {
    pub artifacts: Vec<ResolvedArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactBlobUpload {
    pub integrity: String,
    pub data: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishArtifactRequest {
    pub key: String,
    pub envelope: SignedArtifactEnvelope,
    pub blobs: Vec<ArtifactBlobUpload>,
}

pub struct ValidatedArtifactPublication {
    pub payload: ArtifactPayload,
    pub blobs: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactBlobRequest {
    pub owner: OwnerScope,
    pub integrity: String,
}

impl ArtifactSubject {
    #[must_use]
    pub fn dependency_side_effects(
        package: PackageIdentity,
        source_integrity: impl Into<String>,
    ) -> Self {
        Self::DependencySideEffects { package, source_integrity: source_integrity.into() }
    }

    #[must_use]
    pub fn workspace_task(project: impl Into<String>, task: impl Into<String>) -> Self {
        Self::WorkspaceTask { project: project.into(), task: task.into() }
    }

    fn artifact_kind_and_input_key_prefix(&self) -> (&'static str, &'static str) {
        match self {
            Self::DependencySideEffects { .. } => {
                (DEPENDENCY_SIDE_EFFECTS_ARTIFACT_KIND, DEPENDENCY_SIDE_EFFECTS_INPUT_KEY_PREFIX)
            }
            Self::WorkspaceTask { .. } => {
                (WORKSPACE_TASK_ARTIFACT_KIND, WORKSPACE_TASK_INPUT_KEY_PREFIX)
            }
        }
    }
}

#[cfg(test)]
mod tests;

mod signatures;

mod compatibility;

use compatibility::validate_compatibility;

mod validation;

use validation::{hex, validate_scalar};
