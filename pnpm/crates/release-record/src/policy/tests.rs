use super::{LOG_POLICY_TARGET, LogPolicy, LogPolicyError};
use crate::{
    test_support::{
        FUTURE, LOG_NAME, RoleKeys, key, policy_document, registry, root, sign, snapshot, targets,
        timestamp,
    },
    tuf::TrustedRepository,
};
use pnpm_transparency_log::{PolicyError, TreeHead, add_signatures, record_hash, sign_checkpoint};
use pretty_assertions::assert_eq;
use serde_json::json;
use sha2::{Digest as _, Sha256};

#[test]
fn reads_a_policy_and_verifies_checkpoints_with_it() {
    let registry = registry();
    let policy = LogPolicy::parse(&policy_document(&registry, 1), [7; 32]).unwrap();
    assert_eq!(policy.authority().log_name(), LOG_NAME);
    assert_eq!(policy.authority().first_root_digest(), &[7; 32]);
    assert_eq!(policy.statement_keys().keys(), [registry.statements.verifier_key().clone()]);

    let head = TreeHead { origin: LOG_NAME.to_owned(), size: 10, hash: record_hash(b"root") };
    let note = sign_checkpoint(&head, &[&registry.log]).unwrap();
    let unwitnessed = policy.checkpoints().verify(&note);
    assert!(unwitnessed.is_err());
    let witnessed = add_signatures(&note, &[&registry.witness.at(1_793_000_000)]).unwrap();
    assert_eq!(
        policy
            .checkpoints()
            .verify(&witnessed)
            .unwrap()
            .tree_head,
        head,
    );
}

#[test]
fn a_private_registry_may_name_no_witnesses() {
    let registry = registry();
    let mut document: serde_json::Value =
        serde_json::from_slice(&policy_document(&registry, 0)).unwrap();
    document["witnesses"] = json!({ "keys": [], "threshold": 0 });
    let policy = LogPolicy::parse(&serde_json::to_vec(&document).unwrap(), [0; 32]).unwrap();
    let head = TreeHead { origin: LOG_NAME.to_owned(), size: 1, hash: record_hash(b"root") };
    let note = sign_checkpoint(&head, &[&registry.log]).unwrap();
    assert!(
        policy
            .checkpoints()
            .verify(&note)
            .is_ok(),
    );
}

#[test]
fn rejects_invalid_policies() {
    let registry = registry();
    let document: serde_json::Value =
        serde_json::from_slice(&policy_document(&registry, 1)).unwrap();
    let parse = |document: &serde_json::Value| {
        LogPolicy::parse(&serde_json::to_vec(document).unwrap(), [0; 32])
    };

    let mut wrong_schema = document.clone();
    wrong_schema["schema"] = json!("pnpr-log-policy/2");
    assert!(matches!(parse(&wrong_schema), Err(LogPolicyError::UnsupportedSchema { .. })));

    let mut witness_as_statement_key = document.clone();
    witness_as_statement_key["statementKeys"]["keys"] =
        json!([registry.witness.verifier_key().to_string()]);
    assert!(matches!(
        parse(&witness_as_statement_key),
        Err(LogPolicyError::InvalidStatementKey { .. })
    ));

    let mut no_statement_threshold = document.clone();
    no_statement_threshold["statementKeys"]["threshold"] = json!(0);
    assert!(matches!(parse(&no_statement_threshold), Err(LogPolicyError::NoStatementSignatures)));

    let mut other_log = document.clone();
    other_log["log"] = json!("registry.example/cargo");
    assert!(matches!(
        parse(&other_log),
        Err(LogPolicyError::Policy(PolicyError::InvalidLogKey { .. }))
    ));

    let mut garbled = document;
    garbled["checkpointKeys"]["keys"] = json!(["registry.example/npm+00000000+AAAA"]);
    assert!(matches!(parse(&garbled), Err(LogPolicyError::InvalidKey { .. })));
}

#[test]
fn authenticates_the_policy_through_tuf() {
    let registry = registry();
    let policy_bytes = policy_document(&registry, 1);
    let (root_key, online) = (key(20), key(21));
    let role_keys = RoleKeys {
        root: vec![&root_key],
        root_threshold: 1,
        timestamp: &online,
        snapshot: &online,
        targets: &online,
    };
    let first_root = sign(&root(1, FUTURE, &role_keys), &[&root_key]);
    let targets = sign(&targets(1, FUTURE, &[(LOG_POLICY_TARGET, &policy_bytes)]), &[&online]);
    let snapshot = sign(&snapshot(1, FUTURE, 1, &targets), &[&online]);
    let timestamp = sign(&timestamp(1, FUTURE, 1, &snapshot), &[&online]);

    let first_root_digest: [u8; 32] = Sha256::digest(&first_root).into();
    let mut repository = TrustedRepository::new(&first_root, first_root_digest).unwrap();
    let now = "2026-11-01T00:00:00Z".parse().unwrap();
    repository.update_timestamp(&timestamp, now).unwrap();
    repository.update_snapshot(&snapshot, now).unwrap();
    repository.update_targets(&targets, now).unwrap();

    let policy = LogPolicy::from_repository(&repository, &policy_bytes).unwrap();
    assert_eq!(policy.authority().first_root_digest(), &first_root_digest);
    assert!(
        policy
            .authority()
            .to_string()
            .starts_with("registry.example/npm+sha256:"),
    );

    let tampered = policy_document(&registry, 0);
    assert!(matches!(
        LogPolicy::from_repository(&repository, &tampered),
        Err(LogPolicyError::Tuf(_))
    ));
}
