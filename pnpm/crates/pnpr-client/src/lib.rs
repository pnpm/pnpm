//! Client for pnpr's server-side resolver.
//!
//! Given a set of dependencies, it `POST`s them to `/-/pnpr/v0/resolve`, where
//! the server resolves against the client's registries, verifies the
//! input lockfile under the client's policy, and streams the result back
//! as NDJSON: one `package` frame per resolved tarball as the server's
//! tree walk yields it, then a terminal `done` frame carrying the full
//! lockfile (or an `error` / `violations` frame). The caller consumes
//! the `package` frames to begin fetching tarballs *while the server is
//! still resolving* ([pnpm/pnpm#12234](https://github.com/pnpm/pnpm/issues/12234)),
//! then fetches the rest in parallel like a normal install
//! ([pnpm/pnpm#12230](https://github.com/pnpm/pnpm/issues/12230)).
//!
//! The resolver itself is stateless: it materializes no store and the
//! `/resolve` endpoint persists no tarballs. Resolved tarballs are fetched
//! from upstream public URLs or, for a private proxied route, an upstream's
//! `/~<name>/` registry endpoint, which may cache them server-side under
//! its own private namespace. The opt-in shared-artifact `PoC` is a separate
//! stateful protocol surface.

use std::{
    collections::{BTreeMap, HashSet},
    time::Duration,
};

use derive_more::{Display, Error, From};
use futures_util::StreamExt as _;
use indexmap::IndexMap;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::{PackageExtension, RegistryDeclaration, ResolutionMode, TrustPolicy};
use pnpm_graph_hasher::hash_object_nullable_with_prefix;
use pnpm_lockfile::{Lockfile, TarballRevision};
use pnpm_lockfile_verification::{RenderedViolation, VerifyError};
use reqwest::Client;

pub use pnpm_shared_artifact_protocol::{
    ARTIFACT_KIND, ArtifactBlobRequest, ArtifactBlobUpload, ArtifactCandidate, ArtifactFile,
    ArtifactManifest, ArtifactPayload, BuilderProfile, COMPATIBILITY_TAG_SCHEMA,
    CompatibilityConstraints, INPUT_KEY_PREFIX, LinuxGlibcPlatform, MacOsPlatform, OwnerScope,
    PackageIdentity, PublishArtifactRequest, ResolveArtifactsRequest, SIGNATURE_ALGORITHM,
    SignedArtifactEnvelope, WindowsPlatform, blob_id, linux_glibc_supported_tags, linux_glibc_tag,
    macos_supported_tags, macos_tag, platform_fingerprint, windows_supported_tags, windows_tag,
};
use pnpm_shared_artifact_protocol::{
    MAX_CANDIDATES, MAX_FILE_SIZE, MAX_RESOLVE_RESPONSE_SIZE, MAX_VARIANTS_PER_CANDIDATE,
    ResolveArtifactsResponse, compatibility_rank_prevalidated, validate_supported_tags,
    verify_blob,
};

/// The `registries` a request declares, keyed by registry URL.
pub type RegistryDeclarations = BTreeMap<String, RegistryDeclaration>;
use serde::{Deserialize, Serialize};

/// Dependency map (`name` -> `version range`).
pub type DepMap = BTreeMap<String, String>;

/// A client bound to one pnpr server.
#[must_use]
pub struct PnprClient {
    http: Client,
    base_url: String,
    artifact_request_timeout: Duration,
}

