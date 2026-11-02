//! A registry's log policy, the `pnpr-log-policy.json` target of its TUF
//! repository.
//!
//! ```json
//! {
//!   "schema": "pnpr-log-policy/1",
//!   "log": "registry.example/npm",
//!   "checkpointKeys": { "keys": ["registry.example/npm+<hash>+<key>"], "threshold": 1 },
//!   "statementKeys": { "keys": ["registry.example/statements+<hash>+<key>"], "threshold": 1 },
//!   "witnesses": { "keys": ["witness.example+<hash>+<key>"], "threshold": 1 }
//! }
//! ```
//!
//! Keys are note verifier keys. Checkpoint keys are Ed25519 note keys named
//! after the log, witness keys Ed25519 cosignature keys, and statement keys
//! Ed25519 note keys whose `<name>+<hash>` is the DSSE `keyid`. Rotating a
//! checkpoint or statement key changes this target, not the TUF root.

use crate::{
    identity::Authority,
    tuf::{TrustedRepository, TufError},
};
use derive_more::{Display, Error};
use pnpm_transparency_log::{
    CheckpointPolicy, KeyAlgorithm, KeyError, KeySet, PolicyError, VerifierKey,
};
use serde::Deserialize;

/// The target name of the log policy in a registry's TUF repository.
pub const LOG_POLICY_TARGET: &str = "pnpr-log-policy.json";

/// The `schema` of the log policy this crate reads.
pub const LOG_POLICY_SCHEMA: &str = "pnpr-log-policy/1";

/// A verified log policy: the registry's authority, what its checkpoints
/// must carry, and the keys its statements are signed with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogPolicy {
    authority: Authority,
    checkpoints: CheckpointPolicy,
    statement_keys: KeySet,
}

/// Why a log policy was rejected.
#[derive(Debug, Display, Error)]
#[non_exhaustive]
pub enum LogPolicyError {
    Tuf(TufError),
    #[display("log policy is not valid JSON: {_0}")]
    Json(serde_json::Error),
    #[display("unsupported log policy schema {schema}")]
    UnsupportedSchema {
        #[error(not(source))]
        schema: String,
    },
    #[display("invalid key {key} in the log policy: {error}")]
    InvalidKey {
        #[error(not(source))]
        key: String,
        error: KeyError,
    },
    Policy(PolicyError),
    #[display("statement key {key} must be an Ed25519 note key")]
    InvalidStatementKey {
        #[error(not(source))]
        key: String,
    },
    #[display("a log policy needs at least one statement signature")]
    NoStatementSignatures,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PolicyDocument {
    schema: String,
    log: String,
    checkpoint_keys: KeySetDocument,
    statement_keys: KeySetDocument,
    witnesses: KeySetDocument,
}

#[derive(Deserialize)]
struct KeySetDocument {
    keys: Vec<String>,
    threshold: usize,
}

impl LogPolicy {
    /// Verifies `bytes` as the log policy target of `repository`, whose
    /// targets metadata must already be trusted, and reads it.
    pub fn from_repository(
        repository: &TrustedRepository,
        bytes: &[u8],
    ) -> Result<Self, LogPolicyError> {
        repository.verify_target(LOG_POLICY_TARGET, bytes).map_err(LogPolicyError::Tuf)?;
        LogPolicy::parse(bytes, *repository.first_root_digest())
    }

    /// Reads a log policy whose bytes were already authenticated, for the
    /// TUF repository whose first root has `first_root_digest`.
    pub fn parse(bytes: &[u8], first_root_digest: [u8; 32]) -> Result<Self, LogPolicyError> {
        let document: PolicyDocument = serde_json::from_slice(bytes).map_err(LogPolicyError::Json)?;
        if document.schema != LOG_POLICY_SCHEMA {
            return Err(LogPolicyError::UnsupportedSchema { schema: document.schema });
        }
        let checkpoint_keys = key_set(&document.checkpoint_keys)?;
        let witnesses = key_set(&document.witnesses)?;
        let checkpoints = CheckpointPolicy::new(&document.log, checkpoint_keys, witnesses)
            .map_err(LogPolicyError::Policy)?;
        let statement_keys = key_set(&document.statement_keys)?;
        if statement_keys.threshold() == 0 {
            return Err(LogPolicyError::NoStatementSignatures);
        }
        let is_note_key = |key: &&VerifierKey| key.algorithm() == KeyAlgorithm::Ed25519;
        if let Some(key) = statement_keys
            .keys()
            .iter()
            .find(|key| !is_note_key(key))
        {
            return Err(LogPolicyError::InvalidStatementKey { key: key.key_name() });
        }
        let authority = Authority::new(&document.log, first_root_digest);
        Ok(LogPolicy { authority, checkpoints, statement_keys })
    }

    /// The registry's authority: its log name and first root digest.
    #[must_use]
    pub const fn authority(&self) -> &Authority {
        &self.authority
    }

    /// What the log's checkpoints must carry.
    #[must_use]
    pub const fn checkpoints(&self) -> &CheckpointPolicy {
        &self.checkpoints
    }

    /// The keys registry statements are signed with, and their threshold.
    #[must_use]
    pub const fn statement_keys(&self) -> &KeySet {
        &self.statement_keys
    }
}

fn key_set(document: &KeySetDocument) -> Result<KeySet, LogPolicyError> {
    let keys = document.keys
        .iter()
        .map(|key| {
            key.parse::<VerifierKey>()
                .map_err(|error| LogPolicyError::InvalidKey { key: key.clone(), error })
        })
        .collect::<Result<Vec<_>, _>>()?;
    KeySet::new(keys, document.threshold).map_err(LogPolicyError::Policy)
}

#[cfg(test)]
mod tests;
