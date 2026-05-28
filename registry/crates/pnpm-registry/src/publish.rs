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

use std::collections::BTreeMap;
use std::fmt::Write as FmtWrite;
use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::read::DecoderReader;
use serde_json::{Map, Value};
use ssri::{Algorithm, Integrity, IntegrityChecker, IntegrityOpts};

use crate::error::RegistryError;

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
        out.push(PendingAttachment { filename, data, declared_length });
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
    let integrity: Integrity = declared_integrity
        .parse()
        .map_err(|err| invalid(format!("EINTEGRITY: malformed dist.integrity: {err}")))?;
    let declared_shasum = dist.get("shasum").and_then(Value::as_str);

    let mut checker = IntegrityChecker::new(integrity);
    let mut shasum_hasher =
        declared_shasum.is_some().then(|| IntegrityOpts::new().algorithm(Algorithm::Sha1));

    let mut decoder = DecoderReader::new(Cursor::new(base64_data.as_bytes()), &BASE64);
    let mut file = File::create(dest).map_err(RegistryError::Io)?;
    const CHUNK_BYTES: usize = 64 * 1024;
    let mut buf = vec![0u8; CHUNK_BYTES];
    let mut total: u64 = 0;
    loop {
        let bytes_read = match decoder.read(&mut buf) {
            Ok(0) => break,
            Ok(bytes_read) => bytes_read,
            Err(err) => {
                let _ = std::fs::remove_file(dest);
                return Err(invalid(format!("EINTEGRITY: base64 decode failed: {err}")));
            }
        };
        let chunk = &buf[..bytes_read];
        if let Err(err) = file.write_all(chunk) {
            let _ = std::fs::remove_file(dest);
            return Err(RegistryError::Io(err));
        }
        checker.input(chunk);
        if let Some(hasher) = shasum_hasher.as_mut() {
            hasher.input(chunk);
        }
        total += bytes_read as u64;
    }

    if let Some(expected) = declared_length
        && expected != total
    {
        let _ = std::fs::remove_file(dest);
        return Err(invalid(format!(
            "EINTEGRITY: length mismatch: header says {expected}, decoded {total}",
        )));
    }

    if let Err(err) = checker.result() {
        let _ = std::fs::remove_file(dest);
        return Err(invalid(format!("EINTEGRITY: integrity mismatch: {err}")));
    }
    if let Some(declared) = declared_shasum {
        let hasher = shasum_hasher.expect("shasum_hasher initialized when declared_shasum present");
        let computed = sha1_hex_from_integrity_opts(hasher);
        if !computed.eq_ignore_ascii_case(declared) {
            let _ = std::fs::remove_file(dest);
            return Err(invalid(format!(
                "EINTEGRITY: shasum mismatch: declared {declared:?}, computed {computed:?}",
            )));
        }
    }

    if let Err(err) = file.sync_all() {
        let _ = std::fs::remove_file(dest);
        return Err(RegistryError::Io(err));
    }
    Ok(total)
}

