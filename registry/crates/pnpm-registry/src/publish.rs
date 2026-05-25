//! Helpers for the `PUT /:pkg` publish endpoint.
//!
//! npm sends the entire packument plus base64-encoded tarballs in a
//! single JSON body. This module decodes those attachments, merges
//! the incoming manifest into whatever packument is already on disk,
//! and returns the bytes that should be written back. The actual I/O
//! happens in the server handler — keeping the side-effecting code
//! isolated keeps these helpers easy to unit-test.

use std::collections::BTreeMap;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::{Map, Value};

use crate::error::RegistryError;

/// One decoded attachment from a publish body.
#[derive(Debug)]
pub struct Attachment {
    pub filename: String,
    pub bytes: Vec<u8>,
}

/// Pull all `_attachments` out of a publish body and base64-decode
/// the tarballs. Returns the attachments and removes the
/// `_attachments` field from `body` so the saved packument doesn't
/// duplicate the on-disk tarball. Length mismatches between the
/// declared `length` and the decoded body surface as a 400.
pub fn extract_attachments(body: &mut Value) -> Result<Vec<Attachment>, RegistryError> {
    let Some(obj) = body.as_object_mut() else {
        return Ok(Vec::new());
    };
    let Some(attachments_value) = obj.remove("_attachments") else {
        return Ok(Vec::new());
    };
    let Some(attachments) = attachments_value.as_object() else {
        return Err(RegistryError::BadRequest {
            reason: "_attachments must be an object".to_string(),
        });
    };
    let mut out = Vec::with_capacity(attachments.len());
    for (filename, value) in attachments {
        let Some(value_obj) = value.as_object() else {
            return Err(RegistryError::InvalidAttachment {
                filename: filename.clone(),
                reason: "expected object".to_string(),
            });
        };
        let data = value_obj.get("data").and_then(Value::as_str).ok_or_else(|| {
            RegistryError::InvalidAttachment {
                filename: filename.clone(),
                reason: "missing string field `data`".to_string(),
            }
        })?;
        let bytes = BASE64.decode(data).map_err(|err| RegistryError::InvalidAttachment {
            filename: filename.clone(),
            reason: format!("base64 decode failed: {err}"),
        })?;
        if let Some(expected) = value_obj.get("length").and_then(Value::as_u64) {
            // npm sends both the length and the data; cross-check to
            // catch a truncated upload before we write it.
            if expected != bytes.len() as u64 {
                return Err(RegistryError::InvalidAttachment {
                    filename: filename.clone(),
                    reason: format!(
                        "length mismatch: header says {expected}, decoded {}",
                        bytes.len(),
                    ),
                });
            }
        }
        out.push(Attachment { filename: filename.clone(), bytes });
    }
    Ok(out)
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
    let millis = since_epoch.as_millis() as i64;
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
    use super::{extract_attachments, merge_manifest, now_iso};
    use serde_json::{Value, json};

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
        assert_eq!(attachments[0].bytes, b"hello");
        assert!(body.get("_attachments").is_none(), "_attachments should be stripped");
    }

    #[test]
    fn rejects_length_mismatch() {
        let mut body = json!({
            "_attachments": {
                "f.tgz": { "data": "aGVsbG8=", "length": 99 }
            }
        });
        assert!(extract_attachments(&mut body).is_err());
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
