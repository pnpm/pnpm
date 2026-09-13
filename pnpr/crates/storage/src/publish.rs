//! Helpers for the `PUT /:pkg` publish endpoint.
//!
//! npm sends the entire packument plus base64-encoded tarballs in a
//! single JSON body. This module pulls the attachment metadata out of
//! the body, merges the incoming manifest into whatever packument is
//! already on disk, and provides the streaming decode/verify/write
//! routine the handler uses to persist each tarball. The actual I/O
//! lives here behind a sync interface so the publish handler can run
//! it inside [`tokio::task::spawn_blocking`] without blocking the
//! async runtime, and so these helpers stay easy to unit-test.

pub use manifest::{merge_journaled_packument, merge_manifest};

mod manifest;

use crate::{
    journal::DocumentMerge,
    streaming::{integrity_checker, parse_integrity},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64, read::DecoderReader};
use pnpr_error::{RegistryError, Result};
use serde_json::{Map, Value};
use ssri::{Algorithm, Integrity, IntegrityChecker, IntegrityOpts};
use std::{
    collections::{BTreeMap, HashSet},
    fmt::Write as FmtWrite,
    fs::File,
    io::{Cursor, Read, Write},
    path::Path,
};

/// Per-tarball metadata pulled out of an `_attachments` entry. We
/// hold the base64 payload as an owned `String` rather than decoding
/// it eagerly — the streaming write path consumes it directly and
/// the decoded bytes never have to live in memory.
#[derive(Debug)]
pub struct PendingAttachment {
    pub filename: String,
    /// Base64-encoded tarball, taken verbatim from the publish body.
    pub data: String,
    /// `_attachments.<filename>.length` — the declared byte count of
    /// the *decoded* tarball, used to catch truncated uploads.
    pub declared_length: Option<u64>,
}

/// Pull all `_attachments` out of a publish body. Returns one
/// [`PendingAttachment`] per entry and removes the `_attachments`
/// field from `body` so the saved packument doesn't duplicate the
/// on-disk tarball. Base64 decoding is deferred to the streaming
/// write path; this function just validates shape.
pub fn extract_attachments(body: &mut Value) -> Result<Vec<PendingAttachment>, RegistryError> {
    let Some(obj) = body.as_object_mut() else {
        return Ok(Vec::new());
    };
    let Some(attachments_value) = obj.remove("_attachments") else {
        return Ok(Vec::new());
    };
    let Value::Object(attachments) = attachments_value else {
        return Err(RegistryError::BadRequest {
            reason: "_attachments must be an object".to_string(),
        });
    };
    let mut out = Vec::with_capacity(attachments.len());
    for (filename, value) in attachments {
        let Value::Object(mut value_obj) = value else {
            return Err(RegistryError::InvalidAttachment {
                filename,
                reason: "expected object".to_string(),
            });
        };
        let Some(Value::String(data)) = value_obj.remove("data") else {
            return Err(RegistryError::InvalidAttachment {
                filename,
                reason: "missing string field `data`".to_string(),
            });
        };
        let declared_length = value_obj.get("length").and_then(Value::as_u64);
        out.push(PendingAttachment {
            filename,
            data,
            declared_length,
        });
    }
    Ok(out)
}

/// Stream-decode a base64 attachment, hash it as it flows by, and
/// write the bytes to `dest`. Fails fast (and leaves no on-disk
/// artifact other than the caller-supplied tmp file) if the decoded
/// tarball doesn't match the declared `dist.integrity` SRI, the
/// optional legacy `dist.shasum`, or the declared `length`.
///
/// Synchronous on purpose: the publish handler runs this inside
/// [`tokio::task::spawn_blocking`] so the base64-decode and hashing
/// stages can use plain `std::io` and operate on chunks small enough
/// (`CHUNK_BYTES`) that the full decoded payload never lives in
/// memory at once — only the original base64 string (held by the
/// JSON value) and a 64 KiB working buffer.
///
/// `dist.integrity` is required. npm always emits it; a body that
/// arrives without it is either tampered with or produced by a
/// buggy client, and accepting it would store a tarball whose hash
/// the registry never verified. All EINTEGRITY-class failures
/// surface with an `EINTEGRITY:` prefix so pnpm / npm clients can
/// recognize them.
pub fn stream_decode_verify_and_write(
    filename: &str,
    base64_data: &str,
    declared_length: Option<u64>,
    dist: Option<&Value>,
    dest: &Path,
) -> Result<u64, RegistryError> {
    let expected = ExpectedAttachment::from_dist(filename, dist, declared_length)?;
    let written = decode_and_write(base64_data, dest, &expected);
    if written.is_err() {
        // A partial or mismatched decode leaves nothing publishable behind.
        let _ = std::fs::remove_file(dest);
    }
    written
}

/// What the packument says an attachment must decode to.
struct ExpectedAttachment<'a> {
    filename: &'a str,
    integrity: Integrity,
    shasum: Option<&'a str>,
    length: Option<u64>,
}

