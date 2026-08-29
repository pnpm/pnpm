//! Open wire types and validation for pnpm's shared artifact protocol.
//!
//! The signed payload is transported as base64-encoded JSON bytes. Signatures
//! cover those exact bytes, so implementations do not need to agree on a JSON
//! canonicalization algorithm before they can interoperate.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use derive_more::{Display, Error};
use p256::{
    ecdsa::{
        Signature, SigningKey, VerifyingKey,
        signature::{Signer as _, Verifier as _},
    },
    pkcs8::{DecodePrivateKey as _, DecodePublicKey as _},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256, Sha512};

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

impl SignedArtifactEnvelope {
    pub fn sign(
        payload: &ArtifactPayload,
        key_id: impl Into<String>,
        private_key_pkcs8: &[u8],
    ) -> Result<Self, ArtifactProtocolError> {
        payload.validate()?;
        let key_id = key_id.into();
        validate_scalar("key id", &key_id, 256)?;
        let payload_bytes = serde_json::to_vec(payload).map_err(|error| {
            ArtifactProtocolError::InvalidEnvelope(format!(
                "payload could not be serialized: {error}",
            ))
        })?;
        if payload_bytes.len() > MAX_SIGNED_PAYLOAD_SIZE {
            return Err(ArtifactProtocolError::InvalidEnvelope(format!(
                "signed payload exceeds {MAX_SIGNED_PAYLOAD_SIZE} bytes",
            )));
        }
        let private_key = SigningKey::from_pkcs8_der(private_key_pkcs8).map_err(|_| {
            ArtifactProtocolError::InvalidEnvelope(
                "private key is not PKCS#8-encoded P-256 key material".to_string(),
            )
        })?;
        let signature: Signature = private_key.sign(&payload_bytes);
        Ok(Self {
            algorithm: SIGNATURE_ALGORITHM.to_string(),
            key_id,
            payload: BASE64.encode(payload_bytes),
            signature: BASE64.encode(signature.to_der().as_bytes()),
        })
    }

    pub fn decode_payload(&self) -> Result<(ArtifactPayload, Vec<u8>), ArtifactProtocolError> {
        let payload_bytes = self.decode_payload_bytes()?;
        let payload = decode_payload_json(&payload_bytes)?;
        payload.validate()?;
        Ok((payload, payload_bytes))
    }

    fn decode_payload_bytes(&self) -> Result<Vec<u8>, ArtifactProtocolError> {
        validate_scalar("signature algorithm", &self.algorithm, 64)?;
        if self.algorithm != SIGNATURE_ALGORITHM {
            return Err(ArtifactProtocolError::InvalidEnvelope(format!(
                "unsupported signature algorithm {:?}",
                self.algorithm,
            )));
        }
        validate_scalar("key id", &self.key_id, 256)?;
        if self.payload.len() > MAX_ENCODED_SIGNED_PAYLOAD_SIZE {
            return Err(ArtifactProtocolError::InvalidEnvelope(format!(
                "signed payload exceeds {MAX_SIGNED_PAYLOAD_SIZE} bytes",
            )));
        }
        let payload_bytes = BASE64.decode(&self.payload).map_err(|_| {
            ArtifactProtocolError::InvalidEnvelope("payload is not valid base64".to_string())
        })?;
        if BASE64.encode(&payload_bytes) != self.payload {
            return Err(ArtifactProtocolError::InvalidEnvelope(
                "payload is not canonical base64".to_string(),
            ));
        }
        if payload_bytes.len() > MAX_SIGNED_PAYLOAD_SIZE {
            return Err(ArtifactProtocolError::InvalidEnvelope(format!(
                "signed payload exceeds {MAX_SIGNED_PAYLOAD_SIZE} bytes",
            )));
        }
        Ok(payload_bytes)
    }

    pub fn verify(&self, public_key_spki: &[u8]) -> Result<ArtifactPayload, ArtifactProtocolError> {
        let payload = self.verify_signature(public_key_spki)?;
        payload.validate()?;
        Ok(payload)
    }

    pub fn verify_signature(
        &self,
        public_key_spki: &[u8],
    ) -> Result<ArtifactPayload, ArtifactProtocolError> {
        let payload_bytes = self.verify_signature_bytes(public_key_spki)?;
        decode_payload_json(&payload_bytes)
    }

