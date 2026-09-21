//! Helpers that download and decode the `SHASUMS256.txt` integrity
//! files Node.js, Bun, and similar runtimes publish alongside their
//! binary releases. The file's format is one `<hex-sha256>  <filename>`
//! row per line; pacquet converts each row into an SRI-style
//! `sha256-<base64>` integrity string the lockfile records on the
//! emitted `BinaryResolution`.
//!
//! Three surfaces:
//!
//! - [`fetch_shasums_file`] — download and parse every row at once.
//!   The node-resolver and bun-resolver fan the parsed rows out across
//!   every artifact a release ships.
//! - [`fetch_verified_node_shasums_file`] — download a Node.js release
//!   SHASUMS file, verify its detached `OpenPGP` signature against the
//!   embedded Node.js release keys, then parse the trusted body.
//! - [`pick_file_checksum_from_shasums_file`] — re-parse a previously
//!   downloaded body to extract the integrity of a single file. The
//!   verifier path uses it when only one variant's hash is needed.

pub use disk_cache::RUNTIME_SHASUMS_CACHE_DIR;
pub use errors::{FetchShasumsFileError, FetchVerifiedNodeShasumsError, PickFileChecksumError};

mod disk_cache;
mod errors;
mod node_release_keys;

use std::{path::Path, sync::Arc, time::Duration};

use base64::{Engine, engine::general_purpose::STANDARD as BASE64_STANDARD};

use pnpm_network::{AuthHeaders, RetryOpts, ThrottledClient};

use disk_cache::{
    MAX_CACHED_SHASUMS_LEN, ShasumsTrust, read_cached_bytes, read_cached_shasums,
    write_cached_shasums,
};
mod signatures;
use signatures::is_signed_by_trusted_node_release_key;

/// One row parsed out of a `SHASUMS256.txt` body.
///
/// `integrity` is already SRI-encoded (`sha256-<base64>`); callers can
/// drop the value straight into an
/// [`ssri::Integrity`](https://docs.rs/ssri/latest/ssri/struct.Integrity.html)
/// via `parse`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShasumsFileItem {
    pub integrity: String,
    pub file_name: String,
}

/// Upper bound on a fetched body, for the callers that read one from a
/// URL naming no version. A release asset list is a few kilobytes to a
/// few hundred; a body past this bound is not one, and is the size the
/// disk cache already refuses to hold.
const MAX_SHASUMS_BYTES: usize = MAX_CACHED_SHASUMS_LEN as usize;

/// Download `<shasums_url>` and decode every `<hex>  <filename>` row.
///
/// Whitespace between the hash and the filename is split on `\s+` to
/// tolerate the double-space the upstream files actually use *and* any
/// future formatting drift.
pub async fn fetch_shasums_file(
    http_client: &ThrottledClient,
    shasums_url: &str,
) -> Result<Vec<ShasumsFileItem>, FetchShasumsFileError> {
    let body = fetch_shasums_file_raw(http_client, shasums_url).await?;
    Ok(parse_shasums_file(&body))
}

/// Fetch a Node.js release's `SHASUMS256.txt` and verify its
/// detached `OpenPGP` signature (`SHASUMS256.txt.sig`) against the
/// embedded Node.js release keys before returning the body.
pub async fn fetch_verified_node_shasums(
    http_client: &ThrottledClient,
    shasums_url: &str,
) -> Result<String, FetchVerifiedNodeShasumsError> {
    let (body, _signature) =
        fetch_verified_node_shasums_with_signature(http_client, shasums_url, None).await?;
    Ok(body)
}

/// [`fetch_verified_node_shasums`], additionally returning the verified
/// detached signature so the disk cache can persist it as the entry's
/// verification evidence.
async fn fetch_verified_node_shasums_with_signature(
    http_client: &ThrottledClient,
    shasums_url: &str,
    auth_headers: Option<&AuthHeaders>,
) -> Result<(String, Vec<u8>), FetchVerifiedNodeShasumsError> {
    let shasums_bytes =
        fetch_node_shasums_bytes(http_client, shasums_url, "SHASUMS256.txt", auth_headers).await?;
    let signature_url = format!("{shasums_url}.sig");
    let signature_bytes =
        fetch_node_shasums_bytes(http_client, &signature_url, "SHASUMS256.txt.sig", auth_headers)
            .await?;

    if !is_signed_by_trusted_node_release_key(&shasums_bytes, &signature_bytes)? {
        return Err(FetchVerifiedNodeShasumsError::SignatureInvalid {
            url: shasums_url.to_string(),
        });
    }

    let body = String::from_utf8(shasums_bytes)
        .map_err(|error| FetchVerifiedNodeShasumsError::InvalidUtf8 {
            url: shasums_url.to_string(),
            error: Arc::new(error),
        })?;
    Ok((body, signature_bytes))
}

