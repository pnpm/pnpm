use super::{
    ArtifactBlobRequest, ArtifactCandidate, ArtifactPayload, ArtifactSubject, ArtifactVariant,
    BTreeMap, Duration, HashSet, MAX_CANDIDATES, MAX_FILE_SIZE, MAX_RESOLVE_RESPONSE_SIZE,
    MAX_VARIANTS_PER_CANDIDATE, PROTOCOL_VERSION, PnprClient, PnprClientError,
    PublishArtifactRequest, ResolveArtifactsRequest, ResolveArtifactsResponse, ResolvedArtifact,
    SignedArtifactEnvelope, compatibility_rank_prevalidated, response_body_bounded,
    validate_supported_tags, verify_blob,
};

/// Inputs to the signed shared-artifact lookup `PoC`.
pub struct ResolveArtifactsOptions {
    pub candidates: Vec<ArtifactCandidate>,
    /// Most preferred compatibility tag first.
    pub supported_tags: Vec<String>,
    /// Package names that passed the configured remote-artifact eligibility
    /// policy.
    pub eligible_packages: HashSet<String>,
    /// Package names that passed pnpm's effective `allowBuild` policy.
    pub allowed_builds: HashSet<String>,
    /// The effective `--ignore-scripts` value. When true, no remote lookup is
    /// made because applying build output would violate the same policy that
    /// suppresses a local build.
    pub ignore_scripts: bool,
    /// P-256 `SubjectPublicKeyInfo` DER bytes keyed by the envelope's key id.
    pub trusted_keys: BTreeMap<String, Vec<u8>>,
    pub quarantined_envelope_digests: BTreeMap<String, HashSet<String>>,
    pub on_rejected_artifact: Option<std::sync::Arc<dyn Fn(RejectedArtifact) + Send + Sync>>,
    pub authorization: Option<String>,
}

#[derive(Clone)]
pub struct RejectedArtifact {
    pub input_key: String,
    pub envelope_digest: String,
    pub reason: String,
}

/// A variant whose signature, owner, input key, source integrity, manifest,
/// and compatibility constraints have all passed client-side validation.
pub struct VerifiedArtifact {
    pub payload: ArtifactPayload,
    pub envelope: SignedArtifactEnvelope,
    pub envelope_digest: String,
}

/// Match the TypeScript client's generous ceiling for large artifact transfers
/// while still letting a stalled pnpr fail the install or publication.
pub(super) const ARTIFACT_REQUEST_TIMEOUT: Duration = Duration::from_mins(10);

/// The candidate one response entry answers for. A repeated key, an
/// unrequested one, or an oversized variant list is a protocol error.
fn check_response_key<'a>(
    artifact: &ResolvedArtifact,
    candidates: &BTreeMap<&str, &'a ArtifactCandidate>,
    seen: &mut HashSet<String>,
) -> Result<&'a ArtifactCandidate, PnprClientError> {
    if !seen.insert(artifact.key.clone()) {
        return Err(PnprClientError::Protocol(format!(
            "shared artifact response repeats key {:?}",
            artifact.key,
        )));
    }
    let Some(candidate) = candidates.get(artifact.key.as_str()) else {
        return Err(PnprClientError::Protocol(format!(
            "shared artifact response returned a key that was not requested: {:?}",
            artifact.key,
        )));
    };
    if artifact.variants.len() > MAX_VARIANTS_PER_CANDIDATE {
        return Err(PnprClientError::Protocol(format!(
            "shared artifact response exceeds the per-key variant limit for {:?}",
            artifact.key,
        )));
    }
    Ok(candidate)
}

/// The requested candidates by key, rejecting a batch that is oversized,
/// malformed, or repeats a key.
fn index_candidates(
    candidates: &[ArtifactCandidate],
) -> Result<BTreeMap<&str, &ArtifactCandidate>, PnprClientError> {
    if candidates.len() > MAX_CANDIDATES {
        return Err(PnprClientError::Protocol(format!(
            "shared artifact lookup exceeds the {MAX_CANDIDATES}-candidate limit",
        )));
    }
    let mut by_key = BTreeMap::new();
    for candidate in candidates {
        candidate.validate().map_err(|err| PnprClientError::Protocol(err.to_string()))?;
        if by_key.insert(candidate.key.as_str(), candidate).is_some() {
            return Err(PnprClientError::Protocol(format!(
                "duplicate shared artifact candidate {:?}",
                candidate.key,
            )));
        }
    }
    Ok(by_key)
}

