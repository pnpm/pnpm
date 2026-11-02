//! Signed-note keys: verifier keys in the `<name>+<key hash>+<key>` form, and
//! the signers for log signatures and witness cosignatures.

pub use signer::*;

mod signer;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use derive_more::{Display, Error};
use ed25519_dalek::{Signature, VerifyingKey};
use std::{fmt, str::FromStr};

/// The signature type byte of an Ed25519 note signature.
pub const ED25519_SIGNATURE_TYPE: u8 = 0x01;

/// The signature type byte of an Ed25519 checkpoint cosignature
/// (`cosignature/v1`).
pub const COSIGNATURE_V1_SIGNATURE_TYPE: u8 = 0x04;

/// What a note key signs, which is the signature type byte its encoded key
/// starts with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyAlgorithm {
    /// An Ed25519 signature over the note text, as a log signs its checkpoints.
    Ed25519,
    /// An Ed25519 checkpoint cosignature, with a timestamp, as a witness
    /// signs a checkpoint.
    CosignatureV1,
}

impl KeyAlgorithm {
    /// The signature type byte that starts an encoded key of this algorithm.
    #[must_use]
    pub const fn signature_type(self) -> u8 {
        match self {
            KeyAlgorithm::Ed25519 => ED25519_SIGNATURE_TYPE,
            KeyAlgorithm::CosignatureV1 => COSIGNATURE_V1_SIGNATURE_TYPE,
        }
    }

    fn from_signature_type(signature_type: u8) -> Option<Self> {
        match signature_type {
            ED25519_SIGNATURE_TYPE => Some(KeyAlgorithm::Ed25519),
            COSIGNATURE_V1_SIGNATURE_TYPE => Some(KeyAlgorithm::CosignatureV1),
            _ => None,
        }
    }
}

/// Why a verifier key string was rejected.
#[derive(Debug, Display, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyError {
    #[display("malformed note key: expected <name>+<key hash>+<key>")]
    Malformed,
    #[display("invalid note key name")]
    InvalidName,
    #[display("unsupported note key signature type {signature_type:#04x}")]
    UnsupportedType { signature_type: u8 },
    #[display("invalid Ed25519 public key")]
    InvalidPublicKey,
    #[display("note key hash does not match its name and key")]
    KeyHashMismatch,
}

/// A note verifier key, written `<name>+<key hash>+<key>` as Go's
/// `golang.org/x/mod/sumdb/note` writes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifierKey {
    name: String,
    key_id: u32,
    algorithm: KeyAlgorithm,
    public_key: VerifyingKey,
}

impl VerifierKey {
    /// Builds the key a signer of `algorithm` named `name` verifies with.
    pub fn new(
        name: &str,
        algorithm: KeyAlgorithm,
        public_key: VerifyingKey,
    ) -> Result<Self, KeyError> {
        if !signed_note::is_key_name_valid(name) {
            return Err(KeyError::InvalidName);
        }
        let key_id = signed_note::key_id(name, &encode_key(algorithm, &public_key));
        Ok(VerifierKey { name: name.to_owned(), key_id, algorithm, public_key })
    }

    /// The key name, such as a log origin or a witness name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The key hash: the first four bytes of the SHA-256 of the name, a
    /// newline, and the encoded key.
    #[must_use]
    pub const fn key_id(&self) -> u32 {
        self.key_id
    }

    /// What this key verifies.
    #[must_use]
    pub const fn algorithm(&self) -> KeyAlgorithm {
        self.algorithm
    }

    /// The raw Ed25519 public key.
    #[must_use]
    pub const fn public_key(&self) -> &VerifyingKey {
        &self.public_key
    }

    /// Whether `signature` is this key's signature of `message`, by the
    /// strict Ed25519 verification rules.
    #[must_use]
    pub fn verifies(&self, message: &[u8], signature: &[u8]) -> bool {
        let Ok(signature) = Signature::from_slice(signature) else { return false };
        self.public_key.verify_strict(message, &signature).is_ok()
    }

    /// The `<name>+<key hash>` prefix, which names the key without its
    /// material, as in a DSSE `keyid`.
    #[must_use]
    pub fn key_name(&self) -> String {
        format!("{}+{:08x}", self.name, self.key_id)
    }
}

impl fmt::Display for VerifierKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let encoded = STANDARD.encode(encode_key(self.algorithm, &self.public_key));
        write!(formatter, "{}+{encoded}", self.key_name())
    }
}

impl FromStr for VerifierKey {
    type Err = KeyError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut parts = text.splitn(3, '+');
        let (Some(name), Some(key_hash), Some(encoded)) =
            (parts.next(), parts.next(), parts.next())
        else {
            return Err(KeyError::Malformed);
        };
        let key_id = parse_key_hash(key_hash)?;
        let decoded = STANDARD.decode(encoded).map_err(|_| KeyError::Malformed)?;
        let (algorithm, public_key) = decode_key(&decoded)?;
        let parsed = VerifierKey::new(name, algorithm, public_key)?;
        if parsed.key_id != key_id {
            return Err(KeyError::KeyHashMismatch);
        }
        Ok(parsed)
    }
}

fn parse_key_hash(key_hash: &str) -> Result<u32, KeyError> {
    if key_hash.len() != 8 || !key_hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(KeyError::Malformed);
    }
    u32::from_str_radix(key_hash, 16).map_err(|_| KeyError::Malformed)
}

fn decode_key(decoded: &[u8]) -> Result<(KeyAlgorithm, VerifyingKey), KeyError> {
    let (&signature_type, material) = decoded.split_first().ok_or(KeyError::Malformed)?;
    let algorithm = KeyAlgorithm::from_signature_type(signature_type)
        .ok_or(KeyError::UnsupportedType { signature_type })?;
    let bytes: &[u8; 32] = material.try_into().map_err(|_| KeyError::InvalidPublicKey)?;
    let public_key = VerifyingKey::from_bytes(bytes).map_err(|_| KeyError::InvalidPublicKey)?;
    Ok((algorithm, public_key))
}

fn encode_key(algorithm: KeyAlgorithm, public_key: &VerifyingKey) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(33);
    encoded.push(algorithm.signature_type());
    encoded.extend_from_slice(public_key.as_bytes());
    encoded
}

#[cfg(test)]
mod tests;
