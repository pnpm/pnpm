use crate::base32;
use base64::{
    Engine,
    engine::general_purpose::STANDARD as BASE64,
};
use derive_more::{
    Display,
    Error,
};
use miette::Diagnostic;
use serde::{
    Deserialize,
    Serialize,
};

const DIGEST_LEN: usize = 32;

/// The SHA-256 of a store path's NAR serialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct NarHash([u8; DIGEST_LEN]);

#[derive(Debug, Display, Error, Diagnostic, Clone, PartialEq, Eq)]
#[display("{text:?} is not a SHA-256 NAR hash")]
#[diagnostic(code(ERR_PNPZ_INVALID_NAR_HASH))]
pub struct ParseNarHashError {
    text: String,
}

impl NarHash {
    pub fn from_digest(digest: [u8; DIGEST_LEN]) -> Self {
        Self(digest)
    }

    /// Parses `sha256:<nix base32>`, `sha256:<hex>`, or the SRI form
    /// `sha256-<base64>`.
    pub fn parse(text: &str) -> Result<Self, ParseNarHashError> {
        decode(text)
            .and_then(|bytes| <[u8; DIGEST_LEN]>::try_from(bytes).ok())
            .map(Self)
            .ok_or_else(|| ParseNarHashError { text: text.to_string() })
    }

    /// `sha256:<nix base32>`, the form a `.narinfo` signature covers.
    pub fn to_nix_base32(self) -> String {
        format!("sha256:{}", base32::encode(&self.0))
    }

    /// `sha256-<base64>`, the form lockfiles record.
    pub fn to_sri(self) -> String {
        format!("sha256-{}", BASE64.encode(self.0))
    }
}

impl TryFrom<String> for NarHash {
    type Error = ParseNarHashError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text)
    }
}

impl From<NarHash> for String {
    fn from(hash: NarHash) -> Self {
        hash.to_sri()
    }
}

fn decode(text: &str) -> Option<Vec<u8>> {
    if let Some(sri) = text.strip_prefix("sha256-") {
        return BASE64.decode(sri).ok();
    }
    let digits = text.strip_prefix("sha256:")?;
    if digits.len() == DIGEST_LEN * 2 {
        return decode_hex(digits);
    }
    base32::decode(digits, DIGEST_LEN).ok()
}

fn decode_hex(digits: &str) -> Option<Vec<u8>> {
    digits
        .as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).ok()?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests;