/// Inputs for a single-project resolution.
#[derive(Clone)]
pub struct ResolveOptions {
    pub dependencies: DepMap,
    pub dev_dependencies: DepMap,
    pub optional_dependencies: DepMap,
    /// The client's default registry. The server resolves against this
    /// (and the registries declared alongside it) rather than its own
    /// configuration.
    pub registry: String,
    /// The client's named-registry aliases.
    /// The registries the client declares, keyed by URL, in the shape
    /// of the `registries` setting. The default registry is not among
    /// them: it travels as `registry`.
    pub registries: RegistryDeclarations,
    /// `Authorization` for the pnpr server's own URL (`None` if it needs
    /// none): identifies the caller to pnpr. The client never forwards its
    /// own registry credentials — pnpr selects upstream credentials from
    /// its route policy, so none are placed in the request body.
    pub authorization: Option<String>,
    /// The client's `overrides` (selector -> spec) as raw JSON, applied
    /// at resolve time server-side. Sent unresolved: `catalog:` references
    /// in them are resolved server-side against [`Self::catalogs`].
    pub overrides: Option<serde_json::Value>,
    /// The client's `patchedDependencies`, with paths replaced by their
    /// SHA-256 hashes. The server uses these to key patched snapshots;
    /// materialization and patch application remain client-side.
    pub patched_dependencies: Option<IndexMap<String, String>>,
    /// The client's manifest extensions, applied during server resolution.
    pub package_extensions: Option<IndexMap<String, PackageExtension>>,
    pub allow_unused_patches: bool,
    /// The client's workspace catalogs (`catalog:` / `catalogs:` from
    /// `pnpm-workspace.yaml`). The workspace the server reconstructs from
    /// this request carries no catalog sections, so without these it
    /// cannot resolve a `catalog:` specifier in either dependencies or
    /// overrides ([pnpm/pnpm#13232](https://github.com/pnpm/pnpm/issues/13232)).
    pub catalogs: Option<Catalogs>,
    /// The client's current values for the settings that shape the lockfile
    /// the server resolves. `None` is not `Some(false)`: it leaves the
    /// setting to the server, which takes the input lockfile's value on a
    /// frozen request and its own default otherwise — what a client too old
    /// to send them gets
    /// ([pnpm/pnpm#13389](https://github.com/pnpm/pnpm/issues/13389)).
    pub auto_install_peers: Option<bool>,
    pub dedupe_peers: Option<bool>,
    pub exclude_links_from_lockfile: Option<bool>,
    /// The client's existing on-disk lockfile, when present. Sent both
    /// as the verification target and the resolution-reuse seed.
    pub lockfile: Option<Lockfile>,
    /// Frozen (use the lockfile as-is) vs reuse-and-update resolution
    /// behavior. Does not affect whether the input lockfile is verified.
    pub frozen_lockfile: bool,
    /// `preferFrozenLockfile`. `Some(false)` forces the server to
    /// re-resolve; `None` lets it default to reuse.
    pub prefer_frozen_lockfile: Option<bool>,
    /// Refresh registry artifacts while retaining every locked package
    /// version.
    pub update_patches: bool,
    /// `ignoreManifestCheck`: skip the manifest ↔ lockfile freshness
    /// comparison during the frozen resolve.
    pub ignore_manifest_check: bool,
    /// The client's effective `trustLockfile`. When `true` the server
    /// skips verifying the input lockfile (it still reuses it for
    /// resolution), mirroring the local `--trust-lockfile` opt-out.
    pub trust_lockfile: bool,
    /// The client's `resolutionMode`. The server picks versions the way
    /// the client would, instead of falling back to its own default.
    pub resolution_mode: ResolutionMode,
    /// The client's verification policy. The server verifies the input
    /// lockfile under *this* policy (not its own) before resolving.
    pub minimum_release_age: Option<u64>,
    pub minimum_release_age_exclude: Option<Vec<String>>,
    pub minimum_release_age_ignore_missing_time: bool,
    pub trust_policy: TrustPolicy,
    pub trust_policy_exclude: Option<Vec<String>>,
    pub trust_policy_ignore_after: Option<u64>,
}

/// One workspace project sent to the pnpr resolver.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveProject {
    /// Importer directory relative to the lockfile directory, in POSIX form.
    pub dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub dependencies: DepMap,
    pub dev_dependencies: DepMap,
    pub optional_dependencies: DepMap,
}

/// Inputs for a multi-project workspace resolution.
#[derive(Clone)]
pub struct ResolveProjectsOptions {
    pub projects: Vec<ResolveProject>,
    pub registry: String,
    /// The registries the client declares, keyed by URL, in the shape
    /// of the `registries` setting. The default registry is not among
    /// them: it travels as `registry`.
    pub registries: RegistryDeclarations,
    pub authorization: Option<String>,
    pub overrides: Option<serde_json::Value>,
    pub patched_dependencies: Option<IndexMap<String, String>>,
    pub package_extensions: Option<IndexMap<String, PackageExtension>>,
    pub allow_unused_patches: bool,
    pub catalogs: Option<Catalogs>,
    pub auto_install_peers: Option<bool>,
    pub dedupe_peers: Option<bool>,
    pub exclude_links_from_lockfile: Option<bool>,
    pub lockfile: Option<Lockfile>,
    pub frozen_lockfile: bool,
    pub prefer_frozen_lockfile: Option<bool>,
    pub update_patches: bool,
    /// Regenerate derived lockfile metadata while retaining compatible pins.
    pub fix_lockfile: bool,
    pub ignore_manifest_check: bool,
    pub trust_lockfile: bool,
    /// See [`ResolveOptions::resolution_mode`].
    pub resolution_mode: ResolutionMode,
    pub minimum_release_age: Option<u64>,
    pub minimum_release_age_exclude: Option<Vec<String>>,
    pub minimum_release_age_ignore_missing_time: bool,
    pub trust_policy: TrustPolicy,
    pub trust_policy_exclude: Option<Vec<String>>,
    pub trust_policy_ignore_after: Option<u64>,
}

