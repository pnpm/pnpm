//! Role signature thresholds and file length and hash checks.

use super::{
    TufError, canonical,
    metadata::{Key, RawSignature, Root, Signed},
};
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest as _, Sha256, Sha512};
use std::collections::{BTreeMap, HashSet};

/// Fails unless `metadata` carries valid signatures from the threshold of
/// distinct keys that `root` assigns to `role`.
pub(super) fn verify_role<Body>(
    root: &Root,
    role: &'static str,
    metadata: &Signed<Body>,
) -> Result<(), TufError> {
    let role_keys = root.roles
        .get(role)
        .ok_or(TufError::MissingRole { role })?;
    let message = canonical::encode(&metadata.value)?;
    let signers: HashSet<&str> = metadata.signatures
        .iter()
        .filter(|signature| role_keys.keyids.contains(&signature.keyid))
        .filter(|signature| {
            root.keys.get(&signature.keyid).is_some_and(|key| signs(key, &message, signature))
        })
        .map(|signature| signature.keyid.as_str())
        .collect();
    let required = role_keys.threshold;
    if required == 0 || (signers.len() as u64) < required {
        return Err(TufError::Threshold { role, found: signers.len(), required });
    }
    Ok(())
}

/// Whether `signature` is an Ed25519 signature of `message` by `key`.
fn signs(key: &Key, message: &[u8], signature: &RawSignature) -> bool {
    if key.keytype != "ed25519" || key.scheme != "ed25519" {
        return false;
    }
    let public_key = decode_hex(&key.keyval.public)
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
        .and_then(|bytes| VerifyingKey::from_bytes(&bytes).ok());
    let signature = decode_hex(&signature.sig).and_then(|bytes| Signature::from_slice(&bytes).ok());
    match (public_key, signature) {
        (Some(public_key), Some(signature)) => {
            public_key.verify_strict(message, &signature).is_ok()
        }
        _ => false,
    }
}

/// Fails unless `bytes` has the listed length, if any, and every listed
/// SHA-256 and SHA-512 hash. With `require_hash`, at least one of those
/// hashes must be listed.
pub(super) fn check_file(
    file: &str,
    bytes: &[u8],
    length: Option<u64>,
    hashes: &BTreeMap<String, String>,
    require_hash: bool,
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
    if require_hash && checked == 0 {
        return Err(TufError::NoSupportedHash { file: file.to_owned() });
    }
    Ok(())
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
