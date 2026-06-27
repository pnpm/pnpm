use super::{
    EngineComponent, NpmSigningKey, PackageSignature, plain_version, signature_validates_against,
    verify_one,
};
use base64::Engine as _;
use p256::ecdsa::SigningKey;
use pacquet_lockfile::SnapshotDepRef;

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

fn component() -> EngineComponent {
    EngineComponent {
        name: "pnpm".to_string(),
        registry: "https://registry.example.com/".to_string(),
        version: "12.0.0".to_string(),
        integrity: "sha512-deadbeef".to_string(),
    }
}

fn signed_message(component: &EngineComponent) -> String {
    format!("{}@{}:{}", component.name, component.version, component.integrity)
}

#[test]
fn verify_one_accepts_only_a_genuine_signature() {
    let key = signing_key();
    let pub_b64 = public_key_b64(&key);
    let message = "pnpm@12.0.0:sha512-deadbeef";
    let sig = sign_b64(&key, message);

    assert!(verify_one(&pub_b64, message, &sig), "a genuine signature validates");
    // A signature over different bytes must not validate the message.
    assert!(!verify_one(&pub_b64, "pnpm@12.0.1:sha512-deadbeef", &sig));
    // Malformed key / signature material is a non-match, not a panic.
    assert!(!verify_one("not-base64!!", message, &sig));
    assert!(!verify_one(&pub_b64, message, "not-base64!!"));
}

#[test]
fn signature_validates_accepts_a_trusted_unexpired_key() {
    let key = signing_key();
    let pub_b64 = public_key_b64(&key);
    let component = component();
    let keys = [NpmSigningKey { keyid: "SHA256:test", key: &pub_b64, expires: None }];
    let signatures = [PackageSignature {
        keyid: "SHA256:test".to_string(),
        sig: sign_b64(&key, &signed_message(&component)),
    }];
    assert!(signature_validates_against(&component, &signatures, None, &keys));
}

#[test]
fn signature_validates_rejects_an_expired_key() {
    let key = signing_key();
    let pub_b64 = public_key_b64(&key);
    let component = component();
    let keys = [NpmSigningKey {
        keyid: "SHA256:test",
        key: &pub_b64,
        expires: Some("2000-01-01T00:00:00.000Z"),
    }];
    let signatures = [PackageSignature {
        keyid: "SHA256:test".to_string(),
        sig: sign_b64(&key, &signed_message(&component)),
    }];
    // Published after the key expired, so even a valid signature is rejected.
    assert!(!signature_validates_against(
        &component,
        &signatures,
        Some("2020-01-01T00:00:00.000Z"),
        &keys,
    ));
}

#[test]
fn signature_validates_rejects_unknown_keyid_and_empty_signatures() {
    let key = signing_key();
    let pub_b64 = public_key_b64(&key);
    let component = component();
    let keys = [NpmSigningKey { keyid: "SHA256:test", key: &pub_b64, expires: None }];

    let unknown = [PackageSignature {
        keyid: "SHA256:unknown".to_string(),
        sig: sign_b64(&key, &signed_message(&component)),
    }];
    assert!(!signature_validates_against(&component, &unknown, None, &keys));
    assert!(!signature_validates_against(&component, &[], None, &keys));
}

#[test]
fn plain_version_reads_only_plain_references() {
    let plain: SnapshotDepRef = "1.2.3".parse().expect("parse plain ref");
    assert_eq!(plain_version(&plain), Some("1.2.3".to_string()));

    let alias: SnapshotDepRef = "foo@1.2.3".parse().expect("parse alias ref");
    assert_eq!(plain_version(&alias), None);

    let link = SnapshotDepRef::Link("packages/x".to_string());
    assert_eq!(plain_version(&link), None);
}
