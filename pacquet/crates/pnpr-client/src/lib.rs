//! Client for pnpr's server-side resolver.
//!
//! Given a set of dependencies, it `POST`s them to `/v1/resolve`, where
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
//! pnpr is a stateless resolver: it stores no tarballs and serves no file
//! content.

use std::collections::BTreeMap;

use derive_more::{Display, Error, From};
use futures_util::StreamExt as _;
use pacquet_config::TrustPolicy;
use pacquet_lockfile::Lockfile;
use pacquet_lockfile_verification::{RenderedViolation, VerifyError};
use reqwest::Client;
use serde::Deserialize;

/// Dependency map (`name` -> `version range`).
pub type DepMap = BTreeMap<String, String>;

/// A client bound to one pnpr server.
#[must_use]
pub struct PnprClient {
    http: Client,
    base_url: String,
}

/// Inputs for a single-project resolution.
pub struct ResolveOptions {
    pub dependencies: DepMap,
    pub dev_dependencies: DepMap,
    pub optional_dependencies: DepMap,
    /// The client's default registry. The server resolves against this
    /// (and `named_registries`) rather than its own configuration.
    pub registry: String,
    /// The client's named-registry aliases.
    pub named_registries: DepMap,
    /// The caller's forwarded upstream credentials, keyed by nerf-darted
    /// registry URI, so the server resolves private content as the
    /// caller. Distinct from [`Self::authorization`] (pnpr identity).
    pub auth_headers: DepMap,
    /// `Authorization` for the pnpr server's own URL (`None` if it needs
    /// none): identifies the caller to pnpr. Distinct from the upstream
    /// creds in [`Self::auth_headers`].
    pub authorization: Option<String>,
    /// The client's `overrides` (selector -> spec) as raw JSON, applied
    /// at resolve time server-side.
    pub overrides: Option<serde_json::Value>,
    /// The client's existing on-disk lockfile, when present. Sent both
    /// as the verification target and the resolution-reuse seed.
    pub lockfile: Option<Lockfile>,
    /// Frozen (use the lockfile as-is) vs reuse-and-update resolution
    /// behavior. Does not affect whether the input lockfile is verified.
    pub frozen_lockfile: bool,
    /// `preferFrozenLockfile`. `Some(false)` forces the server to
    /// re-resolve; `None` lets it default to reuse.
    pub prefer_frozen_lockfile: Option<bool>,
    /// `ignoreManifestCheck`: skip the manifest ↔ lockfile freshness
    /// comparison during the frozen resolve.
    pub ignore_manifest_check: bool,
    /// The client's effective `trustLockfile`. When `true` the server
    /// skips verifying the input lockfile (it still reuses it for
    /// resolution), mirroring the local `--trust-lockfile` opt-out.
    pub trust_lockfile: bool,
    /// The client's verification policy. The server verifies the input
    /// lockfile under *this* policy (not its own) before resolving.
    pub minimum_release_age: Option<u64>,
    pub minimum_release_age_exclude: Option<Vec<String>>,
    pub minimum_release_age_ignore_missing_time: bool,
    pub trust_policy: TrustPolicy,
    pub trust_policy_exclude: Option<Vec<String>>,
    pub trust_policy_ignore_after: Option<u64>,
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
/// versions it supports at `GET /-/pnpr`; today only v1 exists.
const PROTOCOL_VERSION: u32 = 1;

#[derive(Default, Deserialize)]
struct HandshakeResponse {
    #[serde(default)]
    pnpr: HandshakeCapability,
}

#[derive(Default, Deserialize)]
struct HandshakeCapability {
    #[serde(default)]
    versions: Vec<u32>,
}

impl PnprClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        let mut base_url = base_url.into();
        if !base_url.ends_with('/') {
            base_url.push('/');
        }
        PnprClient { http: Client::new(), base_url }
    }

    /// Confirm the server speaks a compatible protocol version. Errors
    /// if it's unreachable, isn't a pnpr (404 at `/-/pnpr`), or shares
    /// no protocol version with this client.
    pub async fn handshake(&self) -> Result<(), PnprClientError> {
        let response = self.http.get(format!("{}-/pnpr", self.base_url)).send().await?;
        if !response.status().is_success() {
            return Err(PnprClientError::Server(format!(
                "{} is not a pnpr server (GET /-/pnpr returned {})",
                self.base_url,
                response.status(),
            )));
        }
        let body: HandshakeResponse = response.json().await?;
        if !body.pnpr.versions.contains(&PROTOCOL_VERSION) {
            return Err(PnprClientError::Server(format!(
                "pnpr server speaks protocol versions {:?}, but this client requires v{PROTOCOL_VERSION}",
                body.pnpr.versions,
            )));
        }
        Ok(())
    }

    /// Resolve a single project against the server and return the
    /// resolved lockfile, ignoring the streamed per-package frames. The
    /// server serves no file content — the caller fetches every tarball
    /// itself. Equivalent to [`Self::resolve_streaming`] with a no-op
    /// callback.
    pub async fn resolve(&self, opts: ResolveOptions) -> Result<ResolveOutcome, PnprClientError> {
        self.resolve_streaming(opts, |_| {}).await
    }

    /// Resolve a single project, invoking `on_package` once per resolved
    /// tarball as its `package` frame streams in — *before* the full
    /// lockfile arrives — so the caller can begin fetching each tarball
    /// while the server is still resolving. Returns the resolved lockfile
    /// from the terminal `done` frame.
    pub async fn resolve_streaming(
        &self,
        opts: ResolveOptions,
        mut on_package: impl FnMut(ResolvedPackage),
    ) -> Result<ResolveOutcome, PnprClientError> {
        let request = serde_json::json!({
            "projects": [{
                "dir": ".",
                "dependencies": opts.dependencies,
                "devDependencies": opts.dev_dependencies,
                "optionalDependencies": opts.optional_dependencies,
            }],
            "registry": opts.registry,
            "namedRegistries": opts.named_registries,
            "authHeaders": opts.auth_headers,
            "overrides": opts.overrides,
            "lockfile": opts.lockfile,
            "frozenLockfile": opts.frozen_lockfile,
            "preferFrozenLockfile": opts.prefer_frozen_lockfile,
            "ignoreManifestCheck": opts.ignore_manifest_check,
            "trustLockfile": opts.trust_lockfile,
            "minimumReleaseAge": opts.minimum_release_age,
            "minimumReleaseAgeExclude": opts.minimum_release_age_exclude,
            "minimumReleaseAgeIgnoreMissingTime": opts.minimum_release_age_ignore_missing_time,
            "trustPolicy": opts.trust_policy,
            "trustPolicyExclude": opts.trust_policy_exclude,
            "trustPolicyIgnoreAfter": opts.trust_policy_ignore_after,
        });

        let mut post = self.http.post(format!("{}v1/resolve", self.base_url)).json(&request);
        if let Some(authorization) = opts.authorization.as_deref() {
            post = post.header("authorization", authorization);
        }
        let response = post.send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(PnprClientError::Server(format!("/v1/resolve returned {status}: {body}")));
        }

        // Consume the NDJSON stream line by line. `package` frames feed
        // `on_package` as they arrive (overlapping the server's
        // resolution); the first terminal frame ends the loop. reqwest's
        // `gzip` feature transparently inflates the byte stream if a
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
                    } => {
                        on_package(ResolvedPackage {
                            id,
                            name,
                            version,
                            integrity,
                            tarball,
                            unpacked_size,
                            file_count,
                        });
                    }
                    Frame::Done { lockfile, stats } => {
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
            "/v1/resolve stream ended without a terminal frame".to_string(),
        ))
    }
}

fn parse_frame(line: &[u8]) -> Result<Frame, PnprClientError> {
    serde_json::from_slice(line).map_err(|err| PnprClientError::Protocol(err.to_string()))
}

/// One NDJSON frame from `/v1/resolve`. `package` frames stream as the
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
        /// Absent from frames sent by servers that predate the field
        /// and for packages whose registry never published a
        /// `dist.unpackedSize`.
        #[serde(rename = "unpackedSize", default)]
        unpacked_size: Option<usize>,
        #[serde(rename = "fileCount", default)]
        file_count: Option<usize>,
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
/// to `pacquet_resolving_npm_resolver`'s violation codes; an unknown
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
