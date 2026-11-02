//! Role signature thresholds and file length and hash checks.

use super::{
    TufError, canonical,
    metadata::{Key, RawSignature, Root, Signed},
};
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest as _, Sha256, Sha512};
use std::collections::{BTreeMap, HashSet};

/// Fails unless `metadata` carries valid signatures from the threshold of
/// distinct keys that `root` assigns to `role`. Keys are told apart by their
/// public key, so one key listed under two key IDs counts once.
pub(super) fn verify_role<Body>(
    root: &Root,
    role: &'static str,
    metadata: &Signed<Body>,
) -> Result<(), TufError> {
    let role_keys = root.roles
        .get(role)
        .ok_or(TufError::MissingRole { role })?;
    let message = canonical::encode(&metadata.value)?;
    let signers: HashSet<[u8; 32]> = metadata.signatures
        .iter()
        .filter(|signature| role_keys.keyids.contains(&signature.keyid))
        .filter_map(|signature| signer(root.keys.get(&signature.keyid)?, &message, signature))
        .collect();
    let required = role_keys.threshold;
    if required == 0 || (signers.len() as u64) < required {
        return Err(TufError::Threshold { role, found: signers.len(), required });
    }
    Ok(())
}

/// The public key of `key`, if `signature` is its Ed25519 signature of
/// `message`.
fn signer(key: &Key, message: &[u8], signature: &RawSignature) -> Option<[u8; 32]> {
    if key.keytype != "ed25519" || key.scheme != "ed25519" {
        return None;
    }
    let public_key = decode_hex(&key.keyval.public)
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
        .and_then(|bytes| VerifyingKey::from_bytes(&bytes).ok());
    let signature = decode_hex(&signature.sig).and_then(|bytes| Signature::from_slice(&bytes).ok());
    let (public_key, signature) = (public_key?, signature?);
    public_key.verify_strict(message, &signature).ok()?;
    Some(public_key.to_bytes())
}

/// Whether a listed file must carry at least one SHA-256 or SHA-512 hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HashRequirement {
    /// A target, which TUF always lists with hashes.
    Required,
    /// Snapshot or targets metadata, which may be listed by version alone.
    Optional,
}

/// Fails unless `bytes` has the listed length, if any, and every listed
/// SHA-256 and SHA-512 hash, of which `requirement` says whether at least
/// one must be listed.
pub(super) fn check_file(
    file: &str,
    bytes: &[u8],
    length: Option<u64>,
    hashes: &BTreeMap<String, String>,
    requirement: HashRequirement,
) -> Result<(), TufError> {
    let mismatch = || TufError::FileMismatch { file: file.to_owned() };
    if length.is_some_and(|length| length != bytes.len() as u64) {
        return Err(mismatch());
    }
    let mut checked = 0;
    for (algorithm, expected) in hashes {
        let actual = match algorithm.as_str() {
            "sha256" => Sha256::digest(bytes).to_vec(),
            "sha512" => Sha512::digest(bytes).to_vec(),
            _ => continue,
        };
        if decode_hex(expected).as_deref() != Some(actual.as_slice()) {
            return Err(mismatch());
        }
        checked += 1;
    }
    match (requirement, checked) {
        (HashRequirement::Required, 0) => Err(TufError::NoSupportedHash { file: file.to_owned() }),
        (HashRequirement::Required | HashRequirement::Optional, _) => Ok(()),
    }
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|position| u8::from_str_radix(text.get(position..position + 2)?, 16).ok())
        .collect()
}
