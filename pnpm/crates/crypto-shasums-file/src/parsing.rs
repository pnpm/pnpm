use crate::{PickFileChecksumError, ShasumsFileItem};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64_STANDARD};

/// Parse a `SHASUMS256.txt` body into rows.
///
/// Split out from [`crate::fetch_shasums_file`] so verifier-side code that
/// already has the body in hand can decode it without re-issuing the
/// network request.
#[must_use]
pub fn parse_shasums_file(body: &str) -> Vec<ShasumsFileItem> {
    body
        .lines()
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
/// [`crate::fetch_shasums_file`] tolerates).
pub fn pick_file_checksum_from_shasums_file(
    body: &str,
    file_name: &str,
) -> Result<String, PickFileChecksumError> {
    let needle = format!("  {file_name}");
    let line = body
        .lines()
        .find(|line| line.trim_end().ends_with(&needle))
        .ok_or_else(|| PickFileChecksumError::NotFound {
            file_name: file_name.to_string(),
        })?;
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
