use super::{extract_predicate_type, write_sigstore_bundle};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use tempfile::TempDir;

#[test]
fn writes_raw_json_bundle() {
    let tmp = TempDir::new().unwrap();
    let dest = tmp.path().join("bundle.sigstore");
    let json_str = r#"{"mediaType":"application/vnd.dev.sigstore.bundle.v0.3+json","dsseEnvelope":{"payload":"","payloadType":""}}"#;
    let written =
        write_sigstore_bundle("test.sigstore", json_str, Some(json_str.len() as u64), &dest)
            .unwrap();
    assert_eq!(written, json_str.len() as u64);
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), json_str);
}

#[test]
fn writes_raw_json_with_whitespace() {
    let tmp = TempDir::new().unwrap();
    let dest = tmp.path().join("bundle.sigstore");
    let json_str = "{\n  \"mediaType\": \"application/vnd.dev.sigstore.bundle.v0.3+json\"\n}\n";
    let written =
        write_sigstore_bundle("test.sigstore", json_str, Some(json_str.len() as u64), &dest)
            .unwrap();
    assert_eq!(written, json_str.len() as u64);
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), json_str);
}

#[test]
fn writes_base64_encoded_bundle() {
    let tmp = TempDir::new().unwrap();
    let dest = tmp.path().join("bundle.sigstore");
    let raw = r#"{"mediaType":"application/vnd.dev.sigstore.bundle.v0.3+json"}"#;
    let b64 = BASE64.encode(raw.as_bytes());
    let written =
        write_sigstore_bundle("test.sigstore", &b64, Some(raw.len() as u64), &dest).unwrap();
    assert_eq!(written, raw.len() as u64);
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), raw);
}

#[test]
fn rejects_non_object_bundle() {
    let tmp = TempDir::new().unwrap();
    let dest = tmp.path().join("bundle.sigstore");
    assert!(write_sigstore_bundle("test.sigstore", "123", None, &dest).is_err());
    assert!(write_sigstore_bundle("test.sigstore", "invalid json", None, &dest).is_err());
}

#[test]
fn extracts_predicate_type_from_dsse_envelope() {
    let statement = r#"{"_type":"https://in-toto.io/Statement/v1","predicateType":"https://slsa.dev/provenance/v0.2"}"#;
    let payload = BASE64.encode(statement.as_bytes());
    let bundle = serde_json::json!({
        "dsseEnvelope": {
            "payload": payload,
            "payloadType": "application/vnd.in-toto+json"
        }
    });
    let bundle_str = serde_json::to_string(&bundle).unwrap();
    assert_eq!(extract_predicate_type(&bundle_str), "https://slsa.dev/provenance/v0.2");
}

#[test]
fn defaults_predicate_type_when_missing() {
    let bundle_str = r#"{"other": "data"}"#;
    assert_eq!(extract_predicate_type(bundle_str), "https://slsa.dev/provenance/v1");
}
