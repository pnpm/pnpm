use super::{KeyAlgorithm, KeyError, VerifierKey};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{SignatureError, Signer as _, SigningKey};
use std::fmt;

/// Signs notes, such as a log's checkpoints, with an Ed25519 key.
pub struct NoteSigner {
    key: VerifierKey,
    signing_key: SigningKey,
}

impl NoteSigner {
    /// A signer named `name`, which for a log is its origin.
    pub fn new(name: &str, signing_key: SigningKey) -> Result<Self, KeyError> {
        let key = VerifierKey::new(name, KeyAlgorithm::Ed25519, signing_key.verifying_key())?;
        Ok(NoteSigner { key, signing_key })
    }

    /// Reads a private key written `PRIVATE+KEY+<name>+<key hash>+<key>`, as
    /// Go's `note.GenerateKey` writes one.
    pub fn from_private_key(text: &str) -> Result<Self, KeyError> {
        let rest = text.strip_prefix("PRIVATE+KEY+").ok_or(KeyError::Malformed)?;
        let mut parts = rest.splitn(3, '+');
        let (Some(name), Some(key_hash), Some(encoded)) =
            (parts.next(), parts.next(), parts.next())
        else {
            return Err(KeyError::Malformed);
        };
        let decoded = STANDARD.decode(encoded).map_err(|_| KeyError::Malformed)?;
        let signing_key = decode_seed(&decoded)?;
        let signer = NoteSigner::new(name, signing_key)?;
        if format!("{:08x}", signer.key.key_id()) != key_hash {
            return Err(KeyError::KeyHashMismatch);
        }
        Ok(signer)
    }

    /// The key that verifies this signer's signatures.
    #[must_use]
    pub const fn verifier_key(&self) -> &VerifierKey {
        &self.key
    }

    /// The Ed25519 signature of `message`, for signing formats other than
    /// notes, such as DSSE envelopes.
    #[must_use]
    pub fn sign_bytes(&self, message: &[u8]) -> [u8; 64] {
        self.signing_key.sign(message).to_bytes()
    }
}

impl fmt::Debug for NoteSigner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NoteSigner")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

impl signed_note::Signer for NoteSigner {
    fn name(&self) -> &str {
        self.key.name()
    }

    fn key_id(&self) -> u32 {
        self.key.key_id()
    }

    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, SignatureError> {
        Ok(self.sign_bytes(message).to_vec())
    }
}

/// Produces Ed25519 checkpoint cosignatures (`cosignature/v1`), as a witness
/// does after checking that a log only grew.
pub struct Cosigner {
    key: VerifierKey,
    signing_key: SigningKey,
}

impl Cosigner {
    /// A cosigner named `name`, such as `witness.example/ci`.
    pub fn new(name: &str, signing_key: SigningKey) -> Result<Self, KeyError> {
        let public_key = signing_key.verifying_key();
        let key = VerifierKey::new(name, KeyAlgorithm::CosignatureV1, public_key)?;
        Ok(Cosigner { key, signing_key })
    }

    /// The key that verifies this cosigner's cosignatures.
    #[must_use]
    pub const fn verifier_key(&self) -> &VerifierKey {
        &self.key
    }

    /// A note signer that cosigns as of `timestamp`, in seconds since the
    /// Unix epoch.
    #[must_use]
    pub const fn at(&self, timestamp: u64) -> TimedCosigner<'_> {
        TimedCosigner { cosigner: self, timestamp }
    }
}

impl fmt::Debug for Cosigner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Cosigner")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

/// A [`Cosigner`] bound to the timestamp its cosignatures carry.
#[derive(Debug)]
pub struct TimedCosigner<'a> {
    cosigner: &'a Cosigner,
    timestamp: u64,
}

impl signed_note::Signer for TimedCosigner<'_> {
    fn name(&self) -> &str {
        self.cosigner.key.name()
    }

    fn key_id(&self) -> u32 {
        self.cosigner.key.key_id()
    }

    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, SignatureError> {
        let signed = cosignature_message(self.timestamp, message);
        let signature = self.cosigner.signing_key.sign(&signed);
        let mut encoded = self.timestamp.to_be_bytes().to_vec();
        encoded.extend_from_slice(&signature.to_bytes());
        Ok(encoded)
    }
}

/// The message an Ed25519 checkpoint cosignature signs: the
/// `cosignature/v1` and `time <timestamp>` lines followed by the checkpoint
/// text, extension lines included.
#[must_use]
pub fn cosignature_message(timestamp: u64, checkpoint_text: &[u8]) -> Vec<u8> {
    let mut message = format!("cosignature/v1\ntime {timestamp}\n").into_bytes();
    message.extend_from_slice(checkpoint_text);
    message
}

fn decode_seed(decoded: &[u8]) -> Result<SigningKey, KeyError> {
    let (&signature_type, seed) = decoded.split_first().ok_or(KeyError::Malformed)?;
    if signature_type != KeyAlgorithm::Ed25519.signature_type() {
        return Err(KeyError::UnsupportedType { signature_type });
    }
    let seed: &[u8; 32] = seed.try_into().map_err(|_| KeyError::InvalidPublicKey)?;
    Ok(SigningKey::from_bytes(seed))
}
