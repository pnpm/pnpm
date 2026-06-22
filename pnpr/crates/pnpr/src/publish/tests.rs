use super::{
    extract_attachments, merge_manifest, now_iso, sha1_hex_from_integrity_opts,
    stream_decode_verify_and_write,
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Value, json};
use ssri::{Algorithm, IntegrityOpts};
use std::path::PathBuf;
use tempfile::TempDir;

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
fn stream_rejects_integrity_without_hashes() {
    for declared in ["", " \t\n "] {
        let dist = json!({ "integrity": declared });
        let (result, dest, _tmp) = run_stream(b"bytes", Some(&dist), None);
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("EINTEGRITY") && msg.contains("hash"), "got: {msg}");
        assert!(!dest.exists());
    }
}

#[test]
fn stream_rejects_unsupported_integrity_algorithm() {
    let dist = json!({ "integrity": "md5-deadbeef" });
    let (result, dest, _tmp) = run_stream(b"bytes", Some(&dist), None);
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