impl From<ResolveOptions> for ResolveProjectsOptions {
    fn from(opts: ResolveOptions) -> Self {
        Self {
            projects: vec![ResolveProject {
                dir: ".".to_string(),
                name: None,
                version: None,
                dependencies: opts.dependencies,
                dev_dependencies: opts.dev_dependencies,
                optional_dependencies: opts.optional_dependencies,
            }],
            registry: opts.registry,
            registries: opts.registries,
            authorization: opts.authorization,
            overrides: opts.overrides,
            patched_dependencies: opts.patched_dependencies,
            package_extensions: opts.package_extensions,
            allow_unused_patches: opts.allow_unused_patches,
            catalogs: opts.catalogs,
            auto_install_peers: opts.auto_install_peers,
            dedupe_peers: opts.dedupe_peers,
            exclude_links_from_lockfile: opts.exclude_links_from_lockfile,
            lockfile: opts.lockfile,
            frozen_lockfile: opts.frozen_lockfile,
            prefer_frozen_lockfile: opts.prefer_frozen_lockfile,
            update_patches: opts.update_patches,
            fix_lockfile: false,
            ignore_manifest_check: opts.ignore_manifest_check,
            trust_lockfile: opts.trust_lockfile,
            resolution_mode: opts.resolution_mode,
            minimum_release_age: opts.minimum_release_age,
            minimum_release_age_exclude: opts.minimum_release_age_exclude,
            minimum_release_age_ignore_missing_time: opts.minimum_release_age_ignore_missing_time,
            trust_policy: opts.trust_policy,
            trust_policy_exclude: opts.trust_policy_exclude,
            trust_policy_ignore_after: opts.trust_policy_ignore_after,
        }
    }
}

/// Inputs for `/-/pnpr/v0/verify-lockfile`, the resolution-free trust verdict
/// used by frozen restores that already know the local lockfile is fresh.
#[derive(Clone)]
pub struct VerifyLockfileOptions {
    pub registry: String,
    /// The registries the client declares, keyed by URL, in the shape
    /// of the `registries` setting. The default registry is not among
    /// them: it travels as `registry`.
    pub registries: RegistryDeclarations,
    pub authorization: Option<String>,
    pub overrides: Option<serde_json::Value>,
    pub lockfile: Lockfile,
    pub trust_lockfile: bool,
    pub minimum_release_age: Option<u64>,
    pub minimum_release_age_exclude: Option<Vec<String>>,
    pub minimum_release_age_ignore_missing_time: bool,
    pub trust_policy: TrustPolicy,
    pub trust_policy_exclude: Option<Vec<String>>,
    pub trust_policy_ignore_after: Option<u64>,
}

impl VerifyLockfileOptions {
    #[must_use]
    pub fn from_resolve_options(opts: &ResolveOptions) -> Option<Self> {
        Self::from_owned_resolve_projects_options(opts.clone().into())
    }

    #[must_use]
    pub fn from_resolve_projects_options(opts: &ResolveProjectsOptions) -> Option<Self> {
        Self::from_owned_resolve_projects_options(opts.clone())
    }

    fn from_owned_resolve_projects_options(opts: ResolveProjectsOptions) -> Option<Self> {
        Some(Self {
            registry: opts.registry,
            registries: opts.registries,
            authorization: opts.authorization,
            overrides: opts.overrides,
            lockfile: opts.lockfile?,
            trust_lockfile: opts.trust_lockfile,
            minimum_release_age: opts.minimum_release_age,
            minimum_release_age_exclude: opts.minimum_release_age_exclude,
            minimum_release_age_ignore_missing_time: opts.minimum_release_age_ignore_missing_time,
            trust_policy: opts.trust_policy,
            trust_policy_exclude: opts.trust_policy_exclude,
            trust_policy_ignore_after: opts.trust_policy_ignore_after,
        })
    }
}

/// Result of [`PnprClient::resolve`].
#[must_use]
pub struct ResolveOutcome {
    /// The resolved lockfile, ready for a headless install.
    pub lockfile: Lockfile,
    pub stats: Stats,
}

/// Resolution statistics from the response. Field names mirror the
/// server's camelCase JSON.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Stats {
    pub total_packages: u64,
}

