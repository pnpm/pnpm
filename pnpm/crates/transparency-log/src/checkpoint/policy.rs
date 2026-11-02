use super::{
    CheckpointError, Cosignature, VerifiedCheckpoint, note_error, parse_tree_head,
    split_cosignature,
};
use crate::keys::{KeyAlgorithm, VerifierKey, cosignature_message};
use derive_more::{Display, Error};
use signed_note::{Note, Signature, Verifier, VerifierList};
use std::collections::HashSet;

/// Keys and how many of them must sign.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeySet {
    keys: Vec<VerifierKey>,
    threshold: usize,
}

impl KeySet {
    /// A set of distinct `keys` of which `threshold` must sign.
    pub fn new(keys: Vec<VerifierKey>, threshold: usize) -> Result<Self, PolicyError> {
        if threshold > keys.len() {
            return Err(PolicyError::ThresholdExceedsKeys { threshold, keys: keys.len() });
        }
        let mut seen = HashSet::new();
        if let Some(duplicate) = keys
            .iter()
            .find(|key| !seen.insert(key.key_name()))
        {
            return Err(PolicyError::DuplicateKey { key: duplicate.key_name() });
        }
        Ok(KeySet { keys, threshold })
    }

    /// No keys and a threshold of zero, as a private log that names no
    /// witnesses.
    #[must_use]
    pub const fn empty() -> Self {
        KeySet { keys: Vec::new(), threshold: 0 }
    }

    /// The keys.
    #[must_use]
    pub fn keys(&self) -> &[VerifierKey] {
        &self.keys
    }

    /// How many distinct keys must sign.
    #[must_use]
    pub const fn threshold(&self) -> usize {
        self.threshold
    }

    fn contains(&self, signature: &Signature) -> bool {
        let signer = (signature.name(), signature.id());
        self.keys
            .iter()
            .any(|key| (key.name(), key.key_id()) == signer)
    }
}

/// Why a [`CheckpointPolicy`] could not be built.
#[derive(Debug, Display, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PolicyError {
    #[display("threshold {threshold} exceeds the {keys} keys given")]
    ThresholdExceedsKeys { threshold: usize, keys: usize },
    #[display("note key {key} is listed twice")]
    DuplicateKey {
        #[error(not(source))]
        key: String,
    },
    #[display("a log needs at least one checkpoint signature")]
    NoLogSignatures,
    #[display("log key {key} must be an Ed25519 note key named after the log")]
    InvalidLogKey {
        #[error(not(source))]
        key: String,
    },
    #[display("witness key {key} must be an Ed25519 cosignature key")]
    InvalidWitnessKey {
        #[error(not(source))]
        key: String,
    },
}

/// What a client requires of a log's checkpoints: the log's origin, its
/// checkpoint keys, and the witnesses whose cosignatures make a quorum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointPolicy {
    origin: String,
    log_keys: KeySet,
    witnesses: KeySet,
}

impl CheckpointPolicy {
    /// A policy for the log named `origin`.
    ///
    /// Every log key must be an Ed25519 note key named `origin`, and every
    /// witness key an Ed25519 cosignature key. A policy with no witnesses
    /// detects a rewrite of the history a client has seen, but not a log that
    /// shows different clients different histories.
    pub fn new(origin: &str, log_keys: KeySet, witnesses: KeySet) -> Result<Self, PolicyError> {
        if log_keys.threshold == 0 {
            return Err(PolicyError::NoLogSignatures);
        }
        let is_log_key =
            |key: &VerifierKey| key.algorithm() == KeyAlgorithm::Ed25519 && key.name() == origin;
        if let Some(key) = log_keys.keys
            .iter()
            .find(|key| !is_log_key(key))
        {
            return Err(PolicyError::InvalidLogKey { key: key.key_name() });
        }
        let is_witness_key = |key: &VerifierKey| key.algorithm() == KeyAlgorithm::CosignatureV1;
        if let Some(key) = witnesses.keys
            .iter()
            .find(|key| !is_witness_key(key))
        {
            return Err(PolicyError::InvalidWitnessKey { key: key.key_name() });
        }
        Ok(CheckpointPolicy { origin: origin.to_owned(), log_keys, witnesses })
    }

    /// The log's origin.
    #[must_use]
    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// The log's checkpoint keys.
    #[must_use]
    pub const fn log_keys(&self) -> &KeySet {
        &self.log_keys
    }

    /// The witnesses and their quorum.
    #[must_use]
    pub const fn witnesses(&self) -> &KeySet {
        &self.witnesses
    }

    /// Verifies a signed checkpoint: its log signatures, its witness quorum,
    /// and that it is a checkpoint of this log.
    ///
    /// Signatures from keys the policy does not name are ignored. A signature
    /// from a named key that does not verify rejects the checkpoint.
    pub fn verify(&self, note: &[u8]) -> Result<VerifiedCheckpoint, CheckpointError> {
        let parsed = Note::from_bytes(note).map_err(|_| CheckpointError::MalformedNote)?;
        let (verified, _) = parsed
            .verify(&self.note_verifiers())
            .map_err(|error| note_error(error, self.log_keys.threshold))?;
        self.check_log_signatures(&verified)?;
        let cosignatures = self.witness_cosignatures(&verified);
        if cosignatures.len() < self.witnesses.threshold {
            return Err(CheckpointError::WitnessQuorum {
                found: cosignatures.len(),
                required: self.witnesses.threshold,
            });
        }
        let tree_head = parse_tree_head(parsed.text())?;
        if tree_head.origin != self.origin {
            return Err(CheckpointError::WrongOrigin {
                expected: self.origin.clone(),
                actual: tree_head.origin,
            });
        }
        Ok(VerifiedCheckpoint { tree_head, cosignatures, note: note.to_vec() })
    }

    fn note_verifiers(&self) -> VerifierList {
        let keys = self.log_keys.keys.iter().chain(&self.witnesses.keys);
        let verifiers = keys
            .map(|key| Box::new(NoteKeyVerifier(key.clone())) as Box<dyn Verifier>)
            .collect();
        VerifierList::new(verifiers)
    }

    fn check_log_signatures(&self, verified: &[Signature]) -> Result<(), CheckpointError> {
        let found = verified
            .iter()
            .filter(|signature| self.log_keys.contains(signature))
            .count();
        if found < self.log_keys.threshold {
            return Err(CheckpointError::LogSignatures {
                found,
                required: self.log_keys.threshold,
            });
        }
        Ok(())
    }

    fn witness_cosignatures(&self, verified: &[Signature]) -> Vec<Cosignature> {
        verified
            .iter()
            .filter(|signature| self.witnesses.contains(signature))
            .filter_map(|signature| {
                let (timestamp, _) = split_cosignature(signature.signature())?;
                let witness = format!("{}+{:08x}", signature.name(), signature.id());
                Some(Cosignature { witness, timestamp })
            })
            .collect()
    }
}

/// Adapts a [`VerifierKey`] to the note parser, verifying a cosignature over
/// its `cosignature/v1` message rather than the bare note text.
struct NoteKeyVerifier(VerifierKey);

impl Verifier for NoteKeyVerifier {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn key_id(&self) -> u32 {
        self.0.key_id()
    }

    fn verify(&self, message: &[u8], signature: &[u8]) -> bool {
        match self.0.algorithm() {
            KeyAlgorithm::Ed25519 => self.0.verifies(message, signature),
            KeyAlgorithm::CosignatureV1 => {
                let Some((timestamp, signature)) = split_cosignature(signature) else {
                    return false;
                };
                self.0.verifies(&cosignature_message(timestamp, message), signature)
            }
        }
    }
}
