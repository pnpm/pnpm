//! Vectors from Go's `golang.org/x/mod/sumdb/note` tests.

use super::{Cosigner, KeyAlgorithm, KeyError, NoteSigner, VerifierKey, cosignature_message};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::SigningKey;
use pretty_assertions::assert_eq;
use signed_note::Signer as _;

const PETER_VERIFIER_KEY: &str =
    "PeterNeumann+c74f20a3+ARpc2QcUPDhMQegwxbzhKqiBfsVkmqq/LDE4izWy10TW";
const PETER_PRIVATE_KEY: &str =
    "PRIVATE+KEY+PeterNeumann+c74f20a3+AYEKFALVFGyNhPJEMzD1QIDr+Y7hfZx09iUvxdXHKDFz";

#[test]
fn parses_and_formats_a_go_verifier_key() {
    let key: VerifierKey = PETER_VERIFIER_KEY.parse().unwrap();
    assert_eq!(key.name(), "PeterNeumann");
    assert_eq!(key.key_id(), 0xc74f_20a3);
    assert_eq!(key.algorithm(), KeyAlgorithm::Ed25519);
    assert_eq!(key.key_name(), "PeterNeumann+c74f20a3");
    assert_eq!(key.to_string(), PETER_VERIFIER_KEY);
}

#[test]
fn rejects_every_truncation_and_mangling_of_a_verifier_key() {
    let bytes = PETER_VERIFIER_KEY.as_bytes();
    for start in 0..=bytes.len() {
        for end in start + 1..=bytes.len() {
            if start == 0 && end == bytes.len() {
                continue;
            }
            let slice = String::from_utf8_lossy(&bytes[start..end]);
            assert!(slice.parse::<VerifierKey>().is_err(), "accepted {slice:?}");
        }
    }
    for position in 0..bytes.len() {
        let mut mangled = bytes.to_vec();
        mangled[position] += 1;
        let mangled = String::from_utf8_lossy(&mangled);
        assert!(mangled.parse::<VerifierKey>().is_err(), "accepted {mangled:?}");
    }
}

#[test]
fn rejects_go_bad_keys() {
    let wrong_length = "PeterNeumann+cc469956+ARpc2QcUPDhMQegwxbzhKqiBfsVkmqq/LDE4izWy10TWBADKEY==";
    assert!(wrong_length.parse::<VerifierKey>().is_err());
    let unknown_type = "PeterNeumann+173116ae+ZRpc2QcUPDhMQegwxbzhKqiBfsVkmqq/LDE4izWy10TW";
    assert_eq!(
        unknown_type.parse::<VerifierKey>(),
        Err(KeyError::UnsupportedType { signature_type: 0x65 }),
    );
}

#[test]
fn reads_a_go_private_key_and_signs_as_go_does() {
    let signer = NoteSigner::from_private_key(PETER_PRIVATE_KEY).unwrap();
    assert_eq!(signer.verifier_key().to_string(), PETER_VERIFIER_KEY);
    let text = b"If you think cryptography is the answer to your problem,\n\
        then you don't know what your problem is.\n";
    let signature = signer.sign(text).unwrap();
    let mut line = signer.key_id().to_be_bytes().to_vec();
    line.extend_from_slice(&signature);
    assert_eq!(
        STANDARD.encode(line),
        "x08go/ZJkuBS9UG/SffcvIAQxVBtiFupLLr8pAcElZInNIuGUgYN1FFYC2pZSNXgKvqfqdngotpRZb6KE6RyyBwJnAM=",
    );
}

#[test]
fn rejects_every_truncation_of_a_private_key() {
    let bytes = PETER_PRIVATE_KEY.as_bytes();
    for end in 1..bytes.len() {
        let slice = std::str::from_utf8(&bytes[..end]).unwrap();
        assert!(NoteSigner::from_private_key(slice).is_err(), "accepted {slice:?}");
    }
}

#[test]
fn cosigner_key_uses_the_cosignature_type() {
    let cosigner = Cosigner::new("witness.example/ci", SigningKey::from_bytes(&[7; 32])).unwrap();
    let key = cosigner.verifier_key();
    assert_eq!(key.algorithm(), KeyAlgorithm::CosignatureV1);
    let encoded = key.to_string();
    let reparsed: VerifierKey = encoded.parse().unwrap();
    assert_eq!(&reparsed, key);
    let ed25519 = VerifierKey::new(key.name(), KeyAlgorithm::Ed25519, *key.public_key()).unwrap();
    assert!(ed25519.key_id() != key.key_id(), "the signature type is part of the key hash");
}

#[test]
fn cosignature_message_prefixes_the_checkpoint() {
    assert_eq!(
        cosignature_message(1_679_315_147, b"example.com/behind-the-sofa\n20852163\nCsUY=\n"),
        b"cosignature/v1\ntime 1679315147\nexample.com/behind-the-sofa\n20852163\nCsUY=\n",
    );
}

#[test]
fn rejects_invalid_names() {
    let public_key = SigningKey::from_bytes(&[1; 32]).verifying_key();
    for name in ["", "has space", "has+plus"] {
        assert_eq!(
            VerifierKey::new(name, KeyAlgorithm::Ed25519, public_key),
            Err(KeyError::InvalidName),
        );
    }
}
