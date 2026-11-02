use super::{
    CheckpointError, CheckpointPolicy, Cosignature, KeySet, PolicyError, TreeHead, add_signatures,
    sign_checkpoint,
};
use crate::keys::{Cosigner, KeyAlgorithm, NoteSigner, VerifierKey};
use ed25519_dalek::SigningKey;
use pretty_assertions::assert_eq;
use signed_note::Signer;
use tlog_tiles::record_hash;

/// A checkpoint of the Sigstore staging Rekor v2 log, fetched from
/// `https://log2025-alpha3.rekor.sigstage.dev/checkpoint`, with Ed25519
/// cosignatures from four public witnesses.
const WITNESSED_CHECKPOINT: &[u8] = include_bytes!("../../tests/fixtures/witnessed.checkpoint");
const REKOR_ORIGIN: &str = "log2025-alpha3.rekor.sigstage.dev";
/// The log key from the Sigstore staging trusted root, written as a note key.
const REKOR_KEY: &str =
    "log2025-alpha3.rekor.sigstage.dev+d3d3a70c+AZQ93VXPMmj9uZj7U/7CefQ9yy8RgYFv6mKzOseipjqp";
/// Witness keys as published in the default policy of `FiloSottile/torchwood`.
const STAGEMOLE_KEY: &str =
    "witness.stagemole.eu+67f7aea0+BEqSG3yu9YrmcM3BHvQYTxwFj3uSWakQepafafpUqklv";
const NAVIGLI_KEY: &str =
    "witness.navigli.sunlight.geomys.org+a3e00fe2+BNy/co4C1Hn1p+INwJrfUlgz7W55dSZReusH/GhUhJ/G";

fn keys(encoded: &[&str]) -> Vec<VerifierKey> {
    encoded
        .iter()
        .map(|key| key.parse().unwrap())
        .collect()
}

fn rekor_policy(witnesses: &[&str], quorum: usize) -> CheckpointPolicy {
    let log_keys = KeySet::new(keys(&[REKOR_KEY]), 1).unwrap();
    let witnesses = KeySet::new(keys(witnesses), quorum).unwrap();
    CheckpointPolicy::new(REKOR_ORIGIN, log_keys, witnesses).unwrap()
}

#[test]
fn verifies_a_real_witnessed_checkpoint() {
    let policy = rekor_policy(&[STAGEMOLE_KEY, NAVIGLI_KEY], 2);
    let verified = policy.verify(WITNESSED_CHECKPOINT).unwrap();
    assert_eq!(verified.tree_head.origin, REKOR_ORIGIN);
    assert_eq!(verified.tree_head.size, 6_325_489);
    assert_eq!(verified.tree_head.hash.to_string(), "3gueL9uEWJsNAan6mMUhwALCwED9YoZN3RIgJt8HLG0=");
    let mut witnesses: Vec<_> = verified.cosignatures
        .iter()
        .map(|cosignature| {
            assert_eq!(cosignature.timestamp, 1_793_580_922);
            cosignature.witness.as_str()
        })
        .collect();
    witnesses.sort_unstable();
    assert_eq!(
        witnesses,
        ["witness.navigli.sunlight.geomys.org+a3e00fe2", "witness.stagemole.eu+67f7aea0"],
    );
}

#[test]
fn rejects_a_checkpoint_short_of_its_witness_quorum() {
    let absent = Cosigner::new("witness.example/absent", SigningKey::from_bytes(&[9; 32])).unwrap();
    let absent = absent.verifier_key().to_string();
    let policy = rekor_policy(&[STAGEMOLE_KEY, NAVIGLI_KEY, &absent], 3);
    assert_eq!(
        policy.verify(WITNESSED_CHECKPOINT),
        Err(CheckpointError::WitnessQuorum { found: 2, required: 3 }),
    );
}

#[test]
fn rejects_a_tampered_witness_cosignature() {
    let text = String::from_utf8(WITNESSED_CHECKPOINT.to_vec()).unwrap();
    let line_start =
        text.find("— witness.stagemole.eu ").unwrap() + "— witness.stagemole.eu ".len();
    let mut tampered = text.into_bytes();
    // The first six base64 characters encode the key hash; change one inside
    // the signature itself.
    let target = line_start + 40;
    tampered[target] = if tampered[target] == b'A' { b'B' } else { b'A' };
    let policy = rekor_policy(&[STAGEMOLE_KEY, NAVIGLI_KEY], 2);
    assert_eq!(
        policy.verify(&tampered),
        Err(CheckpointError::InvalidSignature { key: "witness.stagemole.eu+67f7aea0".to_owned() }),
    );
}