/// The most compatible signed variant of one candidate, or `None` when none
/// of them can be used. Ties break on the envelope digest so the choice does
/// not depend on the order the server answered in.
fn best_variant(
    variants: Vec<ArtifactVariant>,
    candidate: &ArtifactCandidate,
    opts: &ResolveArtifactsOptions,
) -> Result<Option<VerifiedArtifact>, PnprClientError> {
    let mut best: Option<(u64, String, VerifiedArtifact)> = None;
    for variant in variants {
        let Some(verified) = verify_variant(variant, candidate, opts)? else {
            continue;
        };
        let Some(rank) =
            compatibility_rank_prevalidated(&verified.payload.compatibility, &opts.supported_tags)
        else {
            continue;
        };
        let digest = verified.envelope_digest.clone();
        if best
            .as_ref()
            .is_none_or(|(best_rank, best_digest, _)| (rank, &digest) < (*best_rank, best_digest))
        {
            best = Some((rank, digest, verified));
        }
    }
    Ok(best.map(|(_, _, artifact)| artifact))
}

/// One variant, once its signature, quarantine status and payload have all
/// been checked. `None` for a variant this consumer cannot use; a rejected
/// payload is reported through `on_rejected_artifact` before it is dropped.
fn verify_variant(
    variant: ArtifactVariant,
    candidate: &ArtifactCandidate,
    opts: &ResolveArtifactsOptions,
) -> Result<Option<VerifiedArtifact>, PnprClientError> {
    let Some(public_key) = opts.trusted_keys.get(&variant.envelope.key_id) else {
        return Ok(None);
    };
    let Ok(payload_bytes) = variant.envelope.verify_signature_bytes(public_key) else {
        return Ok(None);
    };
    let envelope_digest =
        variant.envelope.digest().map_err(|err| PnprClientError::Protocol(err.to_string()))?;
    let quarantined = opts
        .quarantined_envelope_digests
        .get(candidate.key.as_str())
        .is_some_and(|digests| digests.contains(&envelope_digest));
    if quarantined {
        return Ok(None);
    }

    let reject = |reason: String| {
        if let Some(on_rejected_artifact) = &opts.on_rejected_artifact {
            on_rejected_artifact(RejectedArtifact {
                input_key: candidate.key.clone(),
                envelope_digest: envelope_digest.clone(),
                reason,
            });
        }
    };
    let payload: ArtifactPayload = match serde_json::from_slice(&payload_bytes) {
        Ok(payload) => payload,
        Err(error) => {
            reject(format!("payload is not valid JSON: {error}"));
            return Ok(None);
        }
    };
    if let Err(error) = payload.validate() {
        reject(error.to_string());
        return Ok(None);
    }
    if !artifact_matches_candidate(&payload, candidate) {
        return Ok(None);
    }
    Ok(Some(VerifiedArtifact { payload, envelope: variant.envelope, envelope_digest }))
}

fn artifact_matches_candidate(payload: &ArtifactPayload, candidate: &ArtifactCandidate) -> bool {
    let ArtifactCandidate { key: input_key, subject, owner } = candidate;
    payload.input_key == *input_key && payload.subject == *subject && payload.owner == *owner
}

impl PnprClient {
    /// Confirm that the server enabled the v0 signed-artifact `PoC`.
    pub async fn handshake_artifacts(&self) -> Result<(), PnprClientError> {
        let capability = self.fetch_handshake(Some(self.artifact_request_timeout)).await?;
        if !capability.artifacts.contains(&PROTOCOL_VERSION) {
            return Err(PnprClientError::Server(format!(
                "pnpr server does not advertise shared artifact protocol v{PROTOCOL_VERSION}",
            )));
        }
        Ok(())
    }