/// Like [`fetch_shasums_file`], but first verifies the SHASUMS file's
/// detached `OpenPGP` signature against the Node.js release keys.
pub async fn fetch_verified_node_shasums_file(
    http_client: &ThrottledClient,
    shasums_url: &str,
) -> Result<Vec<ShasumsFileItem>, FetchVerifiedNodeShasumsError> {
    fetch_verified_node_shasums_file_cached(http_client, shasums_url, None).await
}

/// Like [`fetch_verified_node_shasums_file`], backed by the disk cache
/// when `cache_dir` is given. The cache stores the body together with
/// its detached signature, and a cache hit re-verifies that signature
/// against the embedded release keys: the cache directory is
/// project-configurable, so a pre-seeded entry must prove it is a
/// genuine release body before it is served. Any verification failure
/// is a miss and the pair is refetched. `shasums_url` must be
/// version-pinned — a mutable URL must never be handed to the cache.
pub async fn fetch_verified_node_shasums_file_cached(
    http_client: &ThrottledClient,
    shasums_url: &str,
    cache_dir: Option<&Path>,
) -> Result<Vec<ShasumsFileItem>, FetchVerifiedNodeShasumsError> {
    fetch_verified_node_shasums_file_cached_inner(http_client, shasums_url, cache_dir, None).await
}

/// Like [`fetch_verified_node_shasums_file_cached`], selecting URL-scoped
/// authorization independently for the body and detached signature.
/// Auth-aware fetches bypass the URL-keyed cache so redirects cannot move
/// metadata across credential boundaries.
pub async fn fetch_verified_node_shasums_file_cached_with_auth_headers(
    http_client: &ThrottledClient,
    shasums_url: &str,
    cache_dir: Option<&Path>,
    auth_headers: &AuthHeaders,
) -> Result<Vec<ShasumsFileItem>, FetchVerifiedNodeShasumsError> {
    fetch_verified_node_shasums_file_cached_inner(
        http_client,
        shasums_url,
        cache_dir,
        Some(auth_headers),
    )
    .await
}

async fn fetch_verified_node_shasums_file_cached_inner(
    http_client: &ThrottledClient,
    shasums_url: &str,
    cache_dir: Option<&Path>,
    auth_headers: Option<&AuthHeaders>,
) -> Result<Vec<ShasumsFileItem>, FetchVerifiedNodeShasumsError> {
    let signature_url = format!("{shasums_url}.sig");
    let cache_dir = if auth_headers.is_some() { None } else { cache_dir };
    if let Some(body) = read_cached_shasums(cache_dir, ShasumsTrust::Verified, shasums_url, None)
        && let Some(signature) =
            read_cached_bytes(cache_dir, ShasumsTrust::Verified, &signature_url, None)
        && is_signed_by_trusted_node_release_key(body.as_bytes(), &signature).unwrap_or(false)
    {
        return Ok(parse_shasums_file(&body));
    }
    let (body, signature) =
        fetch_verified_node_shasums_with_signature(http_client, shasums_url, auth_headers).await?;
    write_cached_shasums(cache_dir, ShasumsTrust::Verified, shasums_url, body.as_bytes());
    write_cached_shasums(cache_dir, ShasumsTrust::Verified, &signature_url, &signature);
    Ok(parse_shasums_file(&body))
}

/// Like [`fetch_shasums_file`], backed by the disk cache when
/// `cache_dir` is given. For mirrors whose SHASUMS files carry no
/// verifiable signature the cached body is trusted exactly as far as
/// the TLS fetch that produced it. `shasums_url` must be
/// version-pinned — a mutable URL must never be handed to the cache.
pub async fn fetch_shasums_file_cached(
    http_client: &ThrottledClient,
    shasums_url: &str,
    cache_dir: Option<&Path>,
) -> Result<Vec<ShasumsFileItem>, FetchShasumsFileError> {
    fetch_shasums_file_cached_inner(
        http_client,
        shasums_url,
        cache_dir,
        None,
        None,
        RetryOpts { retries: 0, ..RetryOpts::default() },
    )
    .await
}

/// Like [`fetch_shasums_file_cached`], honoring the caller's retry policy on
/// a cache miss.
pub async fn fetch_shasums_file_cached_with_retry(
    http_client: &ThrottledClient,
    shasums_url: &str,
    cache_dir: Option<&Path>,
    retry_opts: RetryOpts,
) -> Result<Vec<ShasumsFileItem>, FetchShasumsFileError> {
    fetch_shasums_file_cached_inner(http_client, shasums_url, cache_dir, None, None, retry_opts)
        .await
}