impl<'a> ExpectedAttachment<'a> {
    fn from_dist(
        filename: &'a str,
        dist: Option<&'a Value>,
        length: Option<u64>,
    ) -> Result<Self, RegistryError> {
        let invalid = |reason: String| RegistryError::InvalidAttachment {
            filename: filename.to_string(),
            reason,
        };
        let dist = dist.ok_or_else(|| {
            invalid(
                "EINTEGRITY: packument has no matching versions[v].dist entry for this attachment"
                    .to_string(),
            )
        })?;
        let declared_integrity = dist
            .get("integrity")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("EINTEGRITY: dist.integrity is required".to_string()))?;
        let integrity = parse_integrity(declared_integrity)
            .map_err(|err| invalid(format!("EINTEGRITY: malformed dist.integrity: {err}")))?;
        let shasum = dist.get("shasum").and_then(Value::as_str);
        Ok(Self {
            filename,
            integrity,
            shasum,
            length,
        })
    }

    /// Check the decoded bytes against every digest and length the packument
    /// declared.
    fn verify(
        &self,
        total: u64,
        checker: IntegrityChecker,
        shasum_hasher: Option<IntegrityOpts>,
    ) -> Result<(), RegistryError> {
        if let Some(expected) = self.length
            && expected != total
        {
            return Err(self.invalid(format!(
                "EINTEGRITY: length mismatch: header says {expected}, decoded {total}",
            )));
        }
        checker
            .result()
            .map_err(|err| self.invalid(format!("EINTEGRITY: integrity mismatch: {err}")))?;
        let Some(declared) = self.shasum else {
            return Ok(());
        };
        let hasher = shasum_hasher.expect("shasum_hasher initialized when declared_shasum present");
        let computed = sha1_hex_from_integrity_opts(hasher);
        if computed.eq_ignore_ascii_case(declared) {
            return Ok(());
        }
        Err(self.invalid(format!(
            "EINTEGRITY: shasum mismatch: declared {declared:?}, computed {computed:?}",
        )))
    }
    fn invalid(&self, reason: String) -> RegistryError {
        RegistryError::InvalidAttachment {
            filename: self.filename.to_string(),
            reason,
        }
    }
}

/// Decode the base64 attachment straight to `dest`, hashing as it goes, and
/// verify what landed. The bytes are never held in memory whole.
fn decode_and_write(
    base64_data: &str,
    dest: &Path,
    expected: &ExpectedAttachment<'_>,
) -> Result<u64, RegistryError> {
    let mut checker = integrity_checker(&expected.integrity)
        .map_err(|err| expected.invalid(format!("EINTEGRITY: malformed dist.integrity: {err}")))?;
    let mut shasum_hasher = expected.shasum
        .is_some()
        .then(|| IntegrityOpts::new().algorithm(Algorithm::Sha1));

    let mut decoder = DecoderReader::new(Cursor::new(base64_data.as_bytes()), &BASE64);
    let mut file = File::create(dest).map_err(RegistryError::Io)?;
    const CHUNK_BYTES: usize = 64 * 1024;
    let mut buf = vec![0u8; CHUNK_BYTES];
    let mut total: u64 = 0;
    loop {
        let bytes_read = decoder
            .read(&mut buf)
            .map_err(|err| expected.invalid(format!("EINTEGRITY: base64 decode failed: {err}")))?;
        if bytes_read == 0 {
            break;
        }
        let chunk = &buf[..bytes_read];
        file.write_all(chunk).map_err(RegistryError::Io)?;
        checker.input(chunk);
        if let Some(hasher) = shasum_hasher.as_mut() {
            hasher.input(chunk);
        }
        total += bytes_read as u64;
    }

    expected.verify(total, checker, shasum_hasher)?;
    file.sync_all().map_err(RegistryError::Io)?;
    Ok(total)
}

/// Finalize a SHA-1 [`IntegrityOpts`] and re-encode the digest as a
/// 40-character lowercase hex string — the shape npm's legacy
/// `dist.shasum` field uses. ssri stores digests base64-encoded, so
/// we decode and re-encode as hex for the comparison.
fn sha1_hex_from_integrity_opts(opts: IntegrityOpts) -> String {
    let integrity = opts.result();
    let digest_base64 = integrity.hashes
        .first()
        .expect("ssri produces a Sha1 hash entry when requested")
        .digest
        .as_str();
    let digest_bytes = BASE64.decode(digest_base64).expect("ssri produces valid base64 digests");
    let mut hex = String::with_capacity(digest_bytes.len() * 2);
    for byte in &digest_bytes {
        write!(hex, "{byte:02x}").expect("writing to String never fails");
    }
    hex
}

/// Format the current time as an ISO-8601 / RFC-3339 string with
/// millisecond precision (e.g. `2025-01-02T03:04:05.678Z`). Matches
/// the shape npm and verdaccio use in `time.modified`.
#[must_use]
pub fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let since_epoch = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    iso_from_unix_millis(since_epoch.as_millis() as i64)
}

/// Format a unix-millis timestamp as an ISO-8601 / RFC-3339 string
/// with millisecond precision. The token-listing endpoint surfaces
/// `TokenRecord::created_at` (seconds since epoch) through this same
/// helper so both `time.modified` on packuments and `created` on
/// tokens render with identical shape.
#[must_use]
pub fn iso_from_unix_millis(millis: i64) -> String {
    // Civil-time conversion without pulling in `chrono`.
    // 86_400_000 ms in a day.
    let (days, ms_in_day) = (millis / 86_400_000, millis.rem_euclid(86_400_000));
    let (h, rem) = (ms_in_day / 3_600_000, ms_in_day.rem_euclid(3_600_000));
    let (m, rem) = (rem / 60_000, rem.rem_euclid(60_000));
    let (s, ms) = (rem / 1000, rem.rem_euclid(1000));
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}.{ms:03}Z")
}

/// Days since 1970-01-01 → year/month/day. Howard Hinnant's
/// algorithm, adapted to integer days from epoch. Shift so era
/// is positive (`719_468` = days from 0000-03-01 to 1970-01-01).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let epoch_days = days + 719_468;
    let era = epoch_days.div_euclid(146_097);
    let doe = epoch_days.rem_euclid(146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let year = i64::from(yoe) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}

#[cfg(test)]
mod tests;
