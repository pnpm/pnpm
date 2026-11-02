//! The inclusion and consistency probes are the JSON test vectors of
//! `transparency-dev/merkle` (`testdata/inclusion` and
//! `testdata/consistency`, Apache-2.0), at commit
//! `c39fbf350a405f68571bf4c42a6a518eb0a46a4b`, each tagged with its file.

use super::{
    LogError, advance, prove_consistency, prove_inclusion, verify_consistency,
    verify_consistency_hashes, verify_inclusion, verify_inclusion_hash,
};
use crate::{checkpoint::TreeHead, integrate::integrate};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use pretty_assertions::assert_eq;
use serde::Deserialize;
use std::collections::BTreeMap;
use tlog_tiles::{Hash, HashReader};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InclusionProbe {
    leaf_idx: u64,
    tree_size: u64,
    root: Option<String>,
    leaf_hash: Option<String>,
    proof: Option<Vec<String>>,
    want_err: bool,
    file: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConsistencyProbe {
    size1: u64,
    size2: u64,
    root1: Option<String>,
    root2: Option<String>,
    proof: Option<Vec<String>>,
    want_err: bool,
    file: String,
}

/// Decodes a probe hash; `None` for one that is not 32 bytes, which no valid
/// proof contains.
fn hash(encoded: Option<&String>) -> Option<Hash> {
    let bytes = STANDARD
        .decode(encoded.map_or("", String::as_str))
        .ok()?;
    Some(Hash(bytes.try_into().ok()?))
}

fn hashes(encoded: Option<&Vec<String>>) -> Option<Vec<Hash>> {
    encoded.map_or(Some(Vec::new()), |list| {
        list.iter()
            .map(|item| hash(Some(item)))
            .collect()
    })
}

#[test]
fn agrees_with_transparency_dev_inclusion_probes() {
    let probes: Vec<InclusionProbe> =
        serde_json::from_str(include_str!("../../tests/fixtures/inclusion-probes.json")).unwrap();
    assert_eq!(probes.len(), 98);
    for probe in probes {
        let decoded = (
            hash(probe.root.as_ref()),
            hash(probe.leaf_hash.as_ref()),
            hashes(probe.proof.as_ref()),
        );
        let (Some(root), Some(leaf_hash), Some(proof)) = decoded else {
            assert!(probe.want_err, "{} has malformed hashes but expects success", probe.file);
            continue;
        };
        let result =
            verify_inclusion_hash(&proof, probe.tree_size, root, probe.leaf_idx, leaf_hash);
        assert_eq!(result.is_err(), probe.want_err, "{}: {result:?}", probe.file);
    }
}

#[test]
fn agrees_with_transparency_dev_consistency_probes() {
    let probes: Vec<ConsistencyProbe> =
        serde_json::from_str(include_str!("../../tests/fixtures/consistency-probes.json")).unwrap();
    assert_eq!(probes.len(), 98);
    for probe in probes {
        let decoded =
            (hash(probe.root1.as_ref()), hash(probe.root2.as_ref()), hashes(probe.proof.as_ref()));
        let (Some(root1), Some(root2), Some(proof)) = decoded else {
            // A few probes compare opaque roots that are not 32-byte hashes,
            // which a `Hash` cannot hold: equal ones of equal sizes pass.
            let is_opaque_match = probe.size1 == probe.size2 && probe.root1 == probe.root2;
            assert!(probe.want_err || is_opaque_match, "{} has malformed hashes", probe.file);
            continue;
        };
        let result = verify_consistency_hashes(&proof, (probe.size1, root1), (probe.size2, root2));
        assert_eq!(result.is_err(), probe.want_err, "{}: {result:?}", probe.file);
    }
}

#[derive(Default)]
struct MemoryStore(BTreeMap<u64, Hash>);

impl HashReader for MemoryStore {
    fn read_hashes(&self, indexes: &[u64]) -> Result<Vec<Hash>, tlog_tiles::Error> {
        indexes
            .iter()
            .map(|index| {
                self.0
                    .get(index)
                    .copied()
                    .ok_or(tlog_tiles::Error::IndexesNotInTree)
            })
            .collect()
    }
}

fn entry(index: u64) -> Vec<u8> {
    format!("pnpr-log-entry {index}").into_bytes()
}

/// A log of 300 entries, and the tree heads after each of its batches.
fn log() -> (MemoryStore, Vec<TreeHead>) {
    let mut store = MemoryStore::default();
    let mut heads = Vec::new();
    let mut size = 0;
    for batch in [1, 6, 57, 192, 44] {
        let entries: Vec<Vec<u8>> = (size..size + batch).map(entry).collect();
        let integration = integrate(&store, size, &entries).unwrap();
        store.0.extend(
            integration.stored_hashes
                .iter()
                .map(|stored| (stored.index, stored.hash)),
        );
        size = integration.size;
        heads.push(TreeHead { origin: "log.example".to_owned(), size, hash: integration.root });
    }
    (store, heads)
}

#[test]
fn proves_inclusion_of_every_entry() {
    let (store, heads) = log();
    for head in &heads {
        for index in 0..head.size {
            let proof = prove_inclusion(&store, head.size, index).unwrap();
            verify_inclusion(&proof, head, index, &entry(index)).unwrap();
            let wrong = verify_inclusion(&proof, head, index, b"forged");
            assert!(matches!(wrong, Err(LogError::InvalidProof)));
        }
    }
    let last = heads.last().unwrap();
    assert!(matches!(
        prove_inclusion(&store, last.size, last.size),
        Err(LogError::IndexOutOfRange { index: 300, size: 300 }),
    ));
}

#[test]
fn proves_consistency_between_every_pair_of_tree_heads() {
    let (store, heads) = log();
    assert!(matches!(prove_consistency(&store, 0, 1), Err(LogError::EmptyTree)));
    for (position, old) in heads.iter().enumerate() {
        for new in &heads[position..] {
            let proof = prove_consistency(&store, old.size, new.size).unwrap();
            verify_consistency(&proof, old, new).unwrap();
        }
    }
    let forked = TreeHead { hash: Hash([7; 32]), ..heads[4].clone() };
    let proof = prove_consistency(&store, heads[1].size, heads[4].size).unwrap();
    assert!(matches!(verify_consistency(&proof, &heads[1], &forked), Err(LogError::InvalidProof)));
}

#[test]
fn advance_keeps_the_larger_consistent_tree_head() {
    let (store, heads) = log();
    let (small, large) = (heads[1].clone(), heads[3].clone());
    let proof = prove_consistency(&store, small.size, large.size).unwrap();
    assert_eq!(advance(None, small.clone(), &[]).unwrap(), small);
    assert_eq!(advance(Some(small.clone()), large.clone(), &proof).unwrap(), large);
    assert_eq!(advance(Some(large.clone()), small.clone(), &proof).unwrap(), large);

    let rewritten = TreeHead { hash: Hash([1; 32]), ..large.clone() };
    assert!(advance(Some(large.clone()), rewritten, &[]).is_err());
    let other_log = TreeHead { origin: "other.example".to_owned(), ..large };
    assert!(matches!(advance(Some(small), other_log, &proof), Err(LogError::DifferentLogs { .. })));

    let empty = TreeHead { size: 0, hash: tlog_tiles::EMPTY_HASH, ..large.clone() };
    assert_eq!(advance(Some(empty.clone()), large.clone(), &[]).unwrap(), large);
    assert_eq!(advance(Some(large.clone()), empty.clone(), &[]).unwrap(), large);
    let not_empty = TreeHead { hash: Hash([2; 32]), ..empty };
    assert!(matches!(advance(Some(large), not_empty, &[]), Err(LogError::InvalidProof)));
}

#[test]
fn rejects_proofs_longer_than_any_tree_needs() {
    let proof = vec![Hash::default(); 65];
    let result = verify_inclusion_hash(&proof, u64::MAX, Hash::default(), 0, Hash::default());
    assert!(matches!(result, Err(LogError::ProofTooLong { length: 65 })));
}
