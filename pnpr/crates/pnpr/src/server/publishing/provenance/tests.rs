use super::{sha512_hex, statement_subjects};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::json;

fn bundle(subjects: &serde_json::Value) -> String {
    let statement = json!({
        "_type": "https://in-toto.io/Statement/v1",
        "subject": subjects,
        "predicateType": "https://slsa.dev/provenance/v1",
        "predicate": {},
    });
    json!({
        "mediaType": "application/vnd.dev.sigstore.bundle.v0.3+json",
        "dsseEnvelope": {
            "payload": BASE64.encode(statement.to_string()),
            "payloadType": "application/vnd.in-toto+json",
            "signatures": [],
        },
    })
    .to_string()
}

#[test]
fn reads_subjects_from_a_bundle_as_json_or_base64() {
    let subjects = json!([
        { "name": "pkg:npm/a@1.0.0", "digest": { "sha512": "AB" } },
        { "name": "pkg:npm/a@1.0.0?variant=x", "digest": { "sha256": "cd" } },
    ]);
    let raw = bundle(&subjects);
    let expected = vec![("pkg:npm/a@1.0.0".to_string(), "ab".to_string())];
    assert_eq!(statement_subjects(&raw).unwrap(), expected);
    assert_eq!(statement_subjects(&BASE64.encode(&raw)).unwrap(), expected);
    assert!(statement_subjects("{}").unwrap_err().contains("no DSSE envelope"));
}

#[test]
fn converts_an_sri_sha512_to_hex() {
    let digest = [0xab_u8; 64];
    let integrity = format!("sha512-{}", BASE64.encode(digest));
    assert_eq!(sha512_hex(Some(&integrity)).unwrap(), "ab".repeat(64));
    assert_eq!(sha512_hex(Some("sha1-AAAA")), None);
    assert_eq!(sha512_hex(None), None);
}