/// One resolved tarball package, surfaced from a streamed `package`
/// frame as the server's resolution yields it. Carries exactly what the
/// caller needs to start fetching the tarball before the full lockfile
/// arrives.
#[derive(Debug, Clone)]
pub struct ResolvedPackage {
    /// Canonical `name@version` identifier.
    pub id: String,
    pub name: String,
    pub version: String,
    /// Subresource-integrity string (`sha512-...`).
    pub integrity: String,
    /// The resolver's `dist.tarball` URL.
    pub tarball: String,
    /// `dist.unpackedSize` from the server-side resolve, when the
    /// registry published one. Sizes the decompression buffer exactly
    /// and prioritizes the largest pending downloads when the
    /// connection pool is saturated.
    pub unpacked_size: Option<usize>,
    /// `dist.fileCount` from the server-side resolve, when the registry
    /// published one. The per-file term of the download priority's
    /// pipeline-work estimate.
    pub file_count: Option<usize>,
    /// Registry artifact revision, when the server resolved an immutable
    /// integrity-addressed artifact.
    pub revision: Option<TarballRevision>,
}

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
    pub pinned_envelope_digests: BTreeMap<String, String>,
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

#[derive(Debug, Display, Error, From)]
pub enum PnprClientError {
    #[display("pnpr request failed: {_0}")]
    Http(reqwest::Error),

    #[display("pnpr server error: {_0}")]
    #[from(ignore)]
    Server(#[error(not(source))] String),

    #[display("malformed pnpr response: {_0}")]
    #[from(ignore)]
    Protocol(#[error(not(source))] String),

    /// The server rejected the input lockfile under the client's
    /// verification policy. Carries the reconstructed [`VerifyError`]
    /// so the CLI aborts with the same diagnostic code (and breakdown)
    /// the local verification gate would have produced.
    #[display("{_0}")]
    Verification(VerifyError),

    #[display("{_0}")]
    Io(std::io::Error),
}

/// Protocol version this client speaks. The server advertises the
/// versions it supports at `GET /-/pnpr`; today only v0 exists.
const PROTOCOL_VERSION: u32 = 0;
/// Match the TypeScript client's generous ceiling for large artifact transfers
/// while still letting a stalled pnpr fail the install or publication.
const ARTIFACT_REQUEST_TIMEOUT: Duration = Duration::from_mins(10);

#[derive(Default, Deserialize)]
struct HandshakeResponse {
    #[serde(default)]
    pnpr: HandshakeCapability,
}

#[derive(Default, Deserialize)]
struct HandshakeCapability {
    #[serde(default)]
    versions: Vec<u32>,
    #[serde(default)]
    artifacts: Vec<u32>,
    #[serde(default, rename = "fixLockfile")]
    fix_lockfile: Vec<u32>,
}

impl PnprClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        let mut base_url = base_url.into();
        if !base_url.ends_with('/') {
            base_url.push('/');
        }
        PnprClient {
            http: Client::new(),
            base_url,
            artifact_request_timeout: ARTIFACT_REQUEST_TIMEOUT,
        }
    }

    /// Confirm the server speaks a compatible protocol version. Errors
    /// if it's unreachable, isn't a pnpr (404 at `/-/pnpr`), or shares
    /// no protocol version with this client.
    pub async fn handshake(&self) -> Result<(), PnprClientError> {
        let capability = self.fetch_handshake(None).await?;
        Self::require_resolver_protocol(&capability)
    }

    async fn handshake_fix_lockfile(&self) -> Result<(), PnprClientError> {
        let capability = self.fetch_handshake(None).await?;
        Self::require_resolver_protocol(&capability)?;
        if !capability.fix_lockfile.contains(&PROTOCOL_VERSION) {
            return Err(PnprClientError::Server(format!(
                "pnpr server does not advertise lockfile repair support for resolver protocol v{PROTOCOL_VERSION}",
            )));
        }
        Ok(())
    }

    fn require_resolver_protocol(capability: &HandshakeCapability) -> Result<(), PnprClientError> {
        if !capability.versions.contains(&PROTOCOL_VERSION) {
            return Err(PnprClientError::Server(format!(
                "pnpr server speaks protocol versions {:?}, but this client requires v{PROTOCOL_VERSION}",
                capability.versions,
            )));
        }
        Ok(())
    }

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