/// Finalize a SHA-1 [`IntegrityOpts`] and re-encode the digest as a
/// 40-character lowercase hex string — the shape npm's legacy
/// `dist.shasum` field uses. ssri stores digests base64-encoded, so
/// we decode and re-encode as hex for the comparison.
fn sha1_hex_from_integrity_opts(opts: IntegrityOpts) -> String {
    let integrity = opts.result();
    let digest_base64 = integrity
        .hashes
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

/// Merge an incoming publish manifest into the existing on-disk
/// packument. The result is what we'll write back to
/// `<storage>/<pkg>/package.json`.
///
/// Merge rules (chosen to match verdaccio's behavior for the cases
/// `@pnpm/registry-mock`'s publish script exercises):
///
/// * `name`, `_id` — copied from the new body.
/// * `versions` — union, with the new body's entries taking
///   precedence when keys collide.
/// * `dist-tags` — union, new body overrides on key collision.
/// * `time` — union, new entries override on key collision.
///   `time.modified` is always bumped to "now".
/// * Other top-level keys (`description`, `readme`, `maintainers`,
///   `users`, etc.) come from the new body when present, falling
///   back to the existing packument otherwise.
pub fn merge_manifest(existing: Option<&Value>, incoming: &Value, now_iso: &str) -> Value {
    let mut out = match existing {
        Some(Value::Object(obj)) => obj.clone(),
        _ => Map::new(),
    };

    if let Some(incoming_obj) = incoming.as_object() {
        for (key, value) in incoming_obj {
            match key.as_str() {
                "versions" => {
                    let merged = merge_objects(out.get(key), value);
                    out.insert(key.clone(), merged);
                }
                "dist-tags" => {
                    let merged = merge_objects(out.get(key), value);
                    out.insert(key.clone(), merged);
                }
                "time" => {
                    let merged = merge_objects(out.get(key), value);
                    out.insert(key.clone(), merged);
                }
                "_attachments" => {
                    // Already stripped by extract_attachments; if it
                    // slips through somehow, drop it so we don't
                    // persist base64 blobs alongside the packument.
                }
                _ => {
                    out.insert(key.clone(), value.clone());
                }
            }
        }
    }

    // Synthesize time entries for any new version that didn't get
    // one supplied by the client. pnpm reads `time.modified` for
    // freshness checks, so it must always be present.
    let version_ids: Vec<String> = out
        .get("versions")
        .and_then(Value::as_object)
        .map(|versions| versions.keys().cloned().collect())
        .unwrap_or_default();
    let time_entry = out.entry("time".to_string()).or_insert_with(|| Value::Object(Map::new()));
    if let Some(time_obj) = time_entry.as_object_mut() {
        time_obj.insert("modified".to_string(), Value::String(now_iso.to_string()));
        time_obj.entry("created".to_string()).or_insert_with(|| Value::String(now_iso.to_string()));
        for version_id in version_ids {
            time_obj.entry(version_id).or_insert_with(|| Value::String(now_iso.to_string()));
        }
    }

    // Sort `versions` by semver-ish key order so the on-disk file is
    // stable across runs. Use a BTreeMap to take advantage of
    // serde_json's `preserve_order` feature — without sorting, two
    // publishes of the same package can produce different bytes.
    if let Some(versions) = out.get_mut("versions").and_then(Value::as_object_mut) {
        let sorted: BTreeMap<String, Value> =
            versions.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        *versions = sorted.into_iter().collect();
    }

    Value::Object(out)
}

fn merge_objects(existing: Option<&Value>, incoming: &Value) -> Value {
    let mut merged = match existing {
        Some(Value::Object(obj)) => obj.clone(),
        _ => Map::new(),
    };
    if let Some(incoming_obj) = incoming.as_object() {
        for (key, value) in incoming_obj {
            merged.insert(key.clone(), value.clone());
        }
    }
    Value::Object(merged)
}

/// Format the current time as an ISO-8601 / RFC-3339 string with
/// millisecond precision (e.g. `2025-01-02T03:04:05.678Z`). Matches
/// the shape npm and verdaccio use in `time.modified`.
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
pub fn iso_from_unix_millis(millis: i64) -> String {
    // Civil-time conversion without pulling in `chrono`.
    // 86_400_000 ms in a day.
    let (days, ms_in_day) = (millis / 86_400_000, millis.rem_euclid(86_400_000));
    let (h, rem) = (ms_in_day / 3_600_000, ms_in_day.rem_euclid(3_600_000));
    let (m, rem) = (rem / 60_000, rem.rem_euclid(60_000));
    let (s, ms) = (rem / 1000, rem.rem_euclid(1000));
    // Days since 1970-01-01 → year/month/day. Howard Hinnant's
    // algorithm, adapted to integer days from epoch. Shift so era
    // is positive (719_468 = days from 0000-03-01 to 1970-01-01).
    let epoch_days = days + 719_468;
    let era = epoch_days.div_euclid(146_097);
    let doe = epoch_days.rem_euclid(146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}.{ms:03}Z")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use serde_json::{Value, json};
    use ssri::{Algorithm, IntegrityOpts};
    use tempfile::TempDir;

    use super::{
        extract_attachments, merge_manifest, now_iso, sha1_hex_from_integrity_opts,
        stream_decode_verify_and_write,
    };

    fn sri_sha512(bytes: &[u8]) -> String {
        let mut opts = IntegrityOpts::new().algorithm(Algorithm::Sha512);
        opts.input(bytes);
        opts.result().to_string()
    }

    fn sha1_hex(bytes: &[u8]) -> String {
        let mut opts = IntegrityOpts::new().algorithm(Algorithm::Sha1);
        opts.input(bytes);
        sha1_hex_from_integrity_opts(opts)
    }

    fn run_stream(
        bytes: &[u8],
        dist: Option<&Value>,
        declared_length: Option<u64>,
    ) -> (Result<u64, crate::error::RegistryError>, PathBuf, TempDir) {
        let tmp = TempDir::new().unwrap();
        let dest = tmp.path().join("out.tgz");
        let b64 = BASE64.encode(bytes);
        let result =
            stream_decode_verify_and_write("foo-1.0.0.tgz", &b64, declared_length, dist, &dest);
        (result, dest, tmp)
    }

    #[test]
    fn stream_writes_matching_tarball() {
        let bytes = b"hello-world";
        let dist = json!({ "integrity": sri_sha512(bytes), "shasum": sha1_hex(bytes) });
        let (result, dest, _tmp) = run_stream(bytes, Some(&dist), Some(bytes.len() as u64));
        let written = result.unwrap();
        assert_eq!(written, bytes.len() as u64);
        assert_eq!(std::fs::read(&dest).unwrap(), bytes);
    }

    #[test]
    fn stream_rejects_integrity_mismatch_and_removes_tmp() {
        let dist = json!({ "integrity": sri_sha512(b"other-bytes") });
        let (result, dest, _tmp) = run_stream(b"actual-bytes", Some(&dist), None);
        let err = result.unwrap_err();
        assert!(err.to_string().contains("EINTEGRITY"), "got: {err}");
        assert!(!dest.exists(), "tmp file must be removed on integrity mismatch");
    }

    #[test]
    fn stream_rejects_missing_integrity() {
        let dist = json!({ "tarball": "http://example.com/foo-1.0.0.tgz" });
        let (result, _dest, _tmp) = run_stream(b"bytes", Some(&dist), None);
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("EINTEGRITY") && msg.contains("integrity"), "got: {msg}");
    }

    #[test]
    fn stream_rejects_missing_dist_entry() {
        let (result, _dest, _tmp) = run_stream(b"bytes", None, None);
        assert!(result.unwrap_err().to_string().contains("EINTEGRITY"));
    }

    #[test]
    fn stream_rejects_shasum_mismatch() {
        let bytes = b"shasum-bytes";
        let dist = json!({
            "integrity": sri_sha512(bytes),
            "shasum": "ffffffffffffffffffffffffffffffffffffffff",
        });
        let (result, dest, _tmp) = run_stream(bytes, Some(&dist), None);
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("shasum") && msg.contains("EINTEGRITY"), "got: {msg}");
        assert!(!dest.exists());
    }

    #[test]
    fn stream_rejects_declared_length_mismatch() {
        let bytes = b"len-check";
        let dist = json!({ "integrity": sri_sha512(bytes) });
        let (result, dest, _tmp) = run_stream(bytes, Some(&dist), Some(99));
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("EINTEGRITY") && msg.contains("length mismatch"), "got: {msg}");
        assert!(!dest.exists());
    }

    #[test]
    fn stream_rejects_malformed_integrity_sri() {
        let bytes = b"sri-shape";
        let dist = json!({ "integrity": "not-a-valid-sri" });
        let (result, dest, _tmp) = run_stream(bytes, Some(&dist), None);
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("EINTEGRITY") && msg.contains("malformed"), "got: {msg}");
        assert!(!dest.exists());
    }

    #[test]
    fn stream_rejects_invalid_base64() {
        let tmp = TempDir::new().unwrap();
        let dest = tmp.path().join("out.tgz");
        // Pick an SRI for some real bytes so the integrity branch
        // parses cleanly — we want the failure to surface from the
        // base64 decoder, not the integrity parser.
        let dist = json!({ "integrity": sri_sha512(b"anything") });
        let result = stream_decode_verify_and_write(
            "foo-1.0.0.tgz",
            "!!!not-valid-base64@@@",
            None,
            Some(&dist),
            &dest,
        );
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("EINTEGRITY") && msg.contains("base64"), "got: {msg}");
        assert!(!dest.exists());
    }

    /// Push the streaming loop past its 64 KiB chunk buffer. A
    /// regression where the loop accidentally returned after the
    /// first `read` (or where the hasher only saw the first chunk)
    /// would still pass every smaller-payload test in this module —
    /// this is the one that catches it.
    #[test]
    fn stream_handles_payload_larger_than_chunk_buffer() {
        let bytes = vec![0xAB; 200 * 1024];
        let dist = json!({ "integrity": sri_sha512(&bytes), "shasum": sha1_hex(&bytes) });
        let (result, dest, _tmp) = run_stream(&bytes, Some(&dist), Some(bytes.len() as u64));
        let written = result.unwrap();
        assert_eq!(written, bytes.len() as u64);
        assert_eq!(std::fs::read(&dest).unwrap(), bytes);
    }

    #[test]
    fn extracts_and_strips_attachments() {
        let mut body = json!({
            "name": "foo",
            "_attachments": {
                "foo-1.0.0.tgz": {
                    "content_type": "application/octet-stream",
                    "data": "aGVsbG8=", // "hello"
                    "length": 5,
                }
            }
        });
        let attachments = extract_attachments(&mut body).unwrap();
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].filename, "foo-1.0.0.tgz");
        assert_eq!(attachments[0].data, "aGVsbG8=");
        assert_eq!(attachments[0].declared_length, Some(5));
        assert!(body.get("_attachments").is_none(), "_attachments should be stripped");
    }

    #[test]
    fn handles_missing_attachments() {
        let mut body = json!({ "name": "foo" });
        let attachments = extract_attachments(&mut body).unwrap();
        assert!(attachments.is_empty());
    }

    #[test]
    fn merge_handles_first_publish() {
        let now = "2025-01-02T03:04:05.678Z";
        let incoming = json!({
            "name": "foo",
            "versions": { "1.0.0": { "version": "1.0.0" } },
            "dist-tags": { "latest": "1.0.0" }
        });
        let merged = merge_manifest(None, &incoming, now);
        assert_eq!(merged["name"], "foo");
        assert_eq!(merged["versions"]["1.0.0"]["version"], "1.0.0");
        assert_eq!(merged["dist-tags"]["latest"], "1.0.0");
        assert_eq!(merged["time"]["modified"], now);
        assert_eq!(merged["time"]["1.0.0"], now);
    }

    #[test]
    fn merge_preserves_existing_versions() {
        let now = "2025-01-02T03:04:05.678Z";
        let existing = json!({
            "name": "foo",
            "versions": {
                "1.0.0": { "version": "1.0.0", "dependencies": {} }
            },
            "dist-tags": { "latest": "1.0.0" },
            "time": { "1.0.0": "2024-01-01T00:00:00.000Z" }
        });
        let incoming = json!({
            "name": "foo",
            "versions": {
                "1.1.0": { "version": "1.1.0" }
            },
            "dist-tags": { "latest": "1.1.0" }
        });
        let merged = merge_manifest(Some(&existing), &incoming, now);
        let versions = merged["versions"].as_object().unwrap();
        assert!(versions.contains_key("1.0.0"));
        assert!(versions.contains_key("1.1.0"));
        assert_eq!(merged["dist-tags"]["latest"], "1.1.0");
        assert_eq!(merged["time"]["1.0.0"], "2024-01-01T00:00:00.000Z"); // preserved
        assert_eq!(merged["time"]["1.1.0"], now); // synthesized
        assert_eq!(merged["time"]["modified"], now); // bumped
    }

    #[test]
    fn now_iso_has_expected_shape() {
        let now = now_iso();
        let bytes = now.as_bytes();
        assert_eq!(bytes.len(), 24);
        assert_eq!(bytes[4], b'-');
        assert_eq!(bytes[7], b'-');
        assert_eq!(bytes[10], b'T');
        assert_eq!(bytes[13], b':');
        assert_eq!(bytes[16], b':');
        assert_eq!(bytes[19], b'.');
        assert_eq!(bytes[23], b'Z');
    }

    #[test]
    fn merge_drops_attachments_if_present() {
        let incoming = json!({
            "name": "foo",
            "_attachments": { "f.tgz": { "data": "..." } }
        });
        let merged: Value = merge_manifest(None, &incoming, "now");
        assert!(merged.get("_attachments").is_none());
    }
}
