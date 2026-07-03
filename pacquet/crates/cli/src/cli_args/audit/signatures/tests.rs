use super::{
    PackageSignature, RegistryKey, SignaturePackage, SignatureVerificationResult, parse_timestamp,
    render_signature_verification_result, verify_one, verify_package_signatures,
};
use base64::Engine as _;
use p256::ecdsa::SigningKey;
use pacquet_network::encode_package_name;

fn signing_key() -> SigningKey {
    SigningKey::from_slice(&[0x42; 32]).expect("valid P-256 scalar")
}

fn public_key_b64(key: &SigningKey) -> String {
    use p256::pkcs8::EncodePublicKey;
    let der = key.verifying_key().to_public_key_der().expect("encode SPKI");
    base64::engine::general_purpose::STANDARD.encode(der.as_bytes())
}

fn sign_b64(key: &SigningKey, message: &str) -> String {
    use p256::ecdsa::{Signature, signature::Signer};
    let signature: Signature = key.sign(message.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(signature.to_der().as_bytes())
}

fn package() -> SignaturePackage {
    SignaturePackage {
        name: "foo".to_string(),
        registry: "https://registry.example.com/".to_string(),
        version: "1.0.0".to_string(),
    }
}

fn ecdsa_key(key: &SigningKey, keyid: impl Into<String>, expires: Option<&str>) -> RegistryKey {
    RegistryKey {
        expires: expires.map(str::to_string),
        key: public_key_b64(key),
        keyid: keyid.into(),
        keytype: "ecdsa-sha2-nistp256".to_string(),
        scheme: "ecdsa-sha2-nistp256".to_string(),
    }
}

fn signature(keyid: impl Into<String>, sig: impl Into<String>) -> PackageSignature {
    PackageSignature { keyid: keyid.into(), sig: sig.into() }
}

#[test]
fn verify_one_accepts_only_the_signed_message() {
    let key = signing_key();
    let public = public_key_b64(&key);
    let message = "foo@1.0.0:sha512-abc";

    assert!(verify_one(&public, message, &sign_b64(&key, message)));
    assert!(!verify_one(&public, message, &sign_b64(&key, "foo@1.0.0:other")));
    assert!(!verify_one("not base64 ~~~", message, "also not base64"));
}

#[test]
fn valid_signature_verifies() {
    let key = signing_key();
    let package = package();
    let integrity = "sha512-abc";
    let message = format!("{}@{}:{integrity}", package.name, package.version);
    let signatures = vec![signature("k1", sign_b64(&key, &message))];
    let keys = vec![ecdsa_key(&key, "k1", None)];

    assert!(
        verify_package_signatures(&package, integrity, None, None, &signatures, &keys).is_none(),
    );
}

#[test]
fn unknown_key_yields_an_unknown_key_reason() {
    let package = package();
    let signatures = vec![signature("nope", "AA==")];

    let issue =
        verify_package_signatures(&package, "sha512-abc", None, None, &signatures, &[]).unwrap();
    let reason = issue.reason.unwrap();
    assert!(reason.contains("no corresponding public key"), "{reason}");
}

#[test]
fn invalid_signature_reason_is_preferred_over_unknown_key() {
    let key = signing_key();
    let package = package();
    let integrity = "sha512-abc";
    let tampered = sign_b64(&key, "foo@1.0.0:tampered");
    let signatures = vec![signature("missing", "AA=="), signature("k1", &tampered)];
    let keys = vec![ecdsa_key(&key, "k1", None)];

    let issue =
        verify_package_signatures(&package, integrity, None, None, &signatures, &keys).unwrap();
    let reason = issue.reason.unwrap();
    assert!(reason.contains("invalid registry signature"), "{reason}");
}

#[test]
fn key_expiry_gates_only_when_published_after_expiry() {
    let key = signing_key();
    let package = package();
    let integrity = "sha512-abc";
    let message = format!("foo@1.0.0:{integrity}");
    let signatures = vec![signature("k1", sign_b64(&key, &message))];
    let keys = vec![ecdsa_key(&key, "k1", Some("2020-01-01T00:00:00.000Z"))];

    let published_after = verify_package_signatures(
        &package,
        integrity,
        Some("2021-01-01T00:00:00.000Z"),
        None,
        &signatures,
        &keys,
    );
    assert!(published_after.unwrap().reason.unwrap().contains("expired"));

    assert!(
        verify_package_signatures(
            &package,
            integrity,
            Some("2019-01-01T00:00:00.000Z"),
            None,
            &signatures,
            &keys
        )
        .is_none(),
        "a key not yet expired at publish time stays usable",
    );
    assert!(
        verify_package_signatures(&package, integrity, None, None, &signatures, &keys).is_none(),
        "a missing publish time keeps the key usable",
    );
}

#[test]
fn encode_package_name_matches_encode_uri_component() {
    assert_eq!(encode_package_name("lodash"), "lodash");
    assert_eq!(encode_package_name("a.b-c_d"), "a.b-c_d");
    assert_eq!(encode_package_name("@scope/pkg"), "@scope%2Fpkg");
}

#[test]
fn parse_timestamp_accepts_iso_and_rejects_garbage() {
    assert!(parse_timestamp("2020-01-01T00:00:00.000Z").is_some());
    assert!(parse_timestamp("nonsense").is_none());
}

#[test]
fn render_announces_absence_of_signing_keys() {
    let output = render_signature_verification_result(&SignatureVerificationResult::default());
    assert!(output.contains("audited 0 packages"), "{output}");
    assert!(
        output.contains("No dependencies were installed from a registry with signing keys"),
        "{output}",
    );
}
