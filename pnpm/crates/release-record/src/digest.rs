//! A record's digest: the SHA-512 of the bytes the registry stored and
//! serves at the record route.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use derive_more::{Display, Error};
use sha2::{Digest as _, Sha512};
use std::{
    fmt::{self, Write as _},
    str::FromStr,
};

/// The SHA-512 of a record's bytes. Written `sha512-<base64>`, as an npm
/// integrity is.
///
/// The digest covers the bytes as stored, after any HTTP content encoding is
/// decoded. A client hashes the bytes it received and parses them; it never
/// re-serializes a record.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct RecordDigest([u8; 64]);

/// Why a `sha512-<base64>` digest was rejected.
#[derive(Debug, Display, Error, Clone, PartialEq, Eq)]
#[display("not a sha512-<base64> record digest: {text}")]
pub struct DigestError {
    #[error(not(source))]
    text: String,
}

impl RecordDigest {
    /// The digest of `bytes`.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        RecordDigest(Sha512::digest(bytes).into())
    }

    /// The raw digest.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }

    /// The lowercase hex form an in-toto digest set uses.
    #[must_use]
    pub fn to_hex(&self) -> String {
        lowercase_hex(&self.0)
    }
}

impl fmt::Display for RecordDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "sha512-{}", STANDARD.encode(self.0))
    }
}

impl fmt::Debug for RecordDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl FromStr for RecordDigest {
    type Err = DigestError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let error = || DigestError { text: text.to_owned() };
        let encoded = text.strip_prefix("sha512-").ok_or_else(error)?;
        let decoded = STANDARD.decode(encoded).map_err(|_| error())?;
        Ok(RecordDigest(decoded.try_into().map_err(|_| error())?))
    }
}

/// The lowercase hex form of `bytes`.
pub(crate) fn lowercase_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut hex, byte| {
            // Writing to a `String` cannot fail.
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

#[cfg(test)]
mod tests;