#[test]
fn rejects_a_checkpoint_without_the_log_signature() {
    let log_keys = KeySet::new(keys(&[REKOR_KEY]), 1).unwrap();
    let other_log = NoteSigner::new("other.example/log", SigningKey::from_bytes(&[5; 32])).unwrap();
    let other_keys = KeySet::new(vec![other_log.verifier_key().clone()], 1).unwrap();
    let policy = CheckpointPolicy::new("other.example/log", other_keys, KeySet::empty()).unwrap();
    assert_eq!(
        policy.verify(WITNESSED_CHECKPOINT),
        Err(CheckpointError::LogSignatures { found: 0, required: 1 }),
    );
    assert!(CheckpointPolicy::new("other.example/log", log_keys, KeySet::empty()).is_err());
}

fn tree_head(origin: &str, size: u64) -> TreeHead {
    TreeHead { origin: origin.to_owned(), size, hash: record_hash(&size.to_be_bytes()) }
}

#[test]
fn round_trips_a_signed_and_cosigned_checkpoint() {
    let origin = "registry.example/npm";
    let log = NoteSigner::new(origin, SigningKey::from_bytes(&[1; 32])).unwrap();
    let ci = Cosigner::new("witness.example/ci", SigningKey::from_bytes(&[2; 32])).unwrap();
    let mirror = Cosigner::new("witness.example/mirror", SigningKey::from_bytes(&[3; 32])).unwrap();

    let head = tree_head(origin, 42);
    let note = sign_checkpoint(&head, &[&log]).unwrap();
    let note = add_signatures(&note, &[&ci.at(1_700_000_000), &mirror.at(1_700_000_005)]).unwrap();

    let log_keys = KeySet::new(vec![log.verifier_key().clone()], 1).unwrap();
    let witness_keys = vec![ci.verifier_key().clone(), mirror.verifier_key().clone()];
    let witnesses = KeySet::new(witness_keys, 2).unwrap();
    let policy = CheckpointPolicy::new(origin, log_keys, witnesses).unwrap();
    let verified = policy.verify(&note).unwrap();
    assert_eq!(verified.tree_head, head);
    assert_eq!(verified.note, note);
    assert_eq!(
        verified.cosignatures,
        [
            Cosignature { witness: ci.verifier_key().key_name(), timestamp: 1_700_000_000 },
            Cosignature { witness: mirror.verifier_key().key_name(), timestamp: 1_700_000_005 },
        ],
    );
}

#[test]
fn a_cosignature_does_not_count_as_a_log_signature() {
    let origin = "registry.example/npm";
    let log = NoteSigner::new(origin, SigningKey::from_bytes(&[1; 32])).unwrap();
    let impostor = Cosigner::new(origin, SigningKey::from_bytes(&[1; 32])).unwrap();
    let note = sign_checkpoint(&tree_head(origin, 3), &[&impostor.at(1)]).unwrap();
    let log_keys = KeySet::new(vec![log.verifier_key().clone()], 1).unwrap();
    let policy = CheckpointPolicy::new(origin, log_keys, KeySet::empty()).unwrap();
    assert_eq!(policy.verify(&note), Err(CheckpointError::LogSignatures { found: 0, required: 1 }));
}

#[test]
fn rejects_a_checkpoint_of_another_log_signed_by_a_trusted_name() {
    let origin = "registry.example/npm";
    let log = NoteSigner::new(origin, SigningKey::from_bytes(&[1; 32])).unwrap();
    let note = sign_checkpoint(&tree_head("registry.example/cargo", 3), &[&log]).unwrap();
    let log_keys = KeySet::new(vec![log.verifier_key().clone()], 1).unwrap();
    let policy = CheckpointPolicy::new(origin, log_keys, KeySet::empty()).unwrap();
    assert_eq!(
        policy.verify(&note),
        Err(CheckpointError::WrongOrigin {
            expected: origin.to_owned(),
            actual: "registry.example/cargo".to_owned(),
        }),
    );
}

#[test]
fn validates_key_sets_and_policies() {
    let log = NoteSigner::new("log.example", SigningKey::from_bytes(&[1; 32])).unwrap();
    let key = log.verifier_key().clone();
    assert_eq!(
        KeySet::new(vec![key.clone()], 2),
        Err(PolicyError::ThresholdExceedsKeys { threshold: 2, keys: 1 }),
    );
    assert_eq!(
        KeySet::new(vec![key.clone(), key.clone()], 1),
        Err(PolicyError::DuplicateKey { key: key.key_name() }),
    );
    let log_keys = KeySet::new(vec![key.clone()], 1).unwrap();
    assert_eq!(
        CheckpointPolicy::new(
            "log.example",
            KeySet::new(vec![key.clone()], 0).unwrap(),
            KeySet::empty()
        ),
        Err(PolicyError::NoLogSignatures),
    );
    let not_a_witness = KeySet::new(vec![key.clone()], 1).unwrap();
    assert_eq!(
        CheckpointPolicy::new("log.example", log_keys, not_a_witness),
        Err(PolicyError::InvalidWitnessKey { key: key.key_name() }),
    );
    assert_eq!(key.algorithm(), KeyAlgorithm::Ed25519);
    assert_eq!(log.name(), "log.example");
}