    pub fn verify_signature_bytes(
        &self,
        public_key_spki: &[u8],
    ) -> Result<Vec<u8>, ArtifactProtocolError> {
        let payload_bytes = self.decode_payload_bytes()?;
        let (signature, _) = self.decode_signature()?;
        let public_key = VerifyingKey::from_public_key_der(public_key_spki)
            .map_err(|_| ArtifactProtocolError::InvalidSignature)?;
        public_key
            .verify(&payload_bytes, &signature)
            .map_err(|_| ArtifactProtocolError::InvalidSignature)?;
        Ok(payload_bytes)
    }

    pub fn digest(&self) -> Result<String, ArtifactProtocolError> {
        let payload_bytes = self.decode_payload_bytes()?;
        let (_, signature_bytes) = self.decode_signature()?;
        let mut hasher = Sha256::new();
        hasher.update(b"pnpm-shared-artifact-envelope-v1\0");
        hasher.update(self.algorithm.as_bytes());
        hasher.update([0]);
        hasher.update(self.key_id.as_bytes());
        hasher.update([0]);
        hasher.update(payload_bytes);
        hasher.update([0]);
        hasher.update(signature_bytes);
        Ok(hex(&hasher.finalize()))
    }

    fn decode_signature(&self) -> Result<(Signature, Vec<u8>), ArtifactProtocolError> {
        if self.signature.len() > MAX_ENCODED_SIGNATURE_SIZE {
            return Err(ArtifactProtocolError::InvalidEnvelope(
                "signature is not a DER-encoded P-256 signature".to_string(),
            ));
        }
        let signature_bytes = BASE64.decode(&self.signature).map_err(|_| {
            ArtifactProtocolError::InvalidEnvelope("signature is not valid base64".to_string())
        })?;
        if BASE64.encode(&signature_bytes) != self.signature {
            return Err(ArtifactProtocolError::InvalidEnvelope(
                "signature is not canonical base64".to_string(),
            ));
        }
        let signature = Signature::from_der(&signature_bytes).map_err(|_| {
            ArtifactProtocolError::InvalidEnvelope(
                "signature is not a DER-encoded P-256 signature".to_string(),
            )
        })?;
        if signature.to_der().as_bytes() != signature_bytes {
            return Err(ArtifactProtocolError::InvalidEnvelope(
                "signature is not canonical DER".to_string(),
            ));
        }
        Ok((signature, signature_bytes))
    }
}

fn decode_payload_json(payload_bytes: &[u8]) -> Result<ArtifactPayload, ArtifactProtocolError> {
    serde_json::from_slice(payload_bytes).map_err(|error| {
        ArtifactProtocolError::InvalidEnvelope(format!("payload is not valid JSON: {error}"))
    })
}

impl PublishArtifactRequest {
    pub fn validate(&self) -> Result<ValidatedArtifactPublication, ArtifactProtocolError> {
        let (payload, _) = self.envelope.decode_payload()?;
        if payload.input_key != self.key {
            return Err(ArtifactProtocolError::InvalidEnvelope(
                "signed input key does not match the publication key".to_string(),
            ));
        }
        let required: BTreeMap<&str, u64> = payload
            .manifest
            .added
            .iter()
            .map(|file| (file.integrity.as_str(), file.size))
            .collect();
        let mut blobs = BTreeMap::new();
        let mut uploaded_size = 0_u64;
        for blob in &self.blobs {
            blob_id(&blob.integrity)?;
            if blobs.contains_key(&blob.integrity) {
                return Err(ArtifactProtocolError::InvalidEnvelope(format!(
                    "duplicate blob upload for {:?}",
                    blob.integrity,
                )));
            }
            let Some(&expected_size) = required.get(blob.integrity.as_str()) else {
                return Err(ArtifactProtocolError::InvalidEnvelope(format!(
                    "blob upload {:?} is not referenced by the signed manifest",
                    blob.integrity,
                )));
            };
            if blob.data.len() > MAX_ENCODED_FILE_SIZE {
                return Err(ArtifactProtocolError::InvalidBlobIntegrity(format!(
                    "blob {:?} exceeds the encoded size limit",
                    blob.integrity,
                )));
            }
            let bytes = BASE64.decode(&blob.data).map_err(|_| {
                ArtifactProtocolError::InvalidBlobIntegrity(format!(
                    "blob {:?} is not valid base64",
                    blob.integrity,
                ))
            })?;
            if BASE64.encode(&bytes) != blob.data {
                return Err(ArtifactProtocolError::InvalidBlobIntegrity(format!(
                    "blob {:?} is not canonical base64",
                    blob.integrity,
                )));
            }
            if bytes.len() as u64 != expected_size {
                return Err(ArtifactProtocolError::InvalidBlobIntegrity(format!(
                    "blob {:?} has {} bytes but the signed manifest declares {expected_size}",
                    blob.integrity,
                    bytes.len(),
                )));
            }
            verify_blob(&blob.integrity, &bytes)?;
            uploaded_size = uploaded_size.checked_add(bytes.len() as u64).ok_or_else(|| {
                ArtifactProtocolError::InvalidBlobIntegrity(
                    "uploaded blob size overflow".to_string(),
                )
            })?;
            if uploaded_size > MAX_ARTIFACT_SIZE {
                return Err(ArtifactProtocolError::InvalidBlobIntegrity(format!(
                    "uploaded blobs exceed the {MAX_ARTIFACT_SIZE}-byte artifact limit",
                )));
            }
            blobs.insert(blob.integrity.clone(), bytes);
        }
        Ok(ValidatedArtifactPublication { payload, blobs })
    }
}

