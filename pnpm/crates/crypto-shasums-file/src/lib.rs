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
pub use parsing::{parse_shasums_file, pick_file_checksum_from_shasums_file, sha256_hex_to_sri};

mod disk_cache;
mod node_release_keys;

use std::{io::Cursor, path::Path, string::FromUtf8Error, sync::Arc};

use derive_more::{Display, Error};
use miette::Diagnostic;
use pgp::{
    composed::{Deserializable, DetachedSignature, SignedPublicKey},
    types::KeyDetails,
};
use pnpm_network::{AuthHeaders, ThrottledClient};

use disk_cache::{ShasumsTrust, read_cached_bytes, read_cached_shasums, write_cached_shasums};
use node_release_keys::{NODE_RELEASE_KEYS, NodeReleaseKey};

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
/// Mirrors upstream's `ERR_PNPM_FAILED_DOWNLOAD_SHASUM_FILE` code, which the
/// install reporter parses as a network-stage failure.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum FetchShasumsFileError {
    #[display("Failed to fetch integrity file: {url} (status: {status})")]
    #[diagnostic(code(ERR_PNPM_FAILED_DOWNLOAD_SHASUM_FILE))]
    StatusNotOk { url: String, status: u16 },

    #[display("Failed to fetch integrity file: {url}")]
    #[diagnostic(code(ERR_PNPM_FAILED_DOWNLOAD_SHASUM_FILE))]
    Network {
        url: String,
        #[error(source)]
        error: Arc<reqwest::Error>,
    },
}

/// Errors raised by [`fetch_verified_node_shasums`] and
/// [`fetch_verified_node_shasums_file`].
///
/// Mirrors pnpm's `ERR_PNPM_NODE_SHASUMS_FETCH_FAIL` and
/// `ERR_PNPM_NODE_SHASUMS_SIGNATURE_INVALID` codes. These are specific to
/// Node.js runtime verification, where a repository-configurable
/// mirror cannot be trusted to supply both the binary and the hash
/// list unchecked.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum FetchVerifiedNodeShasumsError {
    #[display("Failed to fetch {what} ({url}) to verify the Node.js download (status: {status})")]
    #[diagnostic(code(ERR_PNPM_NODE_SHASUMS_FETCH_FAIL))]
    StatusNotOk {
        #[error(not(source))]
        what: &'static str,
        #[error(not(source))]
        url: String,
        status: u16,
    },

    #[display("Failed to fetch {what} ({url}) to verify the Node.js download")]
    #[diagnostic(code(ERR_PNPM_NODE_SHASUMS_FETCH_FAIL))]
    Network {
        #[error(not(source))]
        what: &'static str,
        #[error(not(source))]
        url: String,
        #[error(source)]
        error: Arc<reqwest::Error>,
    },

    #[display("Could not read the Node.js SHASUMS signature: {error}")]
    #[diagnostic(code(ERR_PNPM_NODE_SHASUMS_SIGNATURE_INVALID))]
    SignatureUnreadable {
        #[error(source)]
        error: Arc<pgp::errors::Error>,
    },

    #[display("The verified Node.js SHASUMS file at {url} is not valid UTF-8")]
    #[diagnostic(code(ERR_PNPM_NODE_SHASUMS_SIGNATURE_INVALID))]
    InvalidUtf8 {
        #[error(not(source))]
        url: String,
        #[error(source)]
        error: Arc<FromUtf8Error>,
    },

    #[display(
        "Embedded Node.js release key fingerprint mismatch: expected {expected}, got {actual}"
    )]
    #[diagnostic(code(ERR_PNPM_NODE_SHASUMS_SIGNATURE_INVALID))]
    EmbeddedKeyFingerprintMismatch {
        #[error(not(source))]
        expected: &'static str,
        #[error(not(source))]
        actual: String,
    },

    #[display(
        "The OpenPGP signature of {url} does not match any trusted Node.js release key. The downloaded Node.js runtime cannot be verified as a genuine release."
    )]
    #[diagnostic(code(ERR_PNPM_NODE_SHASUMS_SIGNATURE_INVALID))]
    SignatureInvalid {
        #[error(not(source))]
        url: String,
    },
}

/// Errors raised by [`pick_file_checksum_from_shasums_file`].
///
/// Two upstream codes survive the port verbatim — they are the
/// per-file equivalents of `ERR_PNPM_FAILED_DOWNLOAD_SHASUM_FILE`'s download
/// failure and signal that the body the verifier already has does not
/// answer the question being asked.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum PickFileChecksumError {
    #[display("SHA-256 hash not found in SHASUMS256.txt for: {file_name}")]
    #[diagnostic(code(ERR_PNPM_NODE_INTEGRITY_HASH_NOT_FOUND))]
    NotFound {
        #[error(not(source))]
        file_name: String,
    },

    #[display("Malformed SHA-256 for {file_name}: {sha256}")]
    #[diagnostic(code(ERR_PNPM_NODE_MALFORMED_INTEGRITY_HASH))]
    Malformed { file_name: String, sha256: String },
}

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
    let signature_bytes = fetch_node_shasums_bytes(
        http_client,
        &signature_url,
        "SHASUMS256.txt.sig",
        auth_headers,
    )
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
    let cache_dir = if auth_headers.is_some() {
        None
    } else {
        cache_dir
    };
    if let Some(body) = read_cached_shasums(cache_dir, ShasumsTrust::Verified, shasums_url)
        && let Some(signature) =
            read_cached_bytes(cache_dir, ShasumsTrust::Verified, &signature_url)
        && is_signed_by_trusted_node_release_key(body.as_bytes(), &signature).unwrap_or(false)
    {
        return Ok(parse_shasums_file(&body));
    }
    let (body, signature) =
        fetch_verified_node_shasums_with_signature(http_client, shasums_url, auth_headers).await?;
    write_cached_shasums(
        cache_dir,
        ShasumsTrust::Verified,
        shasums_url,
        body.as_bytes(),
    );
    write_cached_shasums(
        cache_dir,
        ShasumsTrust::Verified,
        &signature_url,
        &signature,
    );
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
    fetch_shasums_file_cached_inner(http_client, shasums_url, cache_dir, None).await
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
    fetch_shasums_file_cached_inner(http_client, shasums_url, cache_dir, Some(auth_headers)).await
}

