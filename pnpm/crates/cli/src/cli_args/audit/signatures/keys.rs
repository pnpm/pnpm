//! Checking a registry signature against a registry key.

use super::{RegistryKey, parse_timestamp};
use base64::Engine as _;
use p256::{
    ecdsa::{Signature, VerifyingKey, signature::Verifier},
    pkcs8::DecodePublicKey,
};

pub(super) fn key_is_expired(key: &RegistryKey, published_time: Option<i64>) -> bool {
    // Key expiry is a consistency check, not a security boundary: the
    // publish time comes from the same unauthenticated packument as the
    // signatures. A missing or unparsable publish time therefore keeps the
    // key usable — the signature check below is what gates acceptance.
    match (key.expires.as_deref().and_then(parse_timestamp), published_time) {
        (Some(expires), Some(published)) => published >= expires,
        _ => false,
    }
}

/// Verify one base64 ECDSA-P256 signature over `message` against a base64
/// SPKI public key. Any malformed key material or signature bytes count as a
/// non-match rather than an error, so one bad key can't abort the audit.
pub(super) fn verify_one(public_key_base64: &str, message: &str, signature_base64: &str) -> bool {
    let engine = base64::engine::general_purpose::STANDARD;
    let Ok(key_der) = engine.decode(public_key_base64) else { return false };
    let Ok(verifying_key) = VerifyingKey::from_public_key_der(&key_der) else { return false };
    let Ok(signature_der) = engine.decode(signature_base64) else { return false };
    let Ok(signature) = Signature::from_der(&signature_der) else { return false };
    verifying_key.verify(message.as_bytes(), &signature).is_ok()
}
