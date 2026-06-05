//! Client for pnpr's server-side resolver.
//!
//! Given a set of dependencies, it `POST`s them to `/v1/resolve`, where
//! the server resolves against the client's registries, verifies the
//! input lockfile under the client's policy, and answers with the
//! resolved lockfile as a gzipped JSON object. The caller then fetches
//! every tarball itself, in parallel, like a normal install
//! ([pnpm/pnpm#12230](https://github.com/pnpm/pnpm/issues/12230)).
//!
//! pnpr is a stateless resolver: it stores no tarballs and serves no file
//! content.

use std::{collections::BTreeMap, io::Read as _};

use derive_more::{Display, Error, From};
use flate2::read::GzDecoder;
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
    /// resolved lockfile. The server serves no file content — the caller
    /// fetches every tarball itself.
    pub async fn resolve(&self, opts: ResolveOptions) -> Result<ResolveOutcome, PnprClientError> {
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

        let raw = response.bytes().await?;
        parse_response(&decompress(&raw)?)
    }
}

/// Decompress a `Content-Encoding: gzip` body unless the HTTP stack
/// already did (detected via the gzip magic bytes), so the client works
/// whether or not reqwest's `gzip` feature is on. Returns the bytes as-is
/// when they're already decompressed.
fn decompress(raw: &[u8]) -> Result<Vec<u8>, PnprClientError> {
    if raw.starts_with(&[0x1f, 0x8b]) {
        let mut decoder = GzDecoder::new(raw);
        let mut out = Vec::new();
        decoder.read_to_end(&mut out)?;
        Ok(out)
    } else {
        Ok(raw.to_vec())
    }
}

/// Parse the install response: a JSON object carrying the resolved
/// lockfile and stats, or — when the server rejected the input lockfile
/// under the client's policy — the rendered verification violations.
fn parse_response(payload: &[u8]) -> Result<ResolveOutcome, PnprClientError> {
    let response: ResolveResponse = serde_json::from_slice(payload)
        .map_err(|err| PnprClientError::Protocol(err.to_string()))?;

    if let Some(violations) = response.violations.filter(|list| !list.is_empty()) {
        return Err(PnprClientError::Verification(build_verify_error(violations)));
    }

    let lockfile = response
        .lockfile
        .ok_or_else(|| PnprClientError::Protocol("install response had no lockfile".to_string()))?;

    Ok(ResolveOutcome { lockfile, stats: response.stats })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolveResponse {
    lockfile: Option<Lockfile>,
    #[serde(default)]
    stats: Stats,
    /// Present when the server rejected the input lockfile under the
    /// client's verification policy. Each entry mirrors the local
    /// runner's rendered violation so the client can rebuild the
    /// identical [`VerifyError`].
    #[serde(default)]
    violations: Option<Vec<WireViolation>>,
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
    let rendered = violations
        .into_iter()
        .map(|violation| RenderedViolation {
            name: violation.name,
            version: violation.version,
            code: intern_violation_code(&violation.code),
            reason: violation.reason,
        })
        .collect();
    VerifyError::from_rendered(rendered)
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