    /// Upload one already-signed organization artifact and all blobs that are
    /// not yet present in the owner's namespace.
    pub async fn publish_artifact(
        &self,
        request: &PublishArtifactRequest,
        authorization: Option<&str>,
    ) -> Result<(), PnprClientError> {
        request.validate().map_err(|err| PnprClientError::Protocol(err.to_string()))?;
        let mut put = self
            .http
            .put(format!("{}-/pnpr/v0/artifacts", self.base_url))
            .timeout(self.artifact_request_timeout)
            .json(request);
        if let Some(authorization) = authorization {
            put = put.header("authorization", authorization);
        }
        let response = put.send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response_body_bounded(response, 64 * 1024).await?;
            return Err(PnprClientError::Server(format!(
                "/-/pnpr/v0/artifacts returned {status}: {}",
                String::from_utf8_lossy(&body),
            )));
        }
        Ok(())
    }

    /// Resolve a batch and keep only variants signed by a configured key and
    /// compatible with this consumer. A malformed or untrusted variant is a
    /// cache miss; a malformed response envelope is a protocol error.
    pub async fn resolve_artifacts(
        &self,
        mut opts: ResolveArtifactsOptions,
    ) -> Result<BTreeMap<String, VerifiedArtifact>, PnprClientError> {
        validate_supported_tags(&opts.supported_tags)
            .map_err(|err| PnprClientError::Protocol(err.to_string()))?;
        if opts.ignore_scripts {
            return Ok(BTreeMap::new());
        }
        opts.candidates.retain(|candidate| {
            let ArtifactSubject::DependencySideEffects { package, .. } = &candidate.subject else {
                return false;
            };
            opts.eligible_packages.contains(&package.name)
                && opts.allowed_builds.contains(&package.name)
        });
        if opts.candidates.is_empty() {
            return Ok(BTreeMap::new());
        }
        let candidates = index_candidates(&opts.candidates)?;
        let response = self.post_resolve_artifacts(&opts).await?;
        if response.artifacts.len() > candidates.len() {
            return Err(PnprClientError::Protocol(
                "shared artifact response contains more entries than requested".to_string(),
            ));
        }

        let mut selected = BTreeMap::new();
        let mut response_keys = HashSet::new();
        for artifact in response.artifacts {
            let candidate = check_response_key(&artifact, &candidates, &mut response_keys)?;
            if let Some(best) = best_variant(artifact.variants, candidate, &opts)? {
                selected.insert(candidate.key.clone(), best);
            }
        }
        Ok(selected)
    }

    /// POST the batch and decode the response envelope. A non-success status
    /// or an oversized body is a server error, not a cache miss.
    pub(super) async fn post_resolve_artifacts(
        &self,
        opts: &ResolveArtifactsOptions,
    ) -> Result<ResolveArtifactsResponse, PnprClientError> {
        let request = ResolveArtifactsRequest { candidates: opts.candidates.clone() };
        let mut post = self
            .http
            .post(format!("{}-/pnpr/v0/artifacts/resolve", self.base_url))
            .timeout(self.artifact_request_timeout)
            .json(&request);
        if let Some(authorization) = opts.authorization.as_deref() {
            post = post.header("authorization", authorization);
        }
        let response = post.send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response_body_bounded(response, 64 * 1024).await?;
            return Err(PnprClientError::Server(format!(
                "/-/pnpr/v0/artifacts/resolve returned {status}: {}",
                String::from_utf8_lossy(&body),
            )));
        }
        let body = response_body_bounded(response, MAX_RESOLVE_RESPONSE_SIZE).await?;
        serde_json::from_slice(&body).map_err(|err| PnprClientError::Protocol(err.to_string()))
    }

    /// Download and recompute a selected manifest blob's SHA-512 before
    /// returning any bytes to the caller.
    pub async fn download_artifact_blob(
        &self,
        request: &ArtifactBlobRequest,
        authorization: Option<&str>,
    ) -> Result<Vec<u8>, PnprClientError> {
        request.validate().map_err(|err| PnprClientError::Protocol(err.to_string()))?;
        let mut post = self
            .http
            .post(format!("{}-/pnpr/v0/artifacts/blob", self.base_url))
            .timeout(self.artifact_request_timeout)
            .json(request);
        if let Some(authorization) = authorization {
            post = post.header("authorization", authorization);
        }
        let response = post.send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response_body_bounded(response, 64 * 1024).await?;
            return Err(PnprClientError::Server(format!(
                "/-/pnpr/v0/artifacts/blob returned {status}: {}",
                String::from_utf8_lossy(&body),
            )));
        }
        let bytes = response_body_bounded(response, MAX_FILE_SIZE as usize).await?;
        verify_blob(&request.integrity, &bytes)
            .map_err(|err| PnprClientError::Protocol(err.to_string()))?;
        Ok(bytes)
    }
}
