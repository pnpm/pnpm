//! [DSSE](https://github.com/secure-systems-lab/dsse) envelopes signed with
//! Ed25519 note keys.
//!
//! A statement key is written as a note verifier key,
//! `<name>+<key hash>+<key>`, and its `<name>+<key hash>` is the envelope
//! signature's `keyid`.

use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD},
};
use derive_more::{Display, Error};
use pnpm_transparency_log::{KeyAlgorithm, KeySet, NoteSigner, VerifierKey};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// The payload type of an in-toto Statement.
pub const IN_TOTO_PAYLOAD_TYPE: &str = "application/vnd.in-toto+json";

/// A DSSE envelope in its JSON form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Envelope {
    /// The base64 of the signed payload.
    pub payload: String,
    /// What the payload is, such as [`IN_TOTO_PAYLOAD_TYPE`].
    pub payload_type: String,
    /// The signatures over the payload's pre-authentication encoding.
    pub signatures: Vec<EnvelopeSignature>,
}

/// One signature of an [`Envelope`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvelopeSignature {
    /// The `<name>+<key hash>` of the signing key, or empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub keyid: String,
    /// The base64 of the signature.
    pub sig: String,
}

/// The payload of an envelope whose signatures met a key threshold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedPayload {
    /// The authenticated payload type.
    pub payload_type: String,
    /// The authenticated payload bytes. Use these, never the envelope's
    /// `payload` decoded again.
    pub body: Vec<u8>,
}

/// Why an envelope did not verify.
#[derive(Debug, Display, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DsseError {
    #[display("envelope payload or signature is not base64")]
    Encoding,
    #[display("envelope has {found} valid signatures from distinct keys, {required} required")]
    Threshold { found: usize, required: usize },
    #[display("statement key {key} is not an Ed25519 note key")]
    InvalidKey {
        #[error(not(source))]
        key: String,
    },
}

/// The DSSE pre-authentication encoding of `body` under `payload_type`:
/// `DSSEv1 <len(type)> <type> <len(body)> <body>`.
#[must_use]
pub fn pae(payload_type: &str, body: &[u8]) -> Vec<u8> {
    let mut encoded =
        format!("DSSEv1 {} {payload_type} {} ", payload_type.len(), body.len()).into_bytes();
    encoded.extend_from_slice(body);
    encoded
}

impl Envelope {
    /// Signs `body` under `payload_type` with each of `signers`.
    #[must_use]
    pub fn sign(payload_type: &str, body: &[u8], signers: &[&NoteSigner]) -> Self {
        let message = pae(payload_type, body);
        let signatures = signers
            .iter()
            .map(|signer| EnvelopeSignature {
                keyid: signer.verifier_key().key_name(),
                sig: STANDARD.encode(signer.sign_bytes(&message)),
            })
            .collect();
        Envelope {
            payload: STANDARD.encode(body),
            payload_type: payload_type.to_owned(),
            signatures,
        }
    }

    /// Verifies the envelope against `keys`: at least the set's threshold of
    /// distinct keys must have signed it. Signatures from other keys, and
    /// signatures that do not verify, are ignored.
    pub fn verify(&self, keys: &KeySet) -> Result<VerifiedPayload, DsseError> {
        if let Some(key) = keys
            .keys()
            .iter()
            .find(|key| key.algorithm() != KeyAlgorithm::Ed25519)
        {
            return Err(DsseError::InvalidKey { key: key.key_name() });
        }
        let body = decode(&self.payload)?;
        let message = pae(&self.payload_type, &body);
        let mut accepted = HashSet::new();
        for signature in &self.signatures {
            let sig = decode(&signature.sig)?;
            let signer = keys
                .keys()
                .iter()
                .find(|key| signs(key, signature, &message, &sig));
            if let Some(key) = signer {
                accepted.insert(key.key_name());
            }
        }
        if accepted.len() < keys.threshold() || keys.threshold() == 0 {
            return Err(DsseError::Threshold { found: accepted.len(), required: keys.threshold() });
        }
        Ok(VerifiedPayload { payload_type: self.payload_type.clone(), body })
    }
}

fn signs(key: &VerifierKey, signature: &EnvelopeSignature, message: &[u8], sig: &[u8]) -> bool {
    let is_named = signature.keyid.is_empty() || signature.keyid == key.key_name();
    is_named && key.verifies(message, sig)
}

/// Decodes standard or URL-safe base64, padded or not, as DSSE requires
/// verifiers to accept.
fn decode(encoded: &str) -> Result<Vec<u8>, DsseError> {
    [STANDARD, URL_SAFE, STANDARD_NO_PAD, URL_SAFE_NO_PAD]
        .iter()
        .find_map(|engine| engine.decode(encoded).ok())
        .ok_or(DsseError::Encoding)
}

#[cfg(test)]
mod tests;