impl ArtifactPayload {
    pub fn validate(&self) -> Result<(), ArtifactProtocolError> {
        let (artifact_kind, input_key_prefix) = self.subject.artifact_kind_and_input_key_prefix();
        if self.kind != artifact_kind {
            return Err(ArtifactProtocolError::InvalidEnvelope(format!(
                "unsupported artifact kind {:?}",
                self.kind,
            )));
        }
        if !self.input_key.starts_with(input_key_prefix) {
            return Err(ArtifactProtocolError::InvalidEnvelope(format!(
                "input key must start with {input_key_prefix:?}",
            )));
        }
        validate_scalar("input key", &self.input_key, 4_096)?;
        validate_scalar("builder id", &self.builder_id, 256)?;
        validate_owner(&self.owner)?;
        self.subject.validate(&self.owner)?;
        validate_builder_profile(&self.builder_profile)?;
        validate_compatibility(&self.compatibility)?;
        self.manifest.validate()
    }
}

impl ArtifactCandidate {
    pub fn validate(&self) -> Result<(), ArtifactProtocolError> {
        let (_, input_key_prefix) = self.subject.artifact_kind_and_input_key_prefix();
        if !self.key.starts_with(input_key_prefix) {
            return Err(ArtifactProtocolError::InvalidEnvelope(format!(
                "input key must start with {input_key_prefix:?}",
            )));
        }
        validate_scalar("input key", &self.key, 4_096)?;
        validate_owner(&self.owner)?;
        self.subject.validate(&self.owner)
    }
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

    fn validate(&self, owner: &OwnerScope) -> Result<(), ArtifactProtocolError> {
        match self {
            Self::DependencySideEffects { package, source_integrity } => {
                package.validate()?;
                validate_scalar("source integrity", source_integrity, 1_024)?;
                validate_publisher_package(owner, package)
            }
            Self::WorkspaceTask { project, task } => {
                validate_scalar("workspace project", project, 4_096)?;
                validate_scalar("workspace task", task, 256)?;
                if matches!(owner, OwnerScope::Publisher { .. }) {
                    return Err(ArtifactProtocolError::InvalidEnvelope(
                        "workspace task artifacts require an organization owner".to_string(),
                    ));
                }
                Ok(())
            }
        }
    }
}

impl PackageIdentity {
    pub fn validate(&self) -> Result<(), ArtifactProtocolError> {
        validate_scalar("package name", &self.name, 256)?;
        validate_scalar("package version", &self.version, 256)
    }
}

impl ArtifactBlobRequest {
    pub fn validate(&self) -> Result<(), ArtifactProtocolError> {
        validate_owner(&self.owner)?;
        blob_id(&self.integrity)?;
        Ok(())
    }
}

