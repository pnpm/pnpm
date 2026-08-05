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

use crate::{
    error::RegistryError,
    streaming::{integrity_checker, parse_integrity},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64, read::DecoderReader};
use serde_json::{Map, Value};
use ssri::{Algorithm, IntegrityOpts};
use std::{
    collections::BTreeMap,
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
    let integrity = parse_integrity(declared_integrity)
        .map_err(|err| invalid(format!("EINTEGRITY: malformed dist.integrity: {err}")))?;
    let declared_shasum = dist.get("shasum").and_then(Value::as_str);

    let mut checker = integrity_checker(&integrity)
        .map_err(|err| invalid(format!("EINTEGRITY: malformed dist.integrity: {err}")))?;
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
/// * `versions` — union; an already-hosted version is immutable except for
///   its `deprecated` flag (see [`merge_versions`]), an upstream-only or
///   new version is taken from the body. `hosted` is the locally hosted
///   packument (or `None`); `existing` is the merge seed, which may instead
///   be the upstream packument, so immutability keys off `hosted`, not it.
/// * `dist-tags` — union, new body overrides on key collision.
/// * `time` — union, new entries override on key collision.
///   `time.modified` is always bumped to "now".
/// * Other top-level keys (`description`, `readme`, `maintainers`,
///   `users`, etc.) come from the new body when present, falling
///   back to the existing packument otherwise.
pub fn merge_manifest(
    existing: Option<&Value>,
    incoming: &Value,
    hosted: Option<&Value>,
    now_iso: &str,
) -> Value {
    let mut out = match existing {
        Some(Value::Object(obj)) => obj.clone(),
        _ => Map::new(),
    };

    if let Some(incoming_obj) = incoming.as_object() {
        for (key, value) in incoming_obj {
            match key.as_str() {
                "versions" => {
                    let merged = merge_versions(out.get(key), value, hosted);
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

    hoist_readme_from_latest(&mut out);

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

/// Copy the `readme` / `readmeFilename` of the `dist-tags.latest` version up to
/// the packument top level, where full-packument consumers and registry UIs
/// read it. Publish clients (`libnpmpublish`, pacquet) only send the readme
/// inside `versions[<version>]`, so without this a published package would
/// expose no top-level readme — this hoist matches npm and verdaccio.
///
/// A top-level value already present (e.g. from an earlier publish or the
/// upstream packument) is left in place when the latest version carries no
/// readme, so a metadata-only re-publish never blanks it.
fn hoist_readme_from_latest(out: &mut Map<String, Value>) {
    let Some(latest) = out
        .get("dist-tags")
        .and_then(|tags| tags.get("latest"))
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return;
    };
    let hoisted: Vec<(String, Value)> = out
        .get("versions")
        .and_then(|versions| versions.get(&latest))
        .map(|version| {
            ["readme", "readmeFilename"]
                .into_iter()
                .filter_map(|key| {
                    version
                        .get(key)
                        .filter(|value| !value.is_null())
                        .cloned()
                        .map(|value| (key.to_owned(), value))
                })
                .collect()
        })
        .unwrap_or_default();
    for (key, value) in hoisted {
        out.insert(key, value);
    }
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

/// Merge the `versions` map onto the seed `existing`. A version that is
/// already **hosted** is immutable except for its `deprecated` flag: the
/// hosted manifest is kept and only `deprecated` is applied from the body
/// (set it, or remove it for undeprecate), so `dist`, `dependencies`, `bin`,
/// `engines` and every other resolution-relevant field stay as published. A
/// malformed incoming entry for a hosted version is ignored. Any other
/// version — brand-new, or one that exists only upstream in the seed — is
/// taken from the body, so its manifest matches the tarball being published.
///
/// Immutability keys off `hosted` (the locally hosted packument), not the
/// seed: `existing` may be the upstream packument, and upstream versions are
/// not immutable here — a first local publish of one must win.
fn merge_versions(existing: Option<&Value>, incoming: &Value, hosted: Option<&Value>) -> Value {
    let hosted_versions = hosted.and_then(|h| h.get("versions")).and_then(Value::as_object);
    let mut merged = match existing {
        Some(Value::Object(obj)) => obj.clone(),
        _ => Map::new(),
    };
    if let Some(incoming_obj) = incoming.as_object() {
        for (version, manifest) in incoming_obj {
            let hosted_manifest = hosted_versions
                .and_then(|versions| versions.get(version))
                .and_then(Value::as_object);
            match (hosted_manifest, manifest.as_object()) {
                (Some(hosted_manifest), Some(incoming_manifest)) => {
                    let mut updated = hosted_manifest.clone();
                    match incoming_manifest.get("deprecated") {
                        Some(deprecated) => {
                            updated.insert("deprecated".to_string(), deprecated.clone());
                        }
                        None => {
                            updated.remove("deprecated");
                        }
                    }
                    merged.insert(version.clone(), Value::Object(updated));
                }
                // Malformed incoming entry for a hosted version: keep the
                // hosted manifest rather than overwrite it with junk.
                (Some(hosted_manifest), None) => {
                    merged.insert(version.clone(), Value::Object(hosted_manifest.clone()));
                }
                _ => {
                    merged.insert(version.clone(), manifest.clone());
                }
            }
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
    let year = i64::from(yoe) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}.{ms:03}Z")
}

#[cfg(test)]
mod tests;