    async fn fetch_handshake(
        &self,
        timeout: Option<Duration>,
    ) -> Result<HandshakeCapability, PnprClientError> {
        let mut get = self.http.get(format!("{}-/pnpr", self.base_url));
        if let Some(timeout) = timeout {
            get = get.timeout(timeout);
        }
        let response = get.send().await?;
        if !response.status().is_success() {
            return Err(PnprClientError::Server(format!(
                "{} is not a pnpr server (GET /-/pnpr returned {})",
                self.base_url,
                response.status(),
            )));
        }
        let body: HandshakeResponse = response.json().await?;
        Ok(body.pnpr)
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
            opts.eligible_packages.contains(&candidate.package.name)
                && opts.allowed_builds.contains(&candidate.package.name)
        });
        if opts.candidates.is_empty() {
            return Ok(BTreeMap::new());
        }
        if opts.candidates.len() > MAX_CANDIDATES {
            return Err(PnprClientError::Protocol(format!(
                "shared artifact lookup exceeds the {MAX_CANDIDATES}-candidate limit",
            )));
        }
        let mut candidates = BTreeMap::new();
        for candidate in &opts.candidates {
            candidate.validate().map_err(|err| PnprClientError::Protocol(err.to_string()))?;
            if candidates.insert(candidate.key.as_str(), candidate).is_some() {
                return Err(PnprClientError::Protocol(format!(
                    "duplicate shared artifact candidate {:?}",
                    candidate.key,
                )));
            }
        }
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
        let response: ResolveArtifactsResponse = serde_json::from_slice(&body)
            .map_err(|err| PnprClientError::Protocol(err.to_string()))?;
        if response.artifacts.len() > candidates.len() {
            return Err(PnprClientError::Protocol(
                "shared artifact response contains more entries than requested".to_string(),
            ));
        }

        let mut selected = BTreeMap::new();
        let mut response_keys = HashSet::new();
        for artifact in response.artifacts {
            if !response_keys.insert(artifact.key.clone()) {
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
            let mut best: Option<(u64, String, VerifiedArtifact)> = None;
            for variant in artifact.variants {
                let Some(public_key) = opts.trusted_keys.get(&variant.envelope.key_id) else {
                    continue;
                };
                let Ok(payload_bytes) = variant.envelope.verify_signature_bytes(public_key) else {
                    continue;
                };
                let envelope_digest = variant
                    .envelope
                    .digest()
                    .map_err(|err| PnprClientError::Protocol(err.to_string()))?;
                if opts
                    .quarantined_envelope_digests
                    .get(candidate.key.as_str())
                    .is_some_and(|digests| digests.contains(&envelope_digest))
                {
                    continue;
                }
                let payload: ArtifactPayload = match serde_json::from_slice(&payload_bytes) {
                    Ok(payload) => payload,
                    Err(error) => {
                        if let Some(on_rejected_artifact) = &opts.on_rejected_artifact {
                            on_rejected_artifact(RejectedArtifact {
                                input_key: candidate.key.clone(),
                                envelope_digest,
                                reason: format!("payload is not valid JSON: {error}"),
                            });
                        }
                        continue;
                    }
                };
                if let Err(error) = payload.validate() {
                    if let Some(on_rejected_artifact) = &opts.on_rejected_artifact {
                        on_rejected_artifact(RejectedArtifact {
                            input_key: candidate.key.clone(),
                            envelope_digest,
                            reason: error.to_string(),
                        });
                    }
                    continue;
                }
                if !artifact_matches_candidate(&payload, candidate) {
                    continue;
                }
                let Some(rank) =
                    compatibility_rank_prevalidated(&payload.compatibility, &opts.supported_tags)
                else {
                    continue;
                };
                if opts
                    .pinned_envelope_digests
                    .get(candidate.key.as_str())
                    .is_some_and(|pinned| pinned != &envelope_digest)
                {
                    continue;
                }
                if best.as_ref().is_none_or(|(best_rank, best_digest, _)| {
                    (rank, &envelope_digest) < (*best_rank, best_digest)
                }) {
                    best = Some((
                        rank,
                        envelope_digest.clone(),
                        VerifiedArtifact { payload, envelope: variant.envelope, envelope_digest },
                    ));
                }
            }
            if let Some((_, _, artifact)) = best {
                selected.insert(candidate.key.clone(), artifact);
            }
        }
        Ok(selected)
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

    /// Resolve a single project against the server and return the
    /// resolved lockfile, ignoring the streamed per-package frames.
    /// Equivalent to [`Self::resolve_streaming`] with a no-op callback.
    pub async fn resolve(&self, opts: ResolveOptions) -> Result<ResolveOutcome, PnprClientError> {
        self.resolve_projects(opts.into()).await
    }

    /// Resolve workspace projects against the server and return the resolved
    /// lockfile, ignoring the streamed per-package frames.
    pub async fn resolve_projects(
        &self,
        opts: ResolveProjectsOptions,
    ) -> Result<ResolveOutcome, PnprClientError> {
        self.resolve_projects_streaming(opts, |_| {}).await
    }

    /// Ask the server to verify a lockfile under the client's registry
    /// and policy settings, without resolving or echoing the lockfile
    /// back.
    pub async fn verify_lockfile(
        &self,
        opts: VerifyLockfileOptions,
    ) -> Result<(), PnprClientError> {
        let request = serde_json::json!({
            "registry": opts.registry,
            "registries": opts.registries,
            "overrides": opts.overrides,
            "lockfile": opts.lockfile,
            "trustLockfile": opts.trust_lockfile,
            "minimumReleaseAge": opts.minimum_release_age,
            "minimumReleaseAgeExclude": opts.minimum_release_age_exclude,
            "minimumReleaseAgeIgnoreMissingTime": opts.minimum_release_age_ignore_missing_time,
            "trustPolicy": opts.trust_policy,
            "trustPolicyExclude": opts.trust_policy_exclude,
            "trustPolicyIgnoreAfter": opts.trust_policy_ignore_after,
        });

        let mut post =
            self.http.post(format!("{}-/pnpr/v0/verify-lockfile", self.base_url)).json(&request);
        if let Some(authorization) = opts.authorization.as_deref() {
            post = post.header("authorization", authorization);
        }
        let response = post.send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(PnprClientError::Server(format!(
                "/-/pnpr/v0/verify-lockfile returned {status}: {body}",
            )));
        }

        let mut stream = response.bytes_stream();
        let mut buf: Vec<u8> = Vec::new();
        while let Some(chunk) = stream.next().await {
            buf.extend_from_slice(&chunk?);
            while let Some(newline) = buf.iter().position(|&byte| byte == b'\n') {
                let line: Vec<u8> = buf.drain(..=newline).collect();
                let line = &line[..line.len() - 1];
                if line.is_empty() {
                    continue;
                }
                match parse_verify_frame(line)? {
                    VerifyFrame::Done => return Ok(()),
                    VerifyFrame::Error { message } => {
                        return Err(PnprClientError::Server(message));
                    }
                    VerifyFrame::Violations { violations } => {
                        return Err(PnprClientError::Verification(build_verify_error(violations)));
                    }
                }
            }
        }

        Err(PnprClientError::Protocol(
            "/-/pnpr/v0/verify-lockfile stream ended without a terminal frame".to_string(),
        ))
    }

    /// Resolve a single project, invoking `on_package` once per resolved
    /// tarball as its `package` frame streams in — *before* the full
    /// lockfile arrives — so the caller can begin fetching each tarball
    /// while the server is still resolving. Returns the resolved lockfile
    /// from the terminal `done` frame.
    pub async fn resolve_streaming(
        &self,
        opts: ResolveOptions,
        on_package: impl FnMut(ResolvedPackage),
    ) -> Result<ResolveOutcome, PnprClientError> {
        self.resolve_projects_streaming(opts.into(), on_package).await
    }

    /// Resolve workspace projects, invoking `on_package` once per resolved
    /// tarball before the terminal lockfile frame arrives.
    pub async fn resolve_projects_streaming(
        &self,
        opts: ResolveProjectsOptions,
        mut on_package: impl FnMut(ResolvedPackage),
    ) -> Result<ResolveOutcome, PnprClientError> {
        if opts.fix_lockfile {
            self.handshake_fix_lockfile().await?;
        }
        // The server's response is untrusted, and the caller merges the
        // returned lockfile into `pnpm-lock.yaml`. Constrain it to the
        // importers this request is about — the requested projects plus
        // whatever the input lockfile already carried — so a hostile server
        // cannot introduce dependencies for a project that was never sent.
        // This is a containment check (every returned importer was
        // requested), which is the injection boundary; it deliberately does
        // not require every requested importer to be present. A dependency-
        // free importer is still present-but-empty (pnpm records it as
        // `{ specifiers: {} }`), and a genuinely missing importer is surfaced
        // downstream by the lockfile merge, not a way to inject dependencies.
        let permitted_importers: HashSet<String> = opts
            .projects
            .iter()
            .map(|project| project.dir.clone())
            .chain(opts.lockfile.iter().flat_map(|lockfile| lockfile.importers.keys().cloned()))
            .collect();
        let project_transforms_requested = has_project_transforms(&opts);
        let request = serde_json::json!({
            "projects": opts.projects,
            "registry": opts.registry,
            "registries": opts.registries,
            "overrides": opts.overrides,
            "patchedDependencies": opts.patched_dependencies,
            "packageExtensions": opts.package_extensions,
            "allowUnusedPatches": opts.allow_unused_patches,
            "catalogs": opts.catalogs,
            "autoInstallPeers": opts.auto_install_peers,
            "dedupePeers": opts.dedupe_peers,
            "excludeLinksFromLockfile": opts.exclude_links_from_lockfile,
            "lockfile": opts.lockfile,
            "frozenLockfile": opts.frozen_lockfile,
            "preferFrozenLockfile": opts.prefer_frozen_lockfile,
            "updatePatches": opts.update_patches,
            "fixLockfile": opts.fix_lockfile,
            "ignoreManifestCheck": opts.ignore_manifest_check,
            "trustLockfile": opts.trust_lockfile,
            "resolutionMode": opts.resolution_mode,
            "minimumReleaseAge": opts.minimum_release_age,
            "minimumReleaseAgeExclude": opts.minimum_release_age_exclude,
            "minimumReleaseAgeIgnoreMissingTime": opts.minimum_release_age_ignore_missing_time,
            "trustPolicy": opts.trust_policy,
            "trustPolicyExclude": opts.trust_policy_exclude,
            "trustPolicyIgnoreAfter": opts.trust_policy_ignore_after,
        });

        let mut post = self.http.post(format!("{}-/pnpr/v0/resolve", self.base_url)).json(&request);
        if let Some(authorization) = opts.authorization.as_deref() {
            post = post.header("authorization", authorization);
        }
        let response = post.send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(PnprClientError::Server(format!(
                "/-/pnpr/v0/resolve returned {status}: {body}",
            )));
        }

        if project_transforms_requested
            && response
                .headers()
                .get(PROJECT_TRANSFORMS_HEADER)
                .and_then(|value| value.to_str().ok())
                != Some(PROJECT_TRANSFORMS_VERSION)
        {
            return Err(PnprClientError::Protocol(
                "pnpr server /-/pnpr/v0/resolve does not advertise project-transform support"
                    .to_string(),
            ));
        }

        // Consume the NDJSON stream line by line. The response header above
        // proves transform support before any package frame is consumed, so
        // current servers preserve resolution/fetch overlap while older
        // servers fail without triggering downloads or buffering hints.
        // reqwest's `gzip` feature transparently inflates the byte stream if a
        // proxy compressed it, so the frames arrive as plain JSON lines.
        let mut stream = response.bytes_stream();
        let mut buf: Vec<u8> = Vec::new();
        while let Some(chunk) = stream.next().await {
            buf.extend_from_slice(&chunk?);
            while let Some(newline) = buf.iter().position(|&byte| byte == b'\n') {
                let line: Vec<u8> = buf.drain(..=newline).collect();
                let line = &line[..line.len() - 1];
                if line.is_empty() {
                    continue;
                }
                match parse_frame(line)? {
                    Frame::Package {
                        id,
                        name,
                        version,
                        integrity,
                        tarball,
                        unpacked_size,
                        file_count,
                        revision,
                    } => {
                        on_package(ResolvedPackage {
                            id,
                            name,
                            version,
                            integrity,
                            tarball,
                            unpacked_size,
                            file_count,
                            revision,
                        });
                    }
                    Frame::Done { lockfile, stats } => {
                        if let Some(unexpected) = lockfile
                            .importers
                            .keys()
                            .find(|importer| !permitted_importers.contains(*importer))
                        {
                            return Err(PnprClientError::Protocol(format!(
                                "/-/pnpr/v0/resolve returned an importer that was not requested: {unexpected:?}",
                            )));
                        }
                        assert_transform_metadata(&lockfile, &opts)?;
                        return Ok(ResolveOutcome { lockfile: *lockfile, stats });
                    }
                    Frame::Error { message } => return Err(PnprClientError::Server(message)),
                    Frame::Violations { violations } => {
                        return Err(PnprClientError::Verification(build_verify_error(violations)));
                    }
                }
            }
        }
        Err(PnprClientError::Protocol(
            "/-/pnpr/v0/resolve stream ended without a terminal frame".to_string(),
        ))
    }
}