async fn fetch_shasums_file_cached_inner(
    http_client: &ThrottledClient,
    shasums_url: &str,
    cache_dir: Option<&Path>,
    auth_headers: Option<&AuthHeaders>,
) -> Result<Vec<ShasumsFileItem>, FetchShasumsFileError> {
    let cache_dir = if auth_headers.is_some() {
        None
    } else {
        cache_dir
    };
    if let Some(body) = read_cached_shasums(cache_dir, ShasumsTrust::Unverified, shasums_url) {
        return Ok(parse_shasums_file(&body));
    }
    let body = fetch_shasums_file_raw_with_auth(http_client, shasums_url, auth_headers).await?;
    write_cached_shasums(
        cache_dir,
        ShasumsTrust::Unverified,
        shasums_url,
        body.as_bytes(),
    );
    Ok(parse_shasums_file(&body))
}

/// Companion to [`fetch_shasums_file`] that returns the raw body so
/// callers can later pick a single row out with
/// [`pick_file_checksum_from_shasums_file`].
pub async fn fetch_shasums_file_raw(
    http_client: &ThrottledClient,
    shasums_url: &str,
) -> Result<String, FetchShasumsFileError> {
    fetch_shasums_file_raw_with_auth(http_client, shasums_url, None).await
}

async fn fetch_shasums_file_raw_with_auth(
    http_client: &ThrottledClient,
    shasums_url: &str,
    auth_headers: Option<&AuthHeaders>,
) -> Result<String, FetchShasumsFileError> {
    let (status, body) = if let Some(auth_headers) = auth_headers {
        let response = http_client
            .get_bytes_with_secure_auth_headers(shasums_url, auth_headers)
            .await
            .map_err(|error| FetchShasumsFileError::Network {
                url: shasums_url.to_string(),
                error: Arc::new(error),
            })?;
        (response.status, response.body)
    } else {
        let response = http_client
            .acquire_for_url(shasums_url)
            .await
            .get(shasums_url)
            .send()
            .await
            .map_err(|error| FetchShasumsFileError::Network {
                url: shasums_url.to_string(),
                error: Arc::new(error),
            })?;
        let status = response.status();
        let body = response
            .bytes()
            .await
            .map_err(|error| FetchShasumsFileError::Network {
                url: shasums_url.to_string(),
                error: Arc::new(error),
            })?;
        (status, body.to_vec())
    };
    if !status.is_success() {
        return Err(FetchShasumsFileError::StatusNotOk {
            url: shasums_url.to_string(),
            status: status.as_u16(),
        });
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}

async fn fetch_node_shasums_bytes(
    http_client: &ThrottledClient,
    url: &str,
    what: &'static str,
    auth_headers: Option<&AuthHeaders>,
) -> Result<Vec<u8>, FetchVerifiedNodeShasumsError> {
    let network_error = |error| FetchVerifiedNodeShasumsError::Network {
        what,
        url: url.to_string(),
        error: Arc::new(error),
    };
    let (status, body) = if let Some(auth_headers) = auth_headers {
        let response = http_client
            .get_bytes_with_secure_auth_headers(url, auth_headers)
            .await
            .map_err(network_error)?;
        (response.status, response.body)
    } else {
        let response = http_client
            .acquire_for_url(url)
            .await
            .get(url)
            .send()
            .await
            .map_err(network_error)?;
        let status = response.status();
        let body = response.bytes().await.map_err(network_error)?;
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

fn is_signed_by_trusted_node_release_key(
    content: &[u8],
    signature_bytes: &[u8],
) -> Result<bool, FetchVerifiedNodeShasumsError> {
    let signature =
        DetachedSignature::from_bytes(Cursor::new(signature_bytes)).map_err(signature_unreadable)?;
    for key in trusted_node_release_keys()? {
        if signature.verify(&key.primary_key, content).is_ok() {
            return Ok(true);
        }
        for subkey in &key.public_subkeys {
            if signature.verify(subkey, content).is_ok() {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn trusted_node_release_keys() -> Result<Vec<SignedPublicKey>, FetchVerifiedNodeShasumsError> {
    NODE_RELEASE_KEYS
        .iter()
        .map(read_node_release_key)
        .collect()
}

fn read_node_release_key(
    trusted_key: &NodeReleaseKey,
) -> Result<SignedPublicKey, FetchVerifiedNodeShasumsError> {
    let (key, _headers) = SignedPublicKey::from_armor_single(trusted_key.armored_key.as_bytes())
        .map_err(signature_unreadable)?;
    let actual_fingerprint = key.fingerprint().to_string();
    let fingerprint_matches = actual_fingerprint.eq_ignore_ascii_case(trusted_key.fingerprint);
    if !fingerprint_matches {
        return Err(
            FetchVerifiedNodeShasumsError::EmbeddedKeyFingerprintMismatch {
                expected: trusted_key.fingerprint,
                actual: actual_fingerprint,
            },
        );
    }
    Ok(key)
}

fn signature_unreadable(error: pgp::errors::Error) -> FetchVerifiedNodeShasumsError {
    FetchVerifiedNodeShasumsError::SignatureUnreadable {
        error: Arc::new(error),
    }
}

#[cfg(test)]
mod tests;

mod parsing;
