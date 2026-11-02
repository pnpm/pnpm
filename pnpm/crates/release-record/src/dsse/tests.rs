use super::{DsseError, Envelope, IN_TOTO_PAYLOAD_TYPE, pae};
use crate::test_support::{key, registry};
use pnpm_transparency_log::{Cosigner, KeySet, NoteSigner, VerifierKey};
use pretty_assertions::assert_eq;

/// The statement key of [`registry`] and an envelope it signed, produced by
/// an independent DSSE implementation on pyca/cryptography, with a URL-safe,
/// unpadded payload.
const PYTHON_ENVELOPE: &str = include_str!("../../tests/fixtures/python-dsse-envelope.txt");

#[test]
fn encodes_the_dsse_specification_test_vector() {
    assert_eq!(
        pae("http://example.com/HelloWorld", b"hello world"),
        b"DSSEv1 29 http://example.com/HelloWorld 11 hello world",
    );
}

#[test]
fn verifies_an_envelope_signed_by_another_implementation() {
    let (key, envelope) = PYTHON_ENVELOPE.split_once('\n').unwrap();
    let key: VerifierKey = key.parse().unwrap();
    assert_eq!(key, registry().statements.verifier_key().clone());
    let envelope: Envelope = serde_json::from_str(envelope.trim_end()).unwrap();
    let verified = envelope
        .verify(&KeySet::new(vec![key], 1).unwrap())
        .unwrap();
    assert_eq!(verified.payload_type, IN_TOTO_PAYLOAD_TYPE);
    assert!(verified.body.starts_with(br#"{"_type":"https://in-toto.io/Statement/v1""#));
}

fn signers() -> (NoteSigner, NoteSigner, NoteSigner) {
    (
        NoteSigner::new("statements.example/one", key(1)).unwrap(),
        NoteSigner::new("statements.example/two", key(2)).unwrap(),
        NoteSigner::new("statements.example/three", key(3)).unwrap(),
    )
}

#[test]
fn counts_distinct_trusted_keys_toward_the_threshold() {
    let (one, two, three) = signers();
    let keys =
        KeySet::new(vec![one.verifier_key().clone(), two.verifier_key().clone()], 2).unwrap();
    let both = Envelope::sign("text/plain", b"body", &[&one, &two]);
    assert_eq!(both.verify(&keys).unwrap().body, b"body");

    let twice = Envelope::sign("text/plain", b"body", &[&one, &one]);
    assert_eq!(twice.verify(&keys), Err(DsseError::Threshold { found: 1, required: 2 }));

    let untrusted = Envelope::sign("text/plain", b"body", &[&one, &three]);
    assert_eq!(untrusted.verify(&keys), Err(DsseError::Threshold { found: 1, required: 2 }));
}

#[test]
fn signs_the_payload_type_too() {
    let (one, ..) = signers();
    let keys = KeySet::new(vec![one.verifier_key().clone()], 1).unwrap();
    let mut envelope = Envelope::sign("text/plain", b"body", &[&one]);
    envelope.payload_type = IN_TOTO_PAYLOAD_TYPE.to_owned();
    assert_eq!(envelope.verify(&keys), Err(DsseError::Threshold { found: 0, required: 1 }));
}

#[test]
fn ignores_a_signature_whose_keyid_names_another_key() {
    let (one, two, _) = signers();
    let keys = KeySet::new(vec![one.verifier_key().clone()], 1).unwrap();
    let mut envelope = Envelope::sign("text/plain", b"body", &[&one]);
    envelope.signatures[0].keyid = two.verifier_key().key_name();
    assert!(envelope.verify(&keys).is_err());
    envelope.signatures[0].keyid = String::new();
    assert!(envelope.verify(&keys).is_ok());
}

#[test]
fn rejects_cosignature_keys_and_bad_encodings() {
    let witness = Cosigner::new("witness.example", key(4)).unwrap();
    let keys = KeySet::new(vec![witness.verifier_key().clone()], 1).unwrap();
    let (one, ..) = signers();
    let envelope = Envelope::sign("text/plain", b"body", &[&one]);
    assert!(matches!(envelope.verify(&keys), Err(DsseError::InvalidKey { .. })));

    let keys = KeySet::new(vec![one.verifier_key().clone()], 1).unwrap();
    let mut garbled = envelope;
    garbled.payload = "not base64!".to_owned();
    assert_eq!(garbled.verify(&keys), Err(DsseError::Encoding));
}
