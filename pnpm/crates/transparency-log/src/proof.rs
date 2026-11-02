//! RFC 6962 inclusion and consistency proofs, and following a log from one
//! checkpoint to the next.

use crate::{checkpoint::TreeHead, integrate::read_checked};
use derive_more::{Display, Error};
use tlog_tiles::{
    EMPTY_HASH, Hash, HashReader, check_record, check_tree, prove_record, prove_tree, record_hash,
};

/// The longest proof a tree of up to 2⁶⁴ entries needs.
const MAX_PROOF_LENGTH: usize = 64;

/// Why a log operation or a proof failed.
#[derive(Debug, Display, Error)]
#[non_exhaustive]
pub enum LogError {
    #[display("transparency log: {_0}")]
    Tlog(tlog_tiles::Error),
    #[display("log entry of {size} bytes exceeds the entry size limit")]
    EntryTooLarge { size: usize },
    #[display("tile is not a C2SP hash tile")]
    InvalidTile,
    #[display("entry {index} is not in a tree of {size} entries")]
    IndexOutOfRange { index: u64, size: u64 },
    #[display("tree of {old} entries is larger than the tree of {new} entries it should precede")]
    SizeOrder { old: u64, new: u64 },
    #[display("proof of {length} hashes is longer than any tree needs")]
    ProofTooLong { length: usize },
    #[display("a consistency proof needs a non-empty older tree")]
    EmptyTree,
    #[display("invalid transparency log proof")]
    InvalidProof,
    #[display("checkpoints of different logs: {old} and {new}")]
    DifferentLogs {
        #[error(not(source))]
        old: String,
        #[error(not(source))]
        new: String,
    },
}

impl From<tlog_tiles::Error> for LogError {
    fn from(error: tlog_tiles::Error) -> Self {
        LogError::Tlog(error)
    }
}

/// The inclusion proof of entry `index` in the tree of `tree_size` entries.
pub fn prove_inclusion<Store: HashReader>(
    store: &Store,
    tree_size: u64,
    index: u64,
) -> Result<Vec<Hash>, LogError> {
    if index >= tree_size {
        return Err(LogError::IndexOutOfRange { index, size: tree_size });
    }
    Ok(prove_record(tree_size, index, &CheckedStore(store))?)
}

/// The consistency proof from the tree of `old_size` entries to the tree of
/// `new_size` entries.
pub fn prove_consistency<Store: HashReader>(
    store: &Store,
    old_size: u64,
    new_size: u64,
) -> Result<Vec<Hash>, LogError> {
    if old_size > new_size {
        return Err(LogError::SizeOrder { old: old_size, new: new_size });
    }
    if old_size == 0 {
        return Err(LogError::EmptyTree);
    }
    Ok(prove_tree(new_size, old_size, &CheckedStore(store))?)
}

/// Verifies that `entry` is entry `index` of the tree `tree_head` describes.
pub fn verify_inclusion(
    proof: &[Hash],
    tree_head: &TreeHead,
    index: u64,
    entry: &[u8],
) -> Result<(), LogError> {
    verify_inclusion_hash(proof, tree_head.size, tree_head.hash, index, record_hash(entry))
}

/// Verifies that the entry hashing to `leaf_hash` is entry `index` of the
/// tree of `tree_size` entries with hash `root`.
pub fn verify_inclusion_hash(
    proof: &[Hash],
    tree_size: u64,
    root: Hash,
    index: u64,
    leaf_hash: Hash,
) -> Result<(), LogError> {
    if index >= tree_size {
        return Err(LogError::IndexOutOfRange { index, size: tree_size });
    }
    check_proof_length(proof)?;
    check_record(&proof.to_vec(), tree_size, root, index, leaf_hash)
        .map_err(|_| LogError::InvalidProof)
}

/// Verifies that the tree `new` describes extends the tree `old` describes.
pub fn verify_consistency(proof: &[Hash], old: &TreeHead, new: &TreeHead) -> Result<(), LogError> {
    if old.origin != new.origin {
        return Err(LogError::DifferentLogs { old: old.origin.clone(), new: new.origin.clone() });
    }
    verify_consistency_hashes(proof, (old.size, old.hash), (new.size, new.hash))
}

/// Verifies that the tree of `old` `(size, root)` is a prefix of the tree of
/// `new` `(size, root)`. As in RFC 9162, a proof from the empty tree is an
/// error: every tree extends it, so there is nothing to prove.
pub fn verify_consistency_hashes(
    proof: &[Hash],
    (old_size, old_root): (u64, Hash),
    (new_size, new_root): (u64, Hash),
) -> Result<(), LogError> {
    if old_size > new_size {
        return Err(LogError::SizeOrder { old: old_size, new: new_size });
    }
    check_proof_length(proof)?;
    if old_size == 0 {
        return Err(LogError::EmptyTree);
    }
    if old_size == new_size {
        let is_same_tree = proof.is_empty() && old_root == new_root;
        return if is_same_tree { Ok(()) } else { Err(LogError::InvalidProof) };
    }
    check_tree(&proof.to_vec(), new_size, new_root, old_size, old_root)
        .map_err(|_| LogError::InvalidProof)
}

/// Checks a newly verified tree head against the one a client kept, and
/// returns the one to keep: the larger of the two, once `proof` shows that
/// the smaller is a prefix of it.
///
/// A client that keeps the result can detect any rewrite of the history it
/// has seen. Only witnesses detect a log that shows different clients
/// different histories.
pub fn advance(
    kept: Option<TreeHead>,
    candidate: TreeHead,
    proof: &[Hash],
) -> Result<TreeHead, LogError> {
    let Some(kept) = kept else { return Ok(candidate) };
    let (old, new) = if candidate.size < kept.size { (candidate, kept) } else { (kept, candidate) };
    if old.size > 0 {
        verify_consistency(proof, &old, &new)?;
    } else if old.origin != new.origin {
        return Err(LogError::DifferentLogs { old: old.origin, new: new.origin });
    } else if old.hash != EMPTY_HASH {
        return Err(LogError::InvalidProof);
    }
    Ok(new)
}

fn check_proof_length(proof: &[Hash]) -> Result<(), LogError> {
    if proof.len() > MAX_PROOF_LENGTH {
        return Err(LogError::ProofTooLong { length: proof.len() });
    }
    Ok(())
}

/// A store whose short reads are errors rather than panics.
struct CheckedStore<'a, Store>(&'a Store);

impl<Store: HashReader> HashReader for CheckedStore<'_, Store> {
    fn read_hashes(&self, indexes: &[u64]) -> Result<Vec<Hash>, tlog_tiles::Error> {
        read_checked(self.0, indexes)
    }
}

#[cfg(test)]
mod tests;
