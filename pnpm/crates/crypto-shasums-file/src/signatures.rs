use super::{FetchVerifiedNodeShasumsError, node_release_keys::NODE_RELEASE_KEYS};
use derive_more::{Display, Error};
use pgp::{
    composed::{Deserializable, DetachedSignature, SignedPublicKey},
    types::KeyDetails,
};
use std::{io::Cursor, sync::Arc};

/// An `OpenPGP` public key pnpm embeds to verify a release it downloads,
/// together with the fingerprint the embedded copy must have.
pub struct TrustedReleaseKey {
    pub fingerprint: &'static str,
    pub armored_key: &'static str,
}

/// Why a detached release signature could not be checked.
#[derive(Debug, Display, Error)]
pub enum ReleaseSignatureError {
    Unreadable {
        #[error(source)]
        error: Arc<pgp::errors::Error>,
    },

    #[display("embedded release key fingerprint mismatch: expected {expected}, got {actual}")]
    FingerprintMismatch {
        #[error(not(source))]
        expected: &'static str,
        #[error(not(source))]
        actual: String,
    },
}

/// Whether `signature_bytes`, a detached signature in binary or ASCII-armored
/// form, is a signature of `content` made by one of `keys` or one of their
/// subkeys.
pub fn is_signed_by_trusted_key(
    content: &[u8],
    signature_bytes: &[u8],
    keys: &[TrustedReleaseKey],
) -> Result<bool, ReleaseSignatureError> {
    let signature = if signature_bytes.starts_with(b"-----BEGIN PGP SIGNATURE-----") {
        DetachedSignature::from_armor_single(Cursor::new(signature_bytes))
            .map(|(signature, _headers)| signature)
    } else {
        DetachedSignature::from_bytes(Cursor::new(signature_bytes))
    }
    .map_err(signature_unreadable)?;
    for key in keys {
        let key = read_trusted_key(key)?;
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

pub(super) fn is_signed_by_trusted_node_release_key(
    content: &[u8],
    signature_bytes: &[u8],
) -> Result<bool, FetchVerifiedNodeShasumsError> {
    is_signed_by_trusted_key(content, signature_bytes, NODE_RELEASE_KEYS)
        .map_err(|error| match error {
            ReleaseSignatureError::Unreadable { error } => {
                FetchVerifiedNodeShasumsError::SignatureUnreadable { error }
            }
            ReleaseSignatureError::FingerprintMismatch { expected, actual } => {
                FetchVerifiedNodeShasumsError::EmbeddedKeyFingerprintMismatch { expected, actual }
            }
        })
}

fn read_trusted_key(
    trusted_key: &TrustedReleaseKey,
) -> Result<SignedPublicKey, ReleaseSignatureError> {
    let (key, _headers) = SignedPublicKey::from_armor_single(trusted_key.armored_key.as_bytes())
        .map_err(signature_unreadable)?;
    let actual_fingerprint = key.fingerprint().to_string();
    let fingerprint_matches = actual_fingerprint.eq_ignore_ascii_case(trusted_key.fingerprint);
    if !fingerprint_matches {
        return Err(ReleaseSignatureError::FingerprintMismatch {
            expected: trusted_key.fingerprint,
            actual: actual_fingerprint,
        });
    }
    Ok(key)
}

fn signature_unreadable(error: pgp::errors::Error) -> ReleaseSignatureError {
    ReleaseSignatureError::Unreadable { error: Arc::new(error) }
}
