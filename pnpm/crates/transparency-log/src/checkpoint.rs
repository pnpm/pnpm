//! Signing checkpoints and verifying them against a log's keys and the
//! witnesses its clients require.

pub use policy::*;

mod policy;

use derive_more::{Display, Error};
use signed_note::{Note, NoteError, Signer};
use tlog_tiles::{Checkpoint, Hash};

/// A log's tree head: the content of a checkpoint without its signatures.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeHead {
    /// The log's origin, its name in checkpoints.
    pub origin: String,
    /// The number of entries in the tree.
    pub size: u64,
    /// The RFC 6962 Merkle tree hash.
    pub hash: Hash,
}

impl TreeHead {
    /// The checkpoint text for this tree head, without extension lines.
    pub fn to_checkpoint(&self) -> Result<Checkpoint, CheckpointError> {
        Checkpoint::new(&self.origin, self.size, self.hash, "")
            .map_err(|_| CheckpointError::MalformedCheckpoint)
    }

    fn from_checkpoint(checkpoint: &Checkpoint) -> Self {
        TreeHead {
            origin: checkpoint.origin().to_owned(),
            size: checkpoint.size(),
            hash: *checkpoint.hash(),
        }
    }
}

/// A witness cosignature that verified against a required witness key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cosignature {
    /// The witness key, as `<name>+<key hash>`.
    pub witness: String,
    /// When the witness cosigned, in seconds since the Unix epoch.
    pub timestamp: u64,
}

/// A checkpoint whose log signatures and witness cosignatures met a
/// [`CheckpointPolicy`].
#[derive(Debug, Clone, PartialEq)]
pub struct VerifiedCheckpoint {
    /// The verified tree head.
    pub tree_head: TreeHead,
    /// The cosignatures of the required witnesses.
    pub cosignatures: Vec<Cosignature>,
    /// The signed note as it was received.
    pub note: Vec<u8>,
}

/// Why a checkpoint was rejected.
#[derive(Debug, Display, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CheckpointError {
    #[display("malformed signed note")]
    MalformedNote,
    #[display("malformed checkpoint")]
    MalformedCheckpoint,
    #[display("invalid signature from note key {key}")]
    InvalidSignature {
        #[error(not(source))]
        key: String,
    },
    #[display("ambiguous note key {key}")]
    AmbiguousKey {
        #[error(not(source))]
        key: String,
    },
    #[display("checkpoint is for log {actual}, expected {expected}")]
    WrongOrigin {
        #[error(not(source))]
        expected: String,
        #[error(not(source))]
        actual: String,
    },
    #[display("checkpoint has {found} valid log signatures, {required} required")]
    LogSignatures { found: usize, required: usize },
    #[display("checkpoint has {found} valid witness cosignatures, {required} required")]
    WitnessQuorum { found: usize, required: usize },
}

/// Signs `tree_head` as a checkpoint note with each of `signers`.
pub fn sign_checkpoint(
    tree_head: &TreeHead,
    signers: &[&dyn Signer],
) -> Result<Vec<u8>, CheckpointError> {
    let text = tree_head.to_checkpoint()?.to_bytes();
    let mut note = Note::new(&text, &[]).map_err(|_| CheckpointError::MalformedCheckpoint)?;
    note.add_sigs(signers).map_err(|_| CheckpointError::MalformedNote)?;
    Ok(note.to_bytes())
}

/// Adds the signatures of `signers`, such as witness cosigners, to a signed
/// checkpoint, keeping its existing signatures.
pub fn add_signatures(note: &[u8], signers: &[&dyn Signer]) -> Result<Vec<u8>, CheckpointError> {
    let mut parsed = Note::from_bytes(note).map_err(|_| CheckpointError::MalformedNote)?;
    parsed.add_sigs(signers).map_err(|_| CheckpointError::MalformedNote)?;
    Ok(parsed.to_bytes())
}

pub(crate) fn parse_tree_head(text: &[u8]) -> Result<TreeHead, CheckpointError> {
    let checkpoint =
        Checkpoint::from_bytes(text).map_err(|_| CheckpointError::MalformedCheckpoint)?;
    Ok(TreeHead::from_checkpoint(&checkpoint))
}

pub(crate) fn note_error(error: NoteError, log_threshold: usize) -> CheckpointError {
    match error {
        NoteError::InvalidSignature { name, id } => {
            CheckpointError::InvalidSignature { key: format!("{name}+{id:08x}") }
        }
        NoteError::UnverifiedNote => {
            CheckpointError::LogSignatures { found: 0, required: log_threshold }
        }
        NoteError::VerificationError(signed_note::VerificationError::AmbiguousKey { name, id }) => {
            CheckpointError::AmbiguousKey { key: format!("{name}+{id:08x}") }
        }
        _ => CheckpointError::MalformedNote,
    }
}

/// Splits an Ed25519 cosignature into its timestamp and its signature of
/// [`crate::keys::cosignature_message`].
pub(crate) fn split_cosignature(signature: &[u8]) -> Option<(u64, &[u8])> {
    let (timestamp, signature) = signature.split_first_chunk::<8>()?;
    Some((u64::from_be_bytes(*timestamp), signature))
}

#[cfg(test)]
mod tests;
