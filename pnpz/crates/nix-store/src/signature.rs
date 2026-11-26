use crate::NarInfo;
use base64::{
    Engine,
    engine::general_purpose::STANDARD as BASE64,
};
use derive_more::{
    Display,
    Error,
};
use ed25519_dalek::{
    Signature,
    VerifyingKey,
};
use miette::Diagnostic;

/// The key `cache.nixos.org` signs with.
pub const NIXOS_CACHE_KEY: &str = "cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY=";

/// A public key a client accepts store paths from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedKey {
    name: String,
    key: VerifyingKey,
}

#[derive(Debug, Display, Error, Diagnostic, Clone, PartialEq, Eq)]
#[display("{text:?} is not a public key in the form <name>:<base64 ed25519 key>")]
#[diagnostic(code(ERR_PNPZ_INVALID_TRUSTED_KEY))]
pub struct ParseTrustedKeyError {
    text: String,
}

#[derive(Debug, Display, Error, Diagnostic, Clone, PartialEq, Eq)]
#[display(
    "{store_path} has no valid signature by a trusted key. It is signed by: {}",
    if signers.is_empty() { "nobody".to_string() } else { signers.join(", ") }
)]
#[diagnostic(code(ERR_PNPZ_UNTRUSTED_STORE_PATH))]
pub struct SignatureError {
    store_path: String,
    signers: Vec<String>,
}

impl TrustedKey {
    pub fn parse(text: &str) -> Result<Self, ParseTrustedKeyError> {
        let (name, key) = split_named_bytes::<32>(text)
            .and_then(|(name, bytes)| Some((name, VerifyingKey::from_bytes(&bytes).ok()?)))
            .ok_or_else(|| ParseTrustedKeyError { text: text.to_string() })?;
        Ok(Self { name: name.to_string(), key })
    }

    fn has_signed(&self, fingerprint: &str, signature: &str) -> bool {
        split_named_bytes::<64>(signature)
            .is_some_and(|(name, bytes)| {
                name == self.name
                    && self.key
                        .verify_strict(fingerprint.as_bytes(), &Signature::from_bytes(&bytes))
                        .is_ok()
            })
    }
}

impl NarInfo {
    /// Succeeds when one of `keys` signed this document's fingerprint.
    pub fn verify(&self, keys: &[TrustedKey]) -> Result<(), SignatureError> {
        let fingerprint = self.fingerprint();
        let is_trusted = self.signatures
            .iter()
            .any(|signature| {
                keys.iter()
                    .any(|key| key.has_signed(&fingerprint, signature))
            });
        if is_trusted {
            return Ok(());
        }
        Err(SignatureError {
            store_path: self.store_path.to_string(),
            signers: self.signatures
                .iter()
                .filter_map(|signature| signer_name(signature))
                .collect(),
        })
    }
}

fn signer_name(signature: &str) -> Option<String> {
    signature.split_once(':').map(|(name, _)| name.to_string())
}

/// Splits `<name>:<base64>` and decodes the second half to `LEN` bytes.
fn split_named_bytes<const LEN: usize>(text: &str) -> Option<(&str, [u8; LEN])> {
    let (name, encoded) = text.split_once(':')?;
    let bytes = BASE64
        .decode(encoded)
        .ok()?
        .try_into()
        .ok()?;
    (!name.is_empty()).then_some((name, bytes))
}

#[cfg(test)]
mod tests;