impl ArtifactManifest {
    pub fn validate(&self) -> Result<(), ArtifactProtocolError> {
        let file_count = self.added.len().saturating_add(self.deleted.len());
        if file_count > MAX_MANIFEST_FILES {
            return Err(ArtifactProtocolError::InvalidManifest(format!(
                "manifest contains {file_count} paths; limit is {MAX_MANIFEST_FILES}",
            )));
        }
        let mut exact_paths = HashSet::with_capacity(file_count);
        let mut folded_paths = HashSet::with_capacity(file_count);
        let mut integrity_sizes = BTreeMap::new();
        let mut total_size = 0_u64;
        for file in &self.added {
            validate_manifest_path(&file.path)?;
            insert_unique_path(&file.path, &mut exact_paths, &mut folded_paths)?;
            if file.mode != 0o644 && file.mode != 0o755 {
                return Err(ArtifactProtocolError::InvalidManifest(format!(
                    "path {:?} has unsupported mode {:o}",
                    file.path, file.mode,
                )));
            }
            if file.size > MAX_FILE_SIZE {
                return Err(ArtifactProtocolError::InvalidManifest(format!(
                    "path {:?} exceeds the per-file size limit",
                    file.path,
                )));
            }
            total_size = total_size.checked_add(file.size).ok_or_else(|| {
                ArtifactProtocolError::InvalidManifest("artifact size overflow".to_string())
            })?;
            if total_size > MAX_ARTIFACT_SIZE {
                return Err(ArtifactProtocolError::InvalidManifest(format!(
                    "artifact exceeds the {MAX_ARTIFACT_SIZE}-byte size limit",
                )));
            }
            blob_id(&file.integrity)?;
            if let Some(previous_size) = integrity_sizes.insert(&file.integrity, file.size)
                && previous_size != file.size
            {
                return Err(ArtifactProtocolError::InvalidManifest(format!(
                    "blob integrity {:?} is declared with inconsistent sizes",
                    file.integrity,
                )));
            }
        }
        for path in &self.deleted {
            validate_manifest_path(path)?;
            insert_unique_path(path, &mut exact_paths, &mut folded_paths)?;
        }
        Ok(())
    }
}

pub fn validate_manifest_path(path: &str) -> Result<(), ArtifactProtocolError> {
    if path.is_empty() || path.len() > 4_096 {
        return Err(invalid_path(path, "path length is outside the allowed range"));
    }
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(invalid_path(path, "absolute paths are not allowed"));
    }
    if path.as_bytes().get(1) == Some(&b':') {
        return Err(invalid_path(path, "Windows drive paths are not allowed"));
    }
    if path.contains('\\') {
        return Err(invalid_path(path, "backslash separators are not allowed"));
    }
    if path.chars().any(char::is_control) {
        return Err(invalid_path(path, "control characters are not allowed"));
    }
    if path.split('/').any(|segment| {
        segment.is_empty()
            || segment == "."
            || segment == ".."
            || segment.contains(':')
            || is_windows_reserved_name(segment)
            || segment.ends_with('.')
            || segment.ends_with(' ')
    }) {
        return Err(invalid_path(
            path,
            "empty, dot, parent, and Windows-normalized segments are not allowed",
        ));
    }
    Ok(())
}

fn is_windows_reserved_name(segment: &str) -> bool {
    let basename = segment.split('.').next().unwrap_or(segment).to_ascii_lowercase();
    matches!(basename.as_str(), "con" | "prn" | "aux" | "nul")
        || ["com", "lpt"].iter().any(|prefix| {
            basename.strip_prefix(prefix).is_some_and(|suffix| {
                matches!(
                    suffix,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³",
                )
            })
        })
}

pub fn blob_id(integrity: &str) -> Result<String, ArtifactProtocolError> {
    let Some(encoded) = integrity.strip_prefix("sha512-") else {
        return Err(ArtifactProtocolError::InvalidBlobIntegrity(
            "only sha512 integrity values are accepted".to_string(),
        ));
    };
    if encoded.is_empty() || encoded.contains(char::is_whitespace) {
        return Err(ArtifactProtocolError::InvalidBlobIntegrity(
            "sha512 integrity is malformed".to_string(),
        ));
    }
    let digest = BASE64.decode(encoded).map_err(|_| {
        ArtifactProtocolError::InvalidBlobIntegrity("sha512 digest is not valid base64".to_string())
    })?;
    if digest.len() != 64 {
        return Err(ArtifactProtocolError::InvalidBlobIntegrity(format!(
            "sha512 digest is {} bytes instead of 64",
            digest.len(),
        )));
    }
    Ok(hex(&digest))
}

