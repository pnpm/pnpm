//! Builders for the TUF repositories, log policies, and records the tests
//! verify.

use crate::tuf::canonical::encode as canonical;
use ed25519_dalek::{Signer as _, SigningKey};
use pnpm_transparency_log::{Cosigner, NoteSigner};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

pub const FUTURE: &str = "2099-01-01T00:00:00Z";
pub const PAST: &str = "2000-01-01T00:00:00Z";

pub fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

pub fn hex(bytes: &[u8]) -> String {
    crate::digest::lowercase_hex(bytes)
}

pub fn key_id(key: &SigningKey) -> String {
    hex(&Sha256::digest(key.verifying_key().as_bytes()))
}

fn key_json(key: &SigningKey) -> Value {
    json!({
        "keytype": "ed25519",
        "scheme": "ed25519",
        "keyval": { "public": hex(key.verifying_key().as_bytes()) },
    })
}

/// The keys of each top-level role.
pub struct RoleKeys<'a> {
    pub root: Vec<&'a SigningKey>,
    pub root_threshold: u64,
    pub timestamp: &'a SigningKey,
    pub snapshot: &'a SigningKey,
    pub targets: &'a SigningKey,
}

pub fn root(version: u64, expires: &str, keys: &RoleKeys<'_>) -> Value {
    let mut all_keys = serde_json::Map::new();
    let roles = [keys.timestamp, keys.snapshot, keys.targets];
    for key in keys.root.iter().copied().chain(roles) {
        all_keys.insert(key_id(key), key_json(key));
    }
    let root_ids: Vec<String> = keys.root
        .iter()
        .map(|key| key_id(key))
        .collect();
    let role = |key: &SigningKey| json!({ "keyids": [key_id(key)], "threshold": 1 });
    json!({
        "_type": "root",
        "spec_version": "1.0.31",
        "version": version,
        "expires": expires,
        "consistent_snapshot": false,
        "keys": all_keys,
        "roles": {
            "root": { "keyids": root_ids, "threshold": keys.root_threshold },
            "timestamp": role(keys.timestamp),
            "snapshot": role(keys.snapshot),
            "targets": role(keys.targets),
        },
    })
}

fn listing(version: u64, bytes: &[u8]) -> Value {
    json!({ "version": version, "length": bytes.len(), "hashes": { "sha256": hex(&Sha256::digest(bytes)) } })
}

pub fn timestamp(version: u64, expires: &str, snapshot_version: u64, snapshot: &[u8]) -> Value {
    json!({
        "_type": "timestamp",
        "spec_version": "1.0.31",
        "version": version,
        "expires": expires,
        "meta": { "snapshot.json": listing(snapshot_version, snapshot) },
    })
}

pub fn snapshot(version: u64, expires: &str, targets_version: u64, targets: &[u8]) -> Value {
    json!({
        "_type": "snapshot",
        "spec_version": "1.0.31",
        "version": version,
        "expires": expires,
        "meta": { "targets.json": listing(targets_version, targets) },
    })
}

pub fn targets(version: u64, expires: &str, files: &[(&str, &[u8])]) -> Value {
    let targets: serde_json::Map<String, Value> = files
        .iter()
        .map(|(path, bytes)| {
            let hashes = json!({ "sha256": hex(&Sha256::digest(bytes)) });
            ((*path).to_owned(), json!({ "length": bytes.len(), "hashes": hashes }))
        })
        .collect();
    json!({
        "_type": "targets",
        "spec_version": "1.0.31",
        "version": version,
        "expires": expires,
        "targets": targets,
    })
}

/// Signed metadata bytes: `signed` with a signature from each of `signers`.
pub fn sign(signed: &Value, signers: &[&SigningKey]) -> Vec<u8> {
    let message = canonical(signed).unwrap();
    let signatures: Vec<Value> = signers
        .iter()
        .map(|signer| json!({ "keyid": key_id(signer), "sig": hex(&signer.sign(&message).to_bytes()) }))
        .collect();
    serde_json::to_vec_pretty(&json!({ "signatures": signatures, "signed": signed })).unwrap()
}

/// The keys a test registry signs with.
pub struct Registry {
    pub log: NoteSigner,
    pub statements: NoteSigner,
    pub witness: Cosigner,
}

pub const LOG_NAME: &str = "registry.example/npm";

pub fn registry() -> Registry {
    Registry {
        log: NoteSigner::new(LOG_NAME, key(40)).unwrap(),
        statements: NoteSigner::new("registry.example/statements", key(41)).unwrap(),
        witness: Cosigner::new("witness.example/ci", key(42)).unwrap(),
    }
}

pub fn policy_document(registry: &Registry, witness_threshold: usize) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema": "pnpr-log-policy/1",
        "log": LOG_NAME,
        "checkpointKeys": { "keys": [registry.log.verifier_key().to_string()], "threshold": 1 },
        "statementKeys": { "keys": [registry.statements.verifier_key().to_string()], "threshold": 1 },
        "witnesses": { "keys": [registry.witness.verifier_key().to_string()], "threshold": witness_threshold },
    }))
    .unwrap()
}

pub const NPM_RECORD: &str = r#"{
  "schema": "pnpr-release-record/1",
  "purl": "pkg:npm/%40example/widgets@1.2.0",
  "name": "@example/widgets",
  "version": "1.2.0",
  "revision": 0,
  "interpretation": "npm-install-manifest/1",
  "integrity": "sha512-AAAA",
  "manifest": { "dependencies": { "@example/core": "^2.0.0" } }
}
"#;

pub const OCI_RECORD: &str = r#"{"schema":"pnpr-release-record/1","repository":"acme/app","tag":"1.0","revision":3,"interpretation":"oci-tag/1","purl":"pkg:oci/app@sha256:0123?repository_url=registry.example%2Foci%2Facme%2Fapp&tag=1.0"}"#;

pub const OCI_TOMBSTONE: &str = r#"{"schema":"pnpr-release-record/1","repository":"acme/app","tag":"1.0","revision":4,"interpretation":"oci-tag/1","purl":null}"#;
