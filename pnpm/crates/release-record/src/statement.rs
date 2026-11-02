//! [in-toto Statements](https://github.com/in-toto/attestation/blob/v1.2.0/spec/v1/statement.md)
//! about records, signed as DSSE envelopes.

pub use predicate::*;

mod predicate;

use crate::{
    dsse::{DsseError, Envelope, IN_TOTO_PAYLOAD_TYPE},
    record::StoredRecord,
};
use derive_more::{Display, Error};
use pnpm_transparency_log::{KeySet, NoteSigner};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// The `_type` of an in-toto Statement v1.
pub const STATEMENT_TYPE: &str = "https://in-toto.io/Statement/v1";

/// An in-toto Statement v1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Statement {
    /// Always [`STATEMENT_TYPE`].
    #[serde(rename = "_type")]
    pub statement_type: String,
    /// What the statement is about.
    pub subject: Vec<Subject>,
    /// What the statement claims, as a URI.
    pub predicate_type: String,
    /// The claim.
    #[serde(default)]
    pub predicate: Value,
}

/// A statement subject: a name and the digests of its bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subject {
    /// The record's PURL, or `<repository>:<tag>+r<revision>` for an OCI tag
    /// record.
    pub name: String,
    /// The digests by algorithm, in lowercase hex.
    pub digest: BTreeMap<String, String>,
}

/// Why a statement about a record was rejected.
#[derive(Debug, Display, Error)]
#[non_exhaustive]
pub enum StatementError {
    Envelope(DsseError),
    #[display("envelope payload type is {payload_type}, not an in-toto statement")]
    PayloadType {
        #[error(not(source))]
        payload_type: String,
    },
    #[display("statement is not valid JSON: {_0}")]
    Json(serde_json::Error),
    #[display("statement type is {statement_type}, expected {STATEMENT_TYPE}")]
    StatementType {
        #[error(not(source))]
        statement_type: String,
    },
    #[display("statement claims {actual}, expected {expected}")]
    PredicateType {
        #[error(not(source))]
        expected: &'static str,
        #[error(not(source))]
        actual: String,
    },
    #[display("{predicate_type} statements are not made about {profile} records")]
    NotApplicable {
        #[error(not(source))]
        predicate_type: &'static str,
        #[error(not(source))]
        profile: String,
    },
    #[display("statement subject is not the record {name}")]
    SubjectMismatch {
        #[error(not(source))]
        name: String,
    },
}

impl Statement {
    /// A statement claiming `predicate_type` about `record`, by its name and
    /// SHA-512.
    pub fn about(
        record: &StoredRecord,
        predicate_type: PredicateType,
        predicate: Value,
    ) -> Result<Self, StatementError> {
        check_applicable(record, predicate_type)?;
        Ok(Statement {
            statement_type: STATEMENT_TYPE.to_owned(),
            subject: vec![subject_of(record)],
            predicate_type: predicate_type.uri().to_owned(),
            predicate,
        })
    }

    /// Signs the statement as a DSSE envelope.
    #[must_use]
    pub fn sign(&self, signers: &[&NoteSigner]) -> Envelope {
        let body = serde_json::to_vec(self).expect("a statement always serializes");
        Envelope::sign(IN_TOTO_PAYLOAD_TYPE, &body, signers)
    }
}

/// Verifies `envelope` as a statement claiming `predicate_type` about
/// `record`, signed by the threshold of the registry's statement `keys`.
pub fn verify_statement(
    envelope: &Envelope,
    keys: &KeySet,
    record: &StoredRecord,
    predicate_type: PredicateType,
) -> Result<Statement, StatementError> {
    check_applicable(record, predicate_type)?;
    let payload = envelope.verify(keys).map_err(StatementError::Envelope)?;
    if payload.payload_type != IN_TOTO_PAYLOAD_TYPE {
        return Err(StatementError::PayloadType { payload_type: payload.payload_type });
    }
    let statement: Statement = serde_json::from_slice(&payload.body).map_err(StatementError::Json)?;
    if statement.statement_type != STATEMENT_TYPE {
        return Err(StatementError::StatementType { statement_type: statement.statement_type });
    }
    if statement.predicate_type != predicate_type.uri() {
        return Err(StatementError::PredicateType {
            expected: predicate_type.uri(),
            actual: statement.predicate_type,
        });
    }
    let expected = subject_of(record);
    if !statement.subject.iter().any(|subject| names_record(subject, &expected)) {
        return Err(StatementError::SubjectMismatch { name: expected.name });
    }
    Ok(statement)
}

fn subject_of(record: &StoredRecord) -> Subject {
    let digest = BTreeMap::from([("sha512".to_owned(), record.digest().to_hex())]);
    Subject { name: record.key().subject_name(), digest }
}

/// Whether `subject` names the record: the same name and the same SHA-512.
/// Other digest algorithms in the subject are not trusted to identify it.
fn names_record(subject: &Subject, record: &Subject) -> bool {
    subject.name == record.name && subject.digest.get("sha512") == record.digest.get("sha512")
}

fn check_applicable(
    record: &StoredRecord,
    predicate_type: PredicateType,
) -> Result<(), StatementError> {
    if predicate_type.applies_to(&record.key().profile) {
        return Ok(());
    }
    Err(StatementError::NotApplicable {
        predicate_type: predicate_type.uri(),
        profile: record.key().profile.to_string(),
    })
}

#[cfg(test)]
mod tests;