pub fn verify_blob(integrity: &str, bytes: &[u8]) -> Result<(), ArtifactProtocolError> {
    let expected = blob_id(integrity)?;
    let actual = hex(&Sha512::digest(bytes));
    if expected != actual {
        return Err(ArtifactProtocolError::InvalidBlobIntegrity(
            "downloaded bytes do not match the declared digest".to_string(),
        ));
    }
    Ok(())
}

#[must_use]
pub fn compatibility_rank(
    constraints: &CompatibilityConstraints,
    supported_tags: &[String],
) -> Option<u64> {
    validate_supported_tags(supported_tags).ok()?;
    compatibility_rank_prevalidated(constraints, supported_tags)
}

#[must_use]
pub fn compatibility_rank_prevalidated(
    constraints: &CompatibilityConstraints,
    supported_tags: &[String],
) -> Option<u64> {
    match constraints {
        CompatibilityConstraints::Universal => Some(u64::MAX),
        CompatibilityConstraints::Tagged { tags } => supported_tags
            .iter()
            .enumerate()
            .flat_map(|(index, supported)| {
                tags.iter().filter_map(move |artifact| {
                    if artifact == supported {
                        return u64::try_from(index).ok();
                    }
                    let distance = version_floor_rank(
                        parse_compatibility_tag(supported).ok()?,
                        parse_compatibility_tag(artifact).ok()?,
                    )?;
                    u64::try_from(index)
                        .ok()?
                        .checked_mul(COMPATIBILITY_FLOOR_RANK_STRIDE)?
                        .checked_add(COMPATIBILITY_FLOOR_RANK_OFFSET)?
                        .checked_add(distance)
                })
            })
            .min(),
    }
}

pub fn linux_glibc_tag(platform: LinuxGlibcPlatform<'_>) -> Result<String, ArtifactProtocolError> {
    let LinuxGlibcPlatform { architecture, node_major, glibc_major, glibc_minor } = platform;
    let tag = format!(
        "{COMPATIBILITY_TAG_SCHEMA}:linux-{architecture}-node{node_major}-glibc{glibc_major}.{glibc_minor}",
    );
    validate_compatibility_tag(&tag)?;
    Ok(tag)
}

pub fn linux_glibc_supported_tags(
    platform: LinuxGlibcPlatform<'_>,
) -> Result<Vec<String>, ArtifactProtocolError> {
    let LinuxGlibcPlatform { architecture, node_major, glibc_major, glibc_minor } = platform;
    let count = usize::try_from(glibc_minor)
        .ok()
        .and_then(|minor| minor.checked_add(1))
        .ok_or_else(|| invalid_tag("glibc minor version is too large"))?;
    if count > 64 {
        return Err(invalid_tag("glibc floor expansion exceeds 64 tags"));
    }
    (0..=glibc_minor)
        .rev()
        .map(|minor| {
            linux_glibc_tag(LinuxGlibcPlatform {
                architecture,
                node_major,
                glibc_major,
                glibc_minor: minor,
            })
        })
        .collect()
}

pub fn macos_tag(platform: MacOsPlatform<'_>) -> Result<String, ArtifactProtocolError> {
    let MacOsPlatform { architecture, node_major, macos_major, macos_minor } = platform;
    let tag = format!(
        "{COMPATIBILITY_TAG_SCHEMA}:darwin-{architecture}-node{node_major}-macos{macos_major}.{macos_minor}",
    );
    validate_compatibility_tag(&tag)?;
    Ok(tag)
}

pub fn macos_supported_tags(
    platform: MacOsPlatform<'_>,
) -> Result<Vec<String>, ArtifactProtocolError> {
    Ok(vec![macos_tag(platform)?])
}

pub fn windows_tag(platform: WindowsPlatform<'_>) -> Result<String, ArtifactProtocolError> {
    let WindowsPlatform { architecture, node_major, windows_major, windows_minor, windows_build } =
        platform;
    let tag = format!(
        "{COMPATIBILITY_TAG_SCHEMA}:win32-{architecture}-node{node_major}-windows{windows_major}.{windows_minor}.{windows_build}",
    );
    validate_compatibility_tag(&tag)?;
    Ok(tag)
}

pub fn windows_supported_tags(
    platform: WindowsPlatform<'_>,
) -> Result<Vec<String>, ArtifactProtocolError> {
    Ok(vec![windows_tag(platform)?])
}

