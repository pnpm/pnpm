use super::{
    FetchVerifiedNodeShasumsError,
    node_release_keys::{NODE_RELEASE_KEYS, NodeReleaseKey},
};
use pgp::{
    composed::{Deserializable, DetachedSignature, SignedPublicKey},
    types::KeyDetails,
};
use std::{io::Cursor, sync::Arc};
pub(super) fn is_signed_by_trusted_node_release_key(
    content: &[u8],
    signature_bytes: &[u8],
) -> Result<bool, FetchVerifiedNodeShasumsError> {
    let signature =
        DetachedSignature::from_bytes(Cursor::new(signature_bytes)).map_err(signature_unreadable)?;
    for key in trusted_node_release_keys()? {
        if signature.verify(&key.primary_key, content).is_ok() {
            return Ok(true);
        }
        for subkey in &key.public_subkeys {
            if signature.verify(subkey, content).is_ok() {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn trusted_node_release_keys() -> Result<Vec<SignedPublicKey>, FetchVerifiedNodeShasumsError> {
    NODE_RELEASE_KEYS
        .iter()
        .map(read_node_release_key)
        .collect()
}

fn read_node_release_key(
    trusted_key: &NodeReleaseKey,
) -> Result<SignedPublicKey, FetchVerifiedNodeShasumsError> {
    let (key, _headers) = SignedPublicKey::from_armor_single(trusted_key.armored_key.as_bytes())
        .map_err(signature_unreadable)?;
    let actual_fingerprint = key.fingerprint().to_string();
    let fingerprint_matches = actual_fingerprint.eq_ignore_ascii_case(trusted_key.fingerprint);
    if !fingerprint_matches {
        return Err(FetchVerifiedNodeShasumsError::EmbeddedKeyFingerprintMismatch {
            expected: trusted_key.fingerprint,
            actual: actual_fingerprint,
        });
    }
    Ok(key)
}

fn signature_unreadable(error: pgp::errors::Error) -> FetchVerifiedNodeShasumsError {
    FetchVerifiedNodeShasumsError::SignatureUnreadable { error: Arc::new(error) }
}