fn has_project_transforms(opts: &ResolveProjectsOptions) -> bool {
    opts.patched_dependencies.as_ref().is_some_and(|patches| !patches.is_empty())
        || opts.package_extensions.as_ref().is_some_and(|extensions| !extensions.is_empty())
}

const PROJECT_TRANSFORMS_HEADER: &str = "pnpr-project-transforms";
const PROJECT_TRANSFORMS_VERSION: &str = "1";

fn assert_transform_metadata(
    lockfile: &Lockfile,
    opts: &ResolveProjectsOptions,
) -> Result<(), PnprClientError> {
    if let Some(expected) = opts.patched_dependencies.as_ref().filter(|patches| !patches.is_empty())
        && !equal_patch_hashes(lockfile.patched_dependencies.as_ref(), expected)
    {
        return Err(PnprClientError::Protocol(
            "/-/pnpr/v0/resolve returned patchedDependencies that do not match the request; the server may not support project transforms".to_string(),
        ));
    }

    if let Some(package_extensions) =
        opts.package_extensions.as_ref().filter(|extensions| !extensions.is_empty())
    {
        let value = serde_json::to_value(package_extensions)
            .map_err(|err| PnprClientError::Protocol(err.to_string()))?;
        let expected = hash_object_nullable_with_prefix(&value)
            .expect("a non-empty packageExtensions map has a checksum");
        if lockfile.package_extensions_checksum.as_deref() != Some(expected.as_str()) {
            return Err(PnprClientError::Protocol(
                "/-/pnpr/v0/resolve returned packageExtensionsChecksum that does not match the request; the server may not support project transforms".to_string(),
            ));
        }
    }

    Ok(())
}