pub fn platform_fingerprint(supported_tags: &[String]) -> Result<String, ArtifactProtocolError> {
    validate_supported_tags(supported_tags)?;
    let mut hasher = Sha256::new();
    hasher.update(b"pnpm-platform-fingerprint-v1\0");
    for tag in supported_tags {
        hasher.update(tag.as_bytes());
        hasher.update([0]);
    }
    Ok(hex(&hasher.finalize()))
}

pub fn validate_supported_tags(tags: &[String]) -> Result<(), ArtifactProtocolError> {
    if tags.len() > 64 {
        return Err(invalid_tag("consumer advertises more than 64 supported tags"));
    }
    let mut unique = HashSet::with_capacity(tags.len());
    for tag in tags {
        validate_compatibility_tag(tag)?;
        if !unique.insert(tag) {
            return Err(invalid_tag("consumer compatibility tags contain a duplicate"));
        }
    }
    Ok(())
}

fn validate_owner(owner: &OwnerScope) -> Result<(), ArtifactProtocolError> {
    match owner {
        OwnerScope::Organization { name } => validate_scalar("organization owner", name, 256),
        OwnerScope::Publisher { package } => validate_scalar("publisher owner", package, 256),
    }
}

fn validate_publisher_package(
    owner: &OwnerScope,
    package: &PackageIdentity,
) -> Result<(), ArtifactProtocolError> {
    if let OwnerScope::Publisher { package: owner_package } = owner
        && owner_package != &package.name
    {
        return Err(ArtifactProtocolError::InvalidEnvelope(
            "publisher owner does not match the signed package name".to_string(),
        ));
    }
    Ok(())
}

fn validate_builder_profile(profile: &BuilderProfile) -> Result<(), ArtifactProtocolError> {
    if let Some(image_digest) = profile.image_digest.as_deref() {
        validate_scalar("builder image digest", image_digest, 1_024)?;
    }
    validate_scalar("architecture baseline", &profile.architecture_baseline, 256)?;
    if profile.environment.len() > 128 {
        return Err(ArtifactProtocolError::InvalidEnvelope(
            "builder environment contains more than 128 variables".to_string(),
        ));
    }
    for (name, value) in &profile.environment {
        validate_scalar("builder environment name", name, 256)?;
        validate_scalar("builder environment value", value, 4_096)?;
    }
    Ok(())
}

fn validate_compatibility(
    compatibility: &CompatibilityConstraints,
) -> Result<(), ArtifactProtocolError> {
    let CompatibilityConstraints::Tagged { tags } = compatibility else { return Ok(()) };
    if tags.is_empty() || tags.len() > 64 {
        return Err(ArtifactProtocolError::InvalidEnvelope(
            "tagged compatibility must contain between 1 and 64 tags".to_string(),
        ));
    }
    let mut unique = HashSet::with_capacity(tags.len());
    for tag in tags {
        validate_compatibility_tag(tag)?;
        if !unique.insert(tag) {
            return Err(ArtifactProtocolError::InvalidEnvelope(format!(
                "duplicate compatibility tag {tag:?}",
            )));
        }
    }
    Ok(())
}

fn validate_compatibility_tag(tag: &str) -> Result<(), ArtifactProtocolError> {
    parse_compatibility_tag(tag).map(|_| ())
}

enum ParsedCompatibilityTag<'a> {
    Linux,
    MacOs(MacOsPlatform<'a>),
    Windows(WindowsPlatform<'a>),
}

/// A compatibility tag split into the dimensions that decide *which machines it
/// can apply to* and the `runtime` component carrying the version floor that
/// decides *how well* it fits one of them.
#[derive(Clone, Copy)]
struct CompatibilityTagParts<'a> {
    os: &'a str,
    architecture: &'a str,
    node_major: u32,
    runtime: &'a str,
}

