use super::{
    ArtifactBlobRequest, ArtifactBlobUpload, ArtifactCandidate, ArtifactFile, ArtifactManifest,
    ArtifactPayload, ArtifactProtocolError, ArtifactSubject, BASE64, BTreeMap, BuilderProfile,
    HashSet, MAX_ARTIFACT_SIZE, MAX_ENCODED_FILE_SIZE, MAX_FILE_SIZE, MAX_MANIFEST_FILES,
    OwnerScope, PackageIdentity, PublishArtifactRequest, Sha512, ValidatedArtifactPublication,
    validate_compatibility,
};
use base64::Engine as _;
use sha2::Digest as _;

/// Decode one uploaded blob and check it against what the signed manifest
/// declares for it.
fn decode_uploaded_blob(
    blob: &ArtifactBlobUpload,
    expected_size: u64,
) -> Result<Vec<u8>, ArtifactProtocolError> {
    let invalid = |reason: String| ArtifactProtocolError::InvalidBlobIntegrity(reason);
    if blob.data.len() > MAX_ENCODED_FILE_SIZE {
        return Err(invalid(format!("blob {:?} exceeds the encoded size limit", blob.integrity)));
    }
    let bytes = BASE64
        .decode(&blob.data)
        .map_err(|_| invalid(format!("blob {:?} is not valid base64", blob.integrity)))?;
    if BASE64.encode(&bytes) != blob.data {
        return Err(invalid(format!("blob {:?} is not canonical base64", blob.integrity)));
    }
    if bytes.len() as u64 != expected_size {
        return Err(invalid(format!(
            "blob {:?} has {} bytes but the signed manifest declares {expected_size}",
            blob.integrity,
            bytes.len(),
        )));
    }
    verify_blob(&blob.integrity, &bytes)?;
    Ok(bytes)
}

/// Check one added file's mode, size and integrity. A blob named twice must
/// be declared with one size.
fn validate_added_file<'a>(
    file: &'a ArtifactFile,
    integrity_sizes: &mut BTreeMap<&'a String, u64>,
) -> Result<(), ArtifactProtocolError> {
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
    blob_id(&file.integrity)?;
    if let Some(previous_size) = integrity_sizes.insert(&file.integrity, file.size)
        && previous_size != file.size
    {
        return Err(ArtifactProtocolError::InvalidManifest(format!(
            "blob integrity {:?} is declared with inconsistent sizes",
            file.integrity,
        )));
    }
    Ok(())
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

pub(super) fn validate_scalar(
    label: &str,
    value: &str,
    max_len: usize,
) -> Result<(), ArtifactProtocolError> {
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

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut output, byte| {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("writing to a String cannot fail");
        output
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
        let blobs = self.validate_uploaded_blobs(&required)?;

        Ok(ValidatedArtifactPublication { payload, blobs })
    }

    pub(super) fn validate_uploaded_blobs(
        &self,
        required: &BTreeMap<&str, u64>,
    ) -> Result<BTreeMap<String, Vec<u8>>, ArtifactProtocolError> {
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
            let bytes = decode_uploaded_blob(blob, expected_size)?;
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
        Ok(blobs)
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
    pub(super) fn validate(&self, owner: &OwnerScope) -> Result<(), ArtifactProtocolError> {
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
            validate_added_file(file, &mut integrity_sizes)?;
            total_size = total_size.checked_add(file.size).ok_or_else(|| {
                ArtifactProtocolError::InvalidManifest("artifact size overflow".to_string())
            })?;
            if total_size > MAX_ARTIFACT_SIZE {
                return Err(ArtifactProtocolError::InvalidManifest(format!(
                    "artifact exceeds the {MAX_ARTIFACT_SIZE}-byte size limit",
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
