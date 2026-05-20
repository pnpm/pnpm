//! Pacquet port of pnpm's
//! [`@pnpm/crypto.shasums-file`](https://github.com/pnpm/pnpm/blob/1627943d2a/crypto/shasums-file/src/index.ts).
//!
//! Helpers that download and decode the `SHASUMS256.txt` integrity
//! files Node.js, Bun, and similar runtimes publish alongside their
//! binary releases. The file's format is one `<hex-sha256>  <filename>`
//! row per line; pacquet converts each row into an SRI-style
//! `sha256-<base64>` integrity string the lockfile records on the
//! emitted `BinaryResolution`.
//!
//! Two surfaces:
//!
//! - [`fetch_shasums_file`] — download and parse every row at once.
//!   The node-resolver and bun-resolver fan the parsed rows out across
//!   every artifact a release ships.
//! - [`pick_file_checksum_from_shasums_file`] — re-parse a previously
//!   downloaded body to extract the integrity of a single file. The
//!   verifier path uses it when only one variant's hash is needed.

use std::sync::Arc;

use base64::{Engine, engine::general_purpose::STANDARD as BASE64_STANDARD};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_network::ThrottledClient;

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

/// Errors raised by [`fetch_shasums_file`] and [`fetch_shasums_file_raw`].
///
/// Mirrors upstream's `FAILED_DOWNLOAD_SHASUM_FILE` code, which the
/// install reporter parses as a network-stage failure.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum FetchShasumsFileError {
    #[display("Failed to fetch integrity file: {url} (status: {status})")]
    #[diagnostic(code(FAILED_DOWNLOAD_SHASUM_FILE))]
    StatusNotOk { url: String, status: u16 },

    #[display("Failed to fetch integrity file: {url}")]
    #[diagnostic(code(FAILED_DOWNLOAD_SHASUM_FILE))]
    Network {
        url: String,
        #[error(source)]
        error: Arc<reqwest::Error>,
    },
}

/// Errors raised by [`pick_file_checksum_from_shasums_file`].
///
/// Two upstream codes survive the port verbatim — they are the
/// per-file equivalents of `FAILED_DOWNLOAD_SHASUM_FILE`'s download
/// failure and signal that the body the verifier already has does not
/// answer the question being asked.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum PickFileChecksumError {
    #[display("SHA-256 hash not found in SHASUMS256.txt for: {file_name}")]
    #[diagnostic(code(NODE_INTEGRITY_HASH_NOT_FOUND))]
    NotFound {
        #[error(not(source))]
        file_name: String,
    },

    #[display("Malformed SHA-256 for {file_name}: {sha256}")]
    #[diagnostic(code(NODE_MALFORMED_INTEGRITY_HASH))]
    Malformed { file_name: String, sha256: String },
}

/// Download `<shasums_url>` and decode every `<hex>  <filename>` row.
///
/// Mirrors upstream's
/// [`fetchShasumsFile`](https://github.com/pnpm/pnpm/blob/1627943d2a/crypto/shasums-file/src/index.ts#L11-L27).
/// Empty lines are skipped; whitespace between the hash and the
/// filename is split on `\s+` to tolerate the double-space the
/// upstream files actually use *and* any future formatting drift.
pub async fn fetch_shasums_file(
    http_client: &ThrottledClient,
    shasums_url: &str,
) -> Result<Vec<ShasumsFileItem>, FetchShasumsFileError> {
    let body = fetch_shasums_file_raw(http_client, shasums_url).await?;
    Ok(parse_shasums_file(&body))
}

/// Companion to [`fetch_shasums_file`] that returns the raw body so
/// callers can later pick a single row out with
/// [`pick_file_checksum_from_shasums_file`].
///
/// Mirrors upstream's
/// [`fetchShasumsFileRaw`](https://github.com/pnpm/pnpm/blob/1627943d2a/crypto/shasums-file/src/index.ts#L29-L42).
pub async fn fetch_shasums_file_raw(
    http_client: &ThrottledClient,
    shasums_url: &str,
) -> Result<String, FetchShasumsFileError> {
    let response =
        http_client.acquire_for_url(shasums_url).await.get(shasums_url).send().await.map_err(
            |error| FetchShasumsFileError::Network {
                url: shasums_url.to_string(),
                error: Arc::new(error),
            },
        )?;
    if !response.status().is_success() {
        return Err(FetchShasumsFileError::StatusNotOk {
            url: shasums_url.to_string(),
            status: response.status().as_u16(),
        });
    }
    response.text().await.map_err(|error| FetchShasumsFileError::Network {
        url: shasums_url.to_string(),
        error: Arc::new(error),
    })
}

/// Parse a `SHASUMS256.txt` body into rows.
///
/// Split out from [`fetch_shasums_file`] so verifier-side code that
/// already has the body in hand can decode it without re-issuing the
/// network request.
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
/// Mirrors upstream's
/// [`pickFileChecksumFromShasumsFile`](https://github.com/pnpm/pnpm/blob/1627943d2a/crypto/shasums-file/src/index.ts#L46-L67):
/// match on a row ending in `  <file_name>` (two spaces — the format
/// upstream's files actually use, *not* the `\s+` permissive split
/// [`fetch_shasums_file`] tolerates), validate the hex hash is exactly
/// 64 lower-case hex characters, then re-encode it as `sha256-<b64>`.
pub fn pick_file_checksum_from_shasums_file(
    body: &str,
    file_name: &str,
) -> Result<String, PickFileChecksumError> {
    let needle = format!("  {file_name}");
    let line = body
        .lines()
        .find(|line| line.trim_end().ends_with(&needle))
        .ok_or_else(|| PickFileChecksumError::NotFound { file_name: file_name.to_string() })?;
    let sha256 = line.split_whitespace().next().unwrap_or("");
    if !is_sha256_hex(sha256) {
        return Err(PickFileChecksumError::Malformed {
            file_name: file_name.to_string(),
            sha256: sha256.to_string(),
        });
    }
    Ok(encode_sri(sha256))
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
        && value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
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
