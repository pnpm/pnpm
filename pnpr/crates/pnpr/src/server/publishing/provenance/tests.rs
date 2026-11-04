use super::{Purl, names, sha512_hex, statement_subjects};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::json;

fn bundle(subjects: &serde_json::Value) -> String {
    typed_bundle(subjects, "https://in-toto.io/Statement/v1", "application/vnd.in-toto+json")
}

fn typed_bundle(subjects: &serde_json::Value, statement_type: &str, payload_type: &str) -> String {
    let statement = json!({
        "_type": statement_type,
        "subject": subjects,
        "predicateType": "https://slsa.dev/provenance/v1",
        "predicate": {},
    });
    json!({
        "mediaType": "application/vnd.dev.sigstore.bundle.v0.3+json",
        "dsseEnvelope": {
            "payload": BASE64.encode(statement.to_string()),
            "payloadType": payload_type,
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
    let other = format!("sha512-{}", BASE64.encode([0xcd_u8; 64]));
    assert_eq!(sha512_hex(Some(&format!("{integrity} {other}"))), None);
    assert_eq!(sha512_hex(None), None);
}

#[test]
fn refuses_a_bundle_that_is_not_an_in_toto_statement() {
    let subjects = json!([{ "name": "pkg:npm/a@1.0.0", "digest": { "sha512": "ab" } }]);
    let older = typed_bundle(
        &subjects,
        "https://in-toto.io/Statement/v0.1",
        "application/vnd.in-toto+json",
    );
    assert!(statement_subjects(&older).is_ok());
    let payload = typed_bundle(&subjects, "https://in-toto.io/Statement/v1", "text/plain");
    assert!(statement_subjects(&payload).unwrap_err().contains("payload type"));
    let statement =
        typed_bundle(&subjects, "https://example.com/Other", "application/vnd.in-toto+json");
    assert!(statement_subjects(&statement).unwrap_err().contains("statement type"));
}

#[test]
fn names_an_archive_by_any_spelling_of_its_name_but_not_of_its_variant() {
    let legacy = Purl::npm("legacy!pkg", "1.0.0").unwrap();
    assert!(names("pkg:npm/legacy!pkg@1.0.0", &legacy));
    assert!(names("pkg:npm/legacy%21pkg@1.0.0", &legacy));
    assert!(!names("pkg:npm/legacy!pkg@1.0.1", &legacy));
    let variant = Purl::npm_variant("legacy!pkg", "1.0.0", "linux-x64+linux-arm64").unwrap();
    assert!(names("pkg:npm/legacy!pkg@1.0.0?variant=linux-x64%2Blinux-arm64", &variant));
    assert!(!names("pkg:npm/legacy!pkg@1.0.0?variant=linux-x64+linux-arm64", &variant));
    assert!(!names("pkg:npm/legacy!pkg@1.0.0", &variant));
}

#[test]
fn reads_a_url_safe_unpadded_payload_and_skips_unnamed_subjects() {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    let statement = json!({
        "_type": "https://in-toto.io/Statement/v1",
        "subject": [
            { "digest": { "sha512": "cd" } },
            { "name": "pkg:npm/a@1.0.0", "digest": { "sha512": "ab" } },
        ],
        "predicateType": "https://slsa.dev/provenance/v1",
        "predicate": { "note": "~~~???>>>" },
    });
    let payload = URL_SAFE_NO_PAD.encode(statement.to_string());
    assert!(payload.contains(['-', '_']), "the payload uses the URL-safe alphabet");
    let bundle = json!({
        "dsseEnvelope": {
            "payload": payload,
            "payloadType": "application/vnd.in-toto+json",
            "signatures": [],
        },
    })
    .to_string();
    assert_eq!(
        statement_subjects(&bundle).unwrap(),
        vec![("pkg:npm/a@1.0.0".to_string(), "ab".to_string())],
    );
}
