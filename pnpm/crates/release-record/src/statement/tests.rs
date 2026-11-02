use super::{
    AcceptancePredicate, PredicateType, PrincipalKind, PublicationPredicate, Statement,
    StatementError, TagAction, TagBindingPredicate, verify_statement,
};
use crate::{
    dsse::{Envelope, IN_TOTO_PAYLOAD_TYPE},
    record::StoredRecord,
    test_support::{NPM_RECORD, OCI_TOMBSTONE, registry},
};
use pnpm_transparency_log::KeySet;
use pretty_assertions::assert_eq;
use serde_json::json;

fn npm_record() -> StoredRecord {
    StoredRecord::parse(NPM_RECORD.as_bytes().to_vec()).unwrap()
}

fn statement_keys() -> KeySet {
    KeySet::new(vec![registry().statements.verifier_key().clone()], 1).unwrap()
}

#[test]
fn signs_and_verifies_an_acceptance_statement() {
    let record = npm_record();
    let predicate = AcceptancePredicate { interpretation: record.key().profile.clone() };
    let statement = Statement::about(
        &record,
        PredicateType::Acceptance,
        serde_json::to_value(predicate).unwrap(),
    )
    .unwrap();
    assert_eq!(statement.subject[0].name, "pkg:npm/%40example/widgets@1.2.0");
    assert_eq!(statement.subject[0].digest["sha512"], record.digest().to_hex());

    let envelope = statement.sign(&[&registry().statements]);
    assert_eq!(envelope.payload_type, IN_TOTO_PAYLOAD_TYPE);
    let verified =
        verify_statement(&envelope, &statement_keys(), &record, PredicateType::Acceptance).unwrap();
    assert_eq!(verified, statement);
}

#[test]
fn verifies_a_statement_signed_by_another_implementation() {
    let envelope = include_str!("../../tests/fixtures/python-dsse-envelope.txt")
        .lines()
        .nth(1)
        .unwrap();
    let envelope: Envelope = serde_json::from_str(envelope).unwrap();
    let statement =
        verify_statement(&envelope, &statement_keys(), &npm_record(), PredicateType::Acceptance)
            .unwrap();
    assert_eq!(statement.predicate, json!({ "interpretation": "npm-install-manifest/1" }));
}

#[test]
fn rejects_a_statement_about_other_bytes() {
    let record = npm_record();
    let statement = Statement::about(&record, PredicateType::Acceptance, json!({})).unwrap();
    let envelope = statement.sign(&[&registry().statements]);
    let reformatted =
        StoredRecord::parse(NPM_RECORD.replace("\n  ", "\n    ").into_bytes()).unwrap();
    let error =
        verify_statement(&envelope, &statement_keys(), &reformatted, PredicateType::Acceptance)
            .unwrap_err();
    assert!(matches!(error, StatementError::SubjectMismatch { .. }), "{error}");
}

#[test]
fn rejects_a_statement_claiming_something_else() {
    let record = npm_record();
    let statement = Statement::about(&record, PredicateType::Acceptance, json!({})).unwrap();
    let envelope = statement.sign(&[&registry().statements]);
    let error = verify_statement(&envelope, &statement_keys(), &record, PredicateType::Publication)
        .unwrap_err();
    assert!(matches!(error, StatementError::PredicateType { .. }), "{error}");
}

#[test]
fn rejects_a_statement_from_an_untrusted_key() {
    let record = npm_record();
    let statement = Statement::about(&record, PredicateType::Acceptance, json!({})).unwrap();
    let envelope = statement.sign(&[&registry().log]);
    let error = verify_statement(&envelope, &statement_keys(), &record, PredicateType::Acceptance)
        .unwrap_err();
    assert!(matches!(error, StatementError::Envelope(_)), "{error}");
}

#[test]
fn oci_tag_records_get_only_tag_bindings() {
    let tombstone = StoredRecord::parse(OCI_TOMBSTONE.as_bytes().to_vec()).unwrap();
    for predicate_type in [PredicateType::Acceptance, PredicateType::Publication] {
        let error = Statement::about(&tombstone, predicate_type, json!({})).unwrap_err();
        assert!(matches!(error, StatementError::NotApplicable { .. }), "{error}");
    }
    let predicate = TagBindingPredicate {
        action: TagAction::Removed,
        principal: Some("token:ci-publisher".to_owned()),
    };
    let statement = Statement::about(
        &tombstone,
        PredicateType::TagBinding,
        serde_json::to_value(predicate).unwrap(),
    )
    .unwrap();
    assert_eq!(statement.subject[0].name, "acme/app:1.0+r4");
    assert_eq!(
        statement.predicate,
        json!({ "action": "removed", "principal": "token:ci-publisher" }),
    );
    let envelope = statement.sign(&[&registry().statements]);
    assert!(
        verify_statement(&envelope, &statement_keys(), &tombstone, PredicateType::TagBinding)
            .is_ok(),
    );
    let error = Statement::about(&npm_record(), PredicateType::TagBinding, json!({})).unwrap_err();
    assert!(matches!(error, StatementError::NotApplicable { .. }), "{error}");
}

#[test]
fn reads_predicate_type_uris() {
    for predicate_type in
        [PredicateType::Acceptance, PredicateType::Publication, PredicateType::TagBinding]
    {
        assert_eq!(PredicateType::from_uri(predicate_type.uri()), Some(predicate_type));
    }
    assert_eq!(PredicateType::TagBinding.uri(), "https://pnpm.io/pnpr/tag-binding/v1");
    assert_eq!(PredicateType::from_uri("https://slsa.dev/provenance/v1"), None);
}

#[test]
fn serializes_a_publication_predicate_around_the_publication_record() {
    let predicate: PublicationPredicate = serde_json::from_value(json!({
        "principalKind": "oidc-workload",
        "workload": { "issuer": "https://token.actions.githubusercontent.com", "subject": "repo:example/widgets:ref:refs/heads/main", "claims": { "workflow": "release.yml" } },
        "staged": false,
        "publication": "01J0000000000000000000000",
        "commit": "0123456789abcdef",
    }))
    .unwrap();
    assert_eq!(predicate.principal_kind, PrincipalKind::OidcWorkload);
    assert_eq!(predicate.publication["commit"], "0123456789abcdef");
    let round_trip = serde_json::to_value(&predicate).unwrap();
    assert_eq!(round_trip["publication"], "01J0000000000000000000000");
    assert!(round_trip.get("approver").is_none());
}