/// Like [`fetch_shasums_file_cached`], for a URL that names a release
/// rather than a version: the newest release's file is a different list
/// once the release moves, so a cached body is read back only while it
/// is younger than `max_age`.
///
/// The body is bounded, because such a URL says nothing about which
/// release it will serve and so nothing about how much a mirror may
/// send. A body the cache could not hold is refused rather than parsed.
pub async fn fetch_moving_shasums_file_cached(
    http_client: &ThrottledClient,
    shasums_url: &str,
    cache_dir: Option<&Path>,
    max_age: Duration,
    retry_opts: RetryOpts,
) -> Result<Vec<ShasumsFileItem>, FetchShasumsFileError> {
    if let Some(body) =
        read_cached_shasums(cache_dir, ShasumsTrust::Unverified, shasums_url, Some(max_age))
    {
        return Ok(parse_shasums_file(&body));
    }
    let response = http_client
        .get_limited_bytes_with_secure_auth_and_retry(
            shasums_url,
            &AuthHeaders::default(),
            None,
            retry_opts,
            MAX_SHASUMS_BYTES,
        )
        .await
        .map_err(|error| FetchShasumsFileError::Network {
            url: shasums_url.to_string(),
            error: Arc::new(error),
        })?;
    if response.body_truncated {
        return Err(FetchShasumsFileError::TooLarge {
            url: shasums_url.to_string(),
            limit: MAX_SHASUMS_BYTES,
        });
    }
    if !response.status.is_success() {
        return Err(FetchShasumsFileError::StatusNotOk {
            url: shasums_url.to_string(),
            status: response.status.as_u16(),
        });
    }
    let body = String::from_utf8_lossy(&response.body).into_owned();
    write_cached_shasums(cache_dir, ShasumsTrust::Unverified, shasums_url, body.as_bytes());
    Ok(parse_shasums_file(&body))
}

/// Like [`fetch_shasums_file_cached`], selecting URL-scoped authorization for
/// the request. Auth-aware fetches bypass the URL-keyed cache so redirects
/// cannot move metadata across credential boundaries.
pub async fn fetch_shasums_file_cached_with_auth_headers(
    http_client: &ThrottledClient,
    shasums_url: &str,
    cache_dir: Option<&Path>,
    auth_headers: &AuthHeaders,
) -> Result<Vec<ShasumsFileItem>, FetchShasumsFileError> {
    fetch_shasums_file_cached_inner(
        http_client,
        shasums_url,
        cache_dir,
        Some(auth_headers),
        None,
        RetryOpts { retries: 0, ..RetryOpts::default() },
    )
    .await
}

async fn fetch_shasums_file_cached_inner(
    http_client: &ThrottledClient,
    shasums_url: &str,
    cache_dir: Option<&Path>,
    auth_headers: Option<&AuthHeaders>,
    max_age: Option<Duration>,
    retry_opts: RetryOpts,
) -> Result<Vec<ShasumsFileItem>, FetchShasumsFileError> {
    let cache_dir = if auth_headers.is_some() { None } else { cache_dir };
    if let Some(body) =
        read_cached_shasums(cache_dir, ShasumsTrust::Unverified, shasums_url, max_age)
    {
        return Ok(parse_shasums_file(&body));
    }
    let body =
        fetch_shasums_file_raw_with_auth(http_client, shasums_url, auth_headers, retry_opts).await?;
    write_cached_shasums(cache_dir, ShasumsTrust::Unverified, shasums_url, body.as_bytes());
    Ok(parse_shasums_file(&body))
}

/// Companion to [`fetch_shasums_file`] that returns the raw body so
/// callers can later pick a single row out with
/// [`pick_file_checksum_from_shasums_file`].
pub async fn fetch_shasums_file_raw(
    http_client: &ThrottledClient,
    shasums_url: &str,
) -> Result<String, FetchShasumsFileError> {
    fetch_shasums_file_raw_with_auth(
        http_client,
        shasums_url,
        None,
        RetryOpts { retries: 0, ..RetryOpts::default() },
    )
    .await
}

async fn fetch_shasums_file_raw_with_auth(
    http_client: &ThrottledClient,
    shasums_url: &str,
    auth_headers: Option<&AuthHeaders>,
    retry_opts: RetryOpts,
) -> Result<String, FetchShasumsFileError> {
    let default_auth_headers = AuthHeaders::default();
    let response = http_client
        .get_limited_bytes_with_secure_auth_and_retry(
            shasums_url,
            auth_headers.unwrap_or(&default_auth_headers),
            None,
            retry_opts,
            MAX_SHASUMS_BYTES,
        )
        .await
        .map_err(|error| FetchShasumsFileError::Network {
            url: shasums_url.to_string(),
            error: Arc::new(error),
        })?;
    if response.body_truncated {
        return Err(FetchShasumsFileError::TooLarge {
            url: shasums_url.to_string(),
            limit: MAX_SHASUMS_BYTES,
        });
    }
    if !response.status.is_success() {
        return Err(FetchShasumsFileError::StatusNotOk {
            url: shasums_url.to_string(),
            status: response.status.as_u16(),
        });
    }
    Ok(String::from_utf8_lossy(&response.body).into_owned())
}