fn split_compatibility_tag(tag: &str) -> Result<CompatibilityTagParts<'_>, ArtifactProtocolError> {
    validate_scalar("compatibility tag", tag, 512)?;
    let Some(platform) = tag.strip_prefix("pnpm:v1:") else {
        return Err(invalid_tag("unknown compatibility tag schema"));
    };
    let mut parts = platform.split('-');
    let (Some(os), Some(architecture), Some(node), Some(runtime), None) =
        (parts.next(), parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(invalid_tag("compatibility tag has the wrong number of dimensions"));
    };
    if !matches!(architecture, "x64" | "arm64") {
        return Err(invalid_tag("v1 only defines x64 and arm64 tags"));
    }
    let node_major = parse_canonical_number(
        node.strip_prefix("node").ok_or_else(|| invalid_tag("missing Node dimension"))?,
        "Node major version",
        false,
    )?;
    Ok(CompatibilityTagParts { os, architecture, node_major, runtime })
}

/// How an artifact's constraints divide the machines it reaches, as keys that
/// can be claimed one at a time.
///
/// Two artifacts reach a machine in common exactly when their key sets
/// intersect, so claiming a key per scope is what lets a registry refuse an
/// overlapping publication without reading every artifact stored beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompatibilityScopes {
    /// Reaches every machine there is, so its keys cannot be enumerated.
    Every,
    /// Reaches the machines these keys name, and no others.
    These(BTreeSet<String>),
}

/// The scopes an artifact's constraints reach.
///
/// A tag that describes no machine contributes nothing. Published artifacts
/// cannot carry one — [`ArtifactPayload::validate`] parses every tag — so for
/// anything stored this yields between 1 and 64 keys.
#[must_use]
pub fn compatibility_scopes(constraints: &CompatibilityConstraints) -> CompatibilityScopes {
    match constraints {
        CompatibilityConstraints::Universal => CompatibilityScopes::Every,
        CompatibilityConstraints::Tagged { tags } => CompatibilityScopes::These(
            tags.iter()
                .filter_map(|tag| {
                    let parts = applicable_dimensions(tag)?;
                    Some(format!("{}-{}-node{}", parts.os, parts.architecture, parts.node_major))
                })
                .collect(),
        ),
    }
}

/// The dimensions of a tag that describes some machine, or `None` for one that
/// describes none.
///
/// The floor has to parse as well as the dimensions: `linux-x64-node22-macos13.0`
/// splits into Linux dimensions while naming a macOS floor, so no consumer can
/// ever present it and it overlaps nothing.
fn applicable_dimensions(tag: &str) -> Option<CompatibilityTagParts<'_>> {
    let parts = split_compatibility_tag(tag).ok()?;
    parse_tag_floor(parts).ok()?;
    Some(parts)
}

fn parse_compatibility_tag(tag: &str) -> Result<ParsedCompatibilityTag<'_>, ArtifactProtocolError> {
    parse_tag_floor(split_compatibility_tag(tag)?)
}

fn parse_tag_floor(
    parts: CompatibilityTagParts<'_>,
) -> Result<ParsedCompatibilityTag<'_>, ArtifactProtocolError> {
    let CompatibilityTagParts { os, architecture, node_major, runtime } = parts;
    match os {
        "linux" => {
            let libc =
                runtime.strip_prefix("glibc").ok_or_else(|| invalid_tag("missing glibc floor"))?;
            let (major, minor) = libc
                .split_once('.')
                .ok_or_else(|| invalid_tag("glibc floor must be major.minor"))?;
            parse_canonical_number(major, "glibc major version", false)?;
            parse_canonical_number(minor, "glibc minor version", true)?;
            Ok(ParsedCompatibilityTag::Linux)
        }
        "darwin" => {
            let macos =
                runtime.strip_prefix("macos").ok_or_else(|| invalid_tag("missing macOS floor"))?;
            let (major, minor) = macos
                .split_once('.')
                .ok_or_else(|| invalid_tag("macOS floor must be major.minor"))?;
            let macos_major = parse_macos_version_component(major, "macOS major version", false)?;
            let macos_minor = parse_macos_version_component(minor, "macOS minor version", true)?;
            Ok(ParsedCompatibilityTag::MacOs(MacOsPlatform {
                architecture,
                node_major,
                macos_major,
                macos_minor,
            }))
        }
        "win32" => {
            let windows = runtime
                .strip_prefix("windows")
                .ok_or_else(|| invalid_tag("missing Windows floor"))?;
            let mut components = windows.split('.');
            let (Some(major), Some(minor), Some(build), None) =
                (components.next(), components.next(), components.next(), components.next())
            else {
                return Err(invalid_tag("Windows floor must be major.minor.build"));
            };
            let windows_major =
                parse_windows_version_component(major, "Windows major version", false, 1_000)?;
            let windows_minor =
                parse_windows_version_component(minor, "Windows minor version", true, 1_000)?;
            let windows_build =
                parse_windows_version_component(build, "Windows build number", false, 1_000_000)?;
            Ok(ParsedCompatibilityTag::Windows(WindowsPlatform {
                architecture,
                node_major,
                windows_major,
                windows_minor,
                windows_build,
            }))
        }
        _ => Err(invalid_tag("v1 only defines Linux, macOS, and Windows tags")),
    }
}

