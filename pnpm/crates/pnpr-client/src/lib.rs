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

pub use artifacts::{RejectedArtifact, ResolveArtifactsOptions, VerifiedArtifact};
pub use ecosystem_cache::server_resolves;
pub use ecosystems::{CARGO_ECOSYSTEM, PYPI_ECOSYSTEM};
pub use pnpm_shared_artifact_protocol::{
    ARTIFACT_KIND, ArtifactBlobRequest, ArtifactBlobUpload, ArtifactCandidate, ArtifactFile,
    ArtifactManifest, ArtifactPayload, ArtifactSubject, BuilderProfile, COMPATIBILITY_TAG_SCHEMA,
    CompatibilityConstraints, DEPENDENCY_SIDE_EFFECTS_ARTIFACT_KIND,
    DEPENDENCY_SIDE_EFFECTS_INPUT_KEY_PREFIX, INPUT_KEY_PREFIX, LinuxGlibcPlatform, MacOsPlatform,
    OwnerScope, PackageIdentity, PublishArtifactRequest, ResolveArtifactsRequest,
    SIGNATURE_ALGORITHM, SignedArtifactEnvelope, WORKSPACE_TASK_ARTIFACT_KIND,
    WORKSPACE_TASK_INPUT_KEY_PREFIX, WindowsPlatform, blob_id, linux_glibc_supported_tags,
    linux_glibc_tag, macos_supported_tags, macos_tag, platform_fingerprint, windows_supported_tags,
    windows_tag,
};

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
use pnpm_shared_artifact_protocol::{
    ArtifactVariant, MAX_CANDIDATES, MAX_FILE_SIZE, MAX_RESOLVE_RESPONSE_SIZE,
    MAX_VARIANTS_PER_CANDIDATE, ResolveArtifactsResponse, ResolvedArtifact,
    compatibility_rank_prevalidated, validate_supported_tags, verify_blob,
};
use reqwest::Client;

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
    Verification(VerifyError),

    Io(std::io::Error),
}

/// Protocol version this client speaks. The server advertises the
/// versions it supports at `GET /-/pnpr`; today only v0 exists.
const PROTOCOL_VERSION: u32 = 0;

#[derive(Default, Deserialize)]
struct HandshakeResponse {
    #[serde(default)]
    pnpr: HandshakeCapability,
}

/// One `pnpm pipeline` run as `PUT /-/pnpr/v0/pipeline/runs` carries it:
/// the workspace and run identifiers plus the run's summary document and
/// event stream, verbatim.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishPipelineRunRequest {
    pub workspace: String,
    pub run_id: String,
    pub summary: serde_json::Value,
    pub events: Vec<serde_json::Value>,
}

#[derive(Default, Deserialize)]
struct HandshakeCapability {
    #[serde(default)]
    versions: Vec<u32>,
    #[serde(default)]
    artifacts: Vec<u32>,
    #[serde(default, rename = "fixLockfile")]
    fix_lockfile: Vec<u32>,
    /// The package ecosystems `/-/pnpr/v0/resolve` reads in its request
    /// body. An empty list is a server that resolves npm alone.
    #[serde(default)]
    ecosystems: Vec<String>,
}

/// Inputs for a Python resolution. Send them only to a server that
/// advertises the Python ecosystem ([`PnprClient::supports_ecosystem`]).
#[derive(Clone)]
pub struct PypiResolveOptions {
    /// PEP 508 requirement strings, as the project's manifest spells them.
    pub requirements: Vec<String>,
    /// The interpreter the environment being resolved is for.
    pub target: pnpm_python_resolver::Target,
    /// The Simple API base URL to resolve against.
    pub index: String,
    /// The project's own `requires-python`.
    pub requires_python: Option<String>,
    /// `Authorization` header identifying this caller to pnpr.
    pub authorization: Option<String>,
}

/// Inputs for a Cargo resolution. Send them only to a server that
/// advertises the Cargo ecosystem ([`PnprClient::supports_ecosystem`]).
#[derive(Clone)]
pub struct CargoResolveOptions {
    /// `cargo metadata --no-deps --format-version 1` output for the
    /// workspace being resolved.
    pub metadata: String,
    /// The sparse index to resolve against.
    pub registry: String,
    /// `Authorization` header identifying this caller to pnpr.
    pub authorization: Option<String>,
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

    /// Record one `pnpm pipeline` run on the server. The document is
    /// stored verbatim; a server without the pipeline surface answers 404.
    pub async fn publish_pipeline_run(
        &self,
        request: &PublishPipelineRunRequest,
        authorization: Option<&str>,
    ) -> Result<(), PnprClientError> {
        if authorization.is_some() && !pnpm_network::is_url_secure_for_credentials(&self.base_url) {
            return Err(PnprClientError::Protocol(
                "pipeline report credentials require HTTPS or a loopback server".to_string(),
            ));
        }
        let http = Client::builder().redirect(reqwest::redirect::Policy::none()).build()?;
        let mut put = http
            .put(format!("{}-/pnpr/v0/pipeline/runs", self.base_url))
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
                "/-/pnpr/v0/pipeline/runs returned {status}: {}",
                String::from_utf8_lossy(&body),
            )));
        }
        Ok(())
    }
}

impl PnprClient {
    /// Whether the server resolves `ecosystem` through
    /// `/-/pnpr/v0/resolve`. A server advertising no ecosystems resolves
    /// npm alone.
    pub async fn supports_ecosystem(&self, ecosystem: &str) -> Result<bool, PnprClientError> {
        let capability = self.fetch_handshake(None).await?;
        Self::require_resolver_protocol(&capability)?;
        Ok(capability.ecosystems.iter().any(|supported| supported == ecosystem))
    }
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

/// Cap on the body read back from a failed request, which is quoted into
/// the error message.
const MAX_ERROR_BODY_SIZE: usize = 64 * 1024;

#[cfg(test)]
mod tests;

mod artifacts;
use artifacts::ARTIFACT_REQUEST_TIMEOUT;

mod resolve;
use resolve::read_ndjson_frames;

mod ecosystem_cache;
mod ecosystems;

use ecosystems::{WireViolation, build_verify_error};
