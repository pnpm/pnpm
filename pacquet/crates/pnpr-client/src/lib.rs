//! Client for pnpr's server-accelerated installs.
//!
//! Given a set of dependencies and the client's content-addressable
//! store, it:
//!
//! 1. reads the integrities already in the local store index,
//! 2. `POST`s them with the dependencies to `/v1/install`, asking the
//!    server to inline the file contents it's missing (`inlineFiles`),
//! 3. parses the single combined response — a length-prefixed JSON header
//!    (lockfile, stats, store-index entries, or verification violations)
//!    followed by the missing files' bytes,
//! 4. writes those bytes straight into the local CAFS *by digest* (no
//!    re-hashing) and writes the forwarded store-index entries, and
//! 5. returns the resolved lockfile for a headless install.
//!
//! The whole exchange is one round trip — no handshake, no follow-up
//! `/v1/files` fetch. See
//! [pnpm/pnpm#12165](https://github.com/pnpm/pnpm/issues/12165). The
//! response is buffered rather than truly streamed, a tracked follow-up.

use std::{
    collections::{BTreeMap, HashSet},
    io::Read as _,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use derive_more::{Display, Error, From};
use flate2::read::GzDecoder;
use pacquet_config::TrustPolicy;
use pacquet_lockfile::Lockfile;
use pacquet_lockfile_verification::{RenderedViolation, VerifyError};
use pacquet_store_dir::{StoreDir, StoreIndex, StoreIndexWriter, decode_package_files_index};
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
pub struct InstallOptions<'a> {
    /// The client's content-addressable store. Resolved files and store
    /// index entries are written here.
    pub store_dir: &'a StoreDir,
    pub dependencies: DepMap,
    pub dev_dependencies: DepMap,
    /// The client's default registry. The server resolves against this
    /// (and `named_registries`) rather than its own configuration.
    pub registry: String,
    /// The client's named-registry aliases.
    pub named_registries: DepMap,
    /// The caller's forwarded upstream credentials, keyed by nerf-darted
    /// registry URI, so the server resolves/fetches private content as the
    /// caller. Distinct from [`Self::authorization`] (pnpr identity).
    pub auth_headers: DepMap,
    /// `Authorization` for the pnpr server's own URL (`None` if it needs
    /// none): identifies the caller to pnpr's gate and keys the grant
    /// table. Distinct from the upstream creds in [`Self::auth_headers`].
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
    /// `lockfileOnly`: ask the server to resolve only — return the
    /// lockfile without fetching tarballs or computing the file diff, so
    /// the response carries no missing files. The caller writes the
    /// lockfile and skips materialization, mirroring pnpm's
    /// `--lockfile-only`. See
    /// [pnpm/pnpm#12146](https://github.com/pnpm/pnpm/issues/12146).
    pub lockfile_only: bool,
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

/// Result of [`PnprClient::install`].
#[must_use]
pub struct InstallOutcome {
    /// The resolved lockfile, ready for a headless install.
    pub lockfile: Lockfile,
    pub stats: Stats,
    /// Number of inlined file entries written into the local CAFS.
    pub files_written: usize,
    /// Number of store-index entries written to the local index.
    pub index_entries_written: usize,
}

/// Resolution statistics from the response header. Field names mirror
/// the server's camelCase JSON.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Stats {
    pub total_packages: u64,
    pub already_in_store: u64,
    pub packages_to_fetch: u64,
    pub files_in_new_packages: u64,
    pub files_already_in_cafs: u64,
    pub files_to_download: u64,
    pub download_bytes: u64,
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

    /// Resolve a single project against the server and materialize the
    /// missing files + store-index entries into the local store.
    ///
    /// One round trip: the request asks the server to inline the file
    /// contents (`inlineFiles`), so the response carries the lockfile,
    /// stats, store-index entries, and the missing files' bytes in a
    /// single body — no handshake and no follow-up `/v1/files` fetch.
    /// See [pnpm/pnpm#12165](https://github.com/pnpm/pnpm/issues/12165).
    pub async fn install(
        &self,
        opts: InstallOptions<'_>,
    ) -> Result<InstallOutcome, PnprClientError> {
        let store_keys = read_store_keys(opts.store_dir);
        let store_integrities = integrities_from_keys(&store_keys);
        let present: HashSet<&str> = store_keys.iter().map(String::as_str).collect();

        let request = serde_json::json!({
            "projects": [{
                "dir": ".",
                "dependencies": opts.dependencies,
                "devDependencies": opts.dev_dependencies,
            }],
            "storeIntegrities": store_integrities,
            "registry": opts.registry,
            "namedRegistries": opts.named_registries,
            "authHeaders": opts.auth_headers,
            "overrides": opts.overrides,
            "lockfile": opts.lockfile,
            "frozenLockfile": opts.frozen_lockfile,
            "preferFrozenLockfile": opts.prefer_frozen_lockfile,
            "ignoreManifestCheck": opts.ignore_manifest_check,
            "lockfileOnly": opts.lockfile_only,
            "trustLockfile": opts.trust_lockfile,
            "minimumReleaseAge": opts.minimum_release_age,
            "minimumReleaseAgeExclude": opts.minimum_release_age_exclude,
            "minimumReleaseAgeIgnoreMissingTime": opts.minimum_release_age_ignore_missing_time,
            "trustPolicy": opts.trust_policy,
            "trustPolicyExclude": opts.trust_policy_exclude,
            "trustPolicyIgnoreAfter": opts.trust_policy_ignore_after,
            "inlineFiles": true,
        });

        let mut post = self.http.post(format!("{}v1/install", self.base_url)).json(&request);
        if let Some(authorization) = opts.authorization.as_deref() {
            post = post.header("authorization", authorization);
        }
        let response = post.send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(PnprClientError::Server(format!("/v1/install returned {status}: {body}")));
        }

        let raw = response.bytes().await?;
        let parsed = parse_inline_response(&decompress(&raw)?)?;

        // The server inlines only the files the client is missing; a
        // `--lockfile-only` resolve and a verification pass both carry an
        // empty file payload, so this writes nothing in those cases.
        let files_written = write_files_payload(opts.store_dir, &parsed.files_payload)?;
        let index_entries_written =
            write_index_entries(opts.store_dir, parsed.index_entries, &present).await;

        Ok(InstallOutcome {
            lockfile: parsed.lockfile,
            stats: parsed.stats,
            files_written,
            index_entries_written,
        })
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

struct ParsedInstall {
    lockfile: Lockfile,
    stats: Stats,
    /// The `/v1/files`-shaped binary frames the server inlined after the
    /// header — written into the CAFS by [`write_files_payload`].
    files_payload: Vec<u8>,
    index_entries: Vec<(String, Vec<u8>)>,
}

/// Decode the combined `inlineFiles` install response: a 4-byte
/// big-endian header length, that many bytes of JSON header (lockfile,
/// stats, store-index entries, or verification violations), then the
/// file frames.
fn parse_inline_response(payload: &[u8]) -> Result<ParsedInstall, PnprClientError> {
    if payload.len() < 4 {
        return Err(PnprClientError::Protocol("install response too short".to_string()));
    }
    let header_len = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]) as usize;
    let header_end = 4 + header_len;
    if header_end > payload.len() {
        return Err(PnprClientError::Protocol("install header truncated".to_string()));
    }
    let header: InlineHeader = serde_json::from_slice(&payload[4..header_end])
        .map_err(|err| PnprClientError::Protocol(err.to_string()))?;

    if let Some(violations) = header.violations.filter(|list| !list.is_empty()) {
        return Err(PnprClientError::Verification(build_verify_error(violations)));
    }

    let lockfile = header
        .lockfile
        .ok_or_else(|| PnprClientError::Protocol("install response had no lockfile".to_string()))?;

    let mut index_entries = Vec::with_capacity(header.index_entries.len());
    for entry in header.index_entries {
        let raw =
            BASE64.decode(&entry.b64).map_err(|err| PnprClientError::Protocol(err.to_string()))?;
        index_entries.push((entry.key, raw));
    }

    Ok(ParsedInstall {
        lockfile,
        stats: header.stats,
        files_payload: payload[header_end..].to_vec(),
        index_entries,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InlineHeader {
    lockfile: Option<Lockfile>,
    #[serde(default)]
    stats: Stats,
    #[serde(default)]
    index_entries: Vec<InlineIndexEntry>,
    /// Present when the server rejected the input lockfile under the
    /// client's verification policy. Each entry mirrors the local
    /// runner's rendered violation so the client can rebuild the
    /// identical [`VerifyError`].
    #[serde(default)]
    violations: Option<Vec<WireViolation>>,
}

#[derive(Deserialize)]
struct InlineIndexEntry {
    /// The store-index key, `{integrity}\t{pkgId}`.
    key: String,
    /// The base64-encoded msgpackr-records buffer.
    b64: String,
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

/// Decode the inlined binary file payload and write each entry to the
/// CAFS by digest. Returns the number of entries written. An empty
/// payload (no frames before the end-of-stream marker) writes nothing.
fn write_files_payload(store_dir: &StoreDir, payload: &[u8]) -> Result<usize, PnprClientError> {
    if payload.is_empty() {
        return Ok(0);
    }
    if payload.len() < 4 {
        return Err(PnprClientError::Protocol("files payload too short".to_string()));
    }
    let json_len = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]) as usize;
    let mut offset = 4 + json_len;
    let mut written = 0;

    loop {
        if offset + 64 > payload.len() {
            return Err(PnprClientError::Protocol("truncated files payload".to_string()));
        }
        let digest_bytes = &payload[offset..offset + 64];
        if digest_bytes.iter().all(|byte| *byte == 0) {
            break; // end-of-stream marker
        }
        if offset + 69 > payload.len() {
            return Err(PnprClientError::Protocol("truncated file header".to_string()));
        }
        let size = u32::from_be_bytes([
            payload[offset + 64],
            payload[offset + 65],
            payload[offset + 66],
            payload[offset + 67],
        ]) as usize;
        let executable = payload[offset + 68] & 0x01 != 0;
        let content_start = offset + 69;
        let content_end = content_start + size;
        if content_end > payload.len() {
            return Err(PnprClientError::Protocol("truncated file content".to_string()));
        }
        let content = &payload[content_start..content_end];
        let digest = hex_encode(digest_bytes);

        write_cas_file(store_dir, &digest, executable, content)?;
        written += 1;
        offset = content_end;
    }

    Ok(written)
}

/// Write `content` to its content-addressed path. The digest is trusted
/// (the fast path skips re-hashing); a complete file already on disk is
/// left as-is, and a truncated one is replaced atomically — mirroring
/// the TypeScript `fetch-and-write-cafs` worker.
fn write_cas_file(
    store_dir: &StoreDir,
    digest: &str,
    executable: bool,
    content: &[u8],
) -> Result<(), PnprClientError> {
    let mode = if executable { 0o755 } else { 0o644 };
    let path = store_dir
        .cas_file_path_by_mode(digest, mode)
        .ok_or_else(|| PnprClientError::Protocol(format!("invalid digest: {digest}")))?;

    if let Ok(metadata) = std::fs::metadata(&path)
        && metadata.len() == content.len() as u64
    {
        return Ok(()); // already present and complete
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, content)?;
    set_executable(&tmp, executable)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

#[cfg(unix)]
fn set_executable(path: &std::path::Path, executable: bool) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = if executable { 0o755 } else { 0o644 };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_executable(_path: &std::path::Path, _executable: bool) -> std::io::Result<()> {
    Ok(())
}

/// Write the forwarded store-index entries, skipping keys already
/// present. Each entry's raw msgpackr-records buffer is decoded and
/// re-queued through the writer, whose blocking drain is awaited so the
/// rows are flushed before they're reported as written.
async fn write_index_entries(
    store_dir: &StoreDir,
    entries: Vec<(String, Vec<u8>)>,
    present: &HashSet<&str>,
) -> usize {
    let to_write: Vec<(String, Vec<u8>)> =
        entries.into_iter().filter(|(key, _)| !present.contains(key.as_str())).collect();
    if to_write.is_empty() {
        return 0;
    }

    let (writer, writer_task) = StoreIndexWriter::spawn(store_dir);
    let mut written = 0;
    for (key, raw) in &to_write {
        if let Ok(decoded) = decode_package_files_index(raw) {
            writer.queue(key.clone(), decoded);
            written += 1;
        }
    }
    drop(writer);
    let _ = writer_task.await;
    written
}

fn read_store_keys(store_dir: &StoreDir) -> Vec<String> {
    match StoreIndex::open_readonly_in(store_dir) {
        Ok(index) => index.keys().unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// The SRI integrities already in the store, derived from the
/// `{integrity}\t{pkgId}` index keys. Non-integrity keys (e.g. git URLs)
/// are filtered out — sending them would just bloat the request.
fn integrities_from_keys(keys: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for key in keys {
        let Some((integrity, _pkg_id)) = key.split_once('\t') else { continue };
        if !is_integrity_like(integrity) {
            continue;
        }
        if seen.insert(integrity) {
            out.push(integrity.to_string());
        }
    }
    out
}

fn is_integrity_like(value: &str) -> bool {
    value.starts_with("sha512-") || value.starts_with("sha256-") || value.starts_with("sha1-")
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests;