fn equal_patch_hashes(
    actual: Option<&BTreeMap<String, String>>,
    expected: &IndexMap<String, String>,
) -> bool {
    actual.is_some_and(|actual| {
        actual.len() == expected.len()
            && expected.iter().all(|(selector, hash)| actual.get(selector) == Some(hash))
    })
}

fn parse_frame(line: &[u8]) -> Result<Frame, PnprClientError> {
    serde_json::from_slice(line).map_err(|err| PnprClientError::Protocol(err.to_string()))
}

fn artifact_matches_candidate(payload: &ArtifactPayload, candidate: &ArtifactCandidate) -> bool {
    let ArtifactCandidate { key: input_key, package, source_integrity, owner } = candidate;
    payload.input_key == *input_key
        && payload.package == *package
        && payload.source_integrity == *source_integrity
        && payload.owner == *owner
}

fn parse_verify_frame(line: &[u8]) -> Result<VerifyFrame, PnprClientError> {
    serde_json::from_slice(line).map_err(|err| PnprClientError::Protocol(err.to_string()))
}

async fn response_body_bounded(
    response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, PnprClientError> {
    if response.content_length().is_some_and(|length| length > limit as u64) {
        return Err(PnprClientError::Protocol(format!(
            "pnpr response exceeds the {limit}-byte limit",
        )));
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(PnprClientError::Protocol(format!(
                "pnpr response exceeds the {limit}-byte limit",
            )));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// One NDJSON frame from `/-/pnpr/v0/resolve`. `package` frames stream as the
/// server resolves; exactly one terminal frame (`done` / `error` /
/// `violations`) closes the response.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Frame {
    Package {
        id: String,
        name: String,
        version: String,
        integrity: String,
        tarball: String,
        #[serde(rename = "unpackedSize", default)]
        unpacked_size: Option<usize>,
        #[serde(rename = "fileCount", default)]
        file_count: Option<usize>,
        #[serde(default)]
        revision: Option<TarballRevision>,
    },
    /// Boxed: the lockfile dwarfs the other variants, so keeping it
    /// behind a pointer keeps the enum small.
    Done {
        lockfile: Box<Lockfile>,
        #[serde(default)]
        stats: Stats,
    },
    Error {
        message: String,
    },
    Violations {
        violations: Vec<WireViolation>,
    },
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum VerifyFrame {
    Done,
    Error { message: String },
    Violations { violations: Vec<WireViolation> },
}

#[derive(Deserialize)]
struct WireViolation {
    name: String,
    version: String,
    code: String,
    reason: String,
}

/// Rebuild the [`VerifyError`] the local gate would have raised from
/// the server's rendered violations. Sorting by `name@version` before
/// [`VerifyError::from_rendered`] reproduces the same breakdown order
/// the local runner produces, so the abort is byte-identical.
fn build_verify_error(mut violations: Vec<WireViolation>) -> VerifyError {
    violations.sort_by(|left, right| {
        format!("{}@{}", left.name, left.version).cmp(&format!("{}@{}", right.name, right.version))
    });
    let rendered: Vec<RenderedViolation> = violations
        .into_iter()
        .map(|violation| RenderedViolation {
            name: violation.name,
            version: violation.version,
            code: intern_violation_code(&violation.code),
            reason: violation.reason,
        })
        .collect();
    VerifyError::from_rendered(&rendered)
}

/// Map a wire violation code back to the `&'static str` constant
/// [`VerifyError::from_rendered`] matches on. Values are byte-identical
/// to `pnpm_resolving_npm_resolver`'s violation codes; an unknown
/// code falls back to the generic envelope rather than fabricating a
/// variant. Kept inline (rather than depending on the npm resolver)
/// for the same reason the verification crate aliases them.
fn intern_violation_code(code: &str) -> &'static str {
    match code {
        "MINIMUM_RELEASE_AGE_VIOLATION" => "MINIMUM_RELEASE_AGE_VIOLATION",
        "TRUST_DOWNGRADE" => "TRUST_DOWNGRADE",
        "TARBALL_URL_MISMATCH" => "TARBALL_URL_MISMATCH",
        _ => "LOCKFILE_RESOLUTION_VERIFICATION",
    }
}

#[cfg(test)]
mod tests;