async fn fetch_node_shasums_bytes(
    http_client: &ThrottledClient,
    url: &str,
    what: &'static str,
    auth_headers: Option<&AuthHeaders>,
) -> Result<Vec<u8>, FetchVerifiedNodeShasumsError> {
    let (status, body) = if let Some(auth_headers) = auth_headers {
        let response = http_client
            .get_bytes_with_secure_auth_headers(url, auth_headers)
            .await
            .map_err(|error| node_shasums_network_error(what, url, error))?;
        (response.status, response.body)
    } else {
        let response = http_client
            .acquire_for_url(url)
            .await
            .get(url)
            .send()
            .await
            .map_err(|error| node_shasums_network_error(what, url, error))?;
        let status = response.status();
        let body =
            response.bytes().await.map_err(|error| node_shasums_network_error(what, url, error))?;
        (status, body.to_vec())
    };
    if !status.is_success() {
        return Err(FetchVerifiedNodeShasumsError::StatusNotOk {
            what,
            url: url.to_string(),
            status: status.as_u16(),
        });
    }
    Ok(body)
}

fn node_shasums_network_error(
    what: &'static str,
    url: &str,
    error: reqwest::Error,
) -> FetchVerifiedNodeShasumsError {
    FetchVerifiedNodeShasumsError::Network { what, url: url.to_string(), error: Arc::new(error) }
}

/// Parse a `SHASUMS256.txt` body into rows.
///
/// Split out from [`fetch_shasums_file`] so verifier-side code that
/// already has the body in hand can decode it without re-issuing the
/// network request.
#[must_use]
pub fn parse_shasums_file(body: &str) -> Vec<ShasumsFileItem> {
    body.lines()
        .filter_map(|line| {
            if line.is_empty() {
                return None;
            }
            let mut parts = line.split_whitespace();
            let sha256 = parts.next()?;
            let file_name = parts.next()?;
            Some(ShasumsFileItem {
                integrity: encode_sri(sha256),
                file_name: file_name.to_string(),
            })
        })
        .collect()
}

/// Pull the integrity of one file out of a body the caller already has.
///
/// Matches on a row ending in `  <file_name>` (two spaces — the format
/// upstream's files actually use, *not* the `\s+` permissive split
/// [`fetch_shasums_file`] tolerates).
pub fn pick_file_checksum_from_shasums_file(
    body: &str,
    file_name: &str,
) -> Result<String, PickFileChecksumError> {
    let needle = format!("  {file_name}");
    let line = body
        .lines()
        .find(|line| line.trim_end().ends_with(&needle))
        .ok_or_else(|| PickFileChecksumError::NotFound { file_name: file_name.to_string() })?;
    let sha256 = line
        .split_whitespace()
        .next()
        .unwrap_or("");
    if !is_sha256_hex(sha256) {
        return Err(PickFileChecksumError::Malformed {
            file_name: file_name.to_string(),
            sha256: sha256.to_string(),
        });
    }
    Ok(encode_sri(sha256))
}

/// Encode a sha256 hex digest as the `sha256-<base64>` integrity string
/// the lockfile records, for artifact sources that report a bare hex
/// digest instead of shipping a `SHASUMS256.txt`. `None` when `hex` is not
/// a well-formed sha256 digest, so a malformed one can be skipped rather
/// than installed unverified.
#[must_use]
pub fn sha256_hex_to_sri(hex: &str) -> Option<String> {
    is_sha256_hex(hex).then(|| encode_sri(hex))
}

/// Decode a 64-character lower-case hex string into `sha256-<base64>`.
///
/// Pre-condition: `hex` is the value [`is_sha256_hex`] already
/// validated *or* an upstream-trusted row that came straight out of a
/// well-formed `SHASUMS256.txt`. The decode is infallible under that
/// pre-condition; we still fall back to an empty string on a decode
/// failure so a hex hash that slipped past validation does not panic.
fn encode_sri(hex: &str) -> String {
    let bytes = decode_hex(hex).unwrap_or_default();
    format!("sha256-{}", BASE64_STANDARD.encode(bytes))
}

fn is_sha256_hex(value: &str) -> bool {
    // Upstream regex is `^[a-f0-9]{64}$` — lowercase only. Matching
    // that explicitly keeps a malformed mixed-case hex row from
    // sneaking through the validator that the upstream parser would
    // have rejected.
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(hex.get(index..index + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests;