fn parse_macos_version_component(
    value: &str,
    label: &str,
    allow_zero: bool,
) -> Result<u32, ArtifactProtocolError> {
    let number = parse_canonical_number(value, label, allow_zero)?;
    if number >= 1_000_000 {
        return Err(invalid_tag(&format!("{label} is too large")));
    }
    Ok(number)
}

fn macos_version_rank(platform: MacOsPlatform<'_>) -> u64 {
    u64::from(platform.macos_major) * 1_000_000 + u64::from(platform.macos_minor)
}

fn parse_windows_version_component(
    value: &str,
    label: &str,
    allow_zero: bool,
    exclusive_maximum: u32,
) -> Result<u32, ArtifactProtocolError> {
    let number = parse_canonical_number(value, label, allow_zero)?;
    if number >= exclusive_maximum {
        return Err(invalid_tag(&format!("{label} is too large")));
    }
    Ok(number)
}

fn windows_version_rank(platform: WindowsPlatform<'_>) -> u64 {
    u64::from(platform.windows_major) * 1_000_000_000
        + u64::from(platform.windows_minor) * 1_000_000
        + u64::from(platform.windows_build)
}

fn version_floor_rank(
    consumer: ParsedCompatibilityTag<'_>,
    artifact: ParsedCompatibilityTag<'_>,
) -> Option<u64> {
    match (consumer, artifact) {
        (ParsedCompatibilityTag::MacOs(consumer), ParsedCompatibilityTag::MacOs(artifact))
            if consumer.architecture == artifact.architecture
                && consumer.node_major == artifact.node_major =>
        {
            macos_version_rank(consumer).checked_sub(macos_version_rank(artifact))
        }
        (ParsedCompatibilityTag::Windows(consumer), ParsedCompatibilityTag::Windows(artifact))
            if consumer.architecture == artifact.architecture
                && consumer.node_major == artifact.node_major =>
        {
            windows_version_rank(consumer).checked_sub(windows_version_rank(artifact))
        }
        _ => None,
    }
}

fn parse_canonical_number(
    value: &str,
    label: &str,
    allow_zero: bool,
) -> Result<u32, ArtifactProtocolError> {
    let number = value.parse::<u32>().map_err(|_| invalid_tag(&format!("invalid {label}")))?;
    if number.to_string() != value || (!allow_zero && number == 0) {
        return Err(invalid_tag(&format!("non-canonical {label}")));
    }
    Ok(number)
}

fn invalid_tag(reason: &str) -> ArtifactProtocolError {
    ArtifactProtocolError::InvalidEnvelope(format!("invalid compatibility tag: {reason}"))
}

fn validate_scalar(label: &str, value: &str, max_len: usize) -> Result<(), ArtifactProtocolError> {
    if value.is_empty() || value.len() > max_len || value.chars().any(char::is_control) {
        return Err(ArtifactProtocolError::InvalidEnvelope(format!(
            "{label} is empty, too long, or contains a control character",
        )));
    }
    Ok(())
}

fn insert_unique_path(
    path: &str,
    exact_paths: &mut HashSet<String>,
    folded_paths: &mut HashSet<String>,
) -> Result<(), ArtifactProtocolError> {
    if !exact_paths.insert(path.to_string()) {
        return Err(invalid_path(path, "duplicate path"));
    }
    if !folded_paths.insert(path.to_lowercase()) {
        return Err(invalid_path(path, "path collides on a case-insensitive filesystem"));
    }
    Ok(())
}

fn invalid_path(path: &str, reason: &str) -> ArtifactProtocolError {
    ArtifactProtocolError::InvalidManifest(format!("unsafe path {path:?}: {reason}"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut output, byte| {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("writing to a String cannot fail");
        output
    })
}

#[cfg(test)]
mod tests;
