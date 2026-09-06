use derive_more::{Display, Error};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::fmt;

/// The only algorithm pnpr stores under. The spec allows others, but a
/// registry that accepted several would have to prove two spellings of the
/// same bytes are one blob; refusing the rest keeps a blob's identity single.
const ALGORITHM: &str = "sha256";

const HEX_LEN: usize = 64;

#[derive(Debug, Display, Error, Clone, PartialEq, Eq)]
pub enum DigestError {
    #[display("digest {raw:?} must be `<algorithm>:<hex>`")]
    Malformed { raw: String },
    #[display("digest algorithm {algorithm:?} is not supported, expected {ALGORITHM:?}")]
    UnsupportedAlgorithm { algorithm: String },
    #[display("digest {raw:?} must carry {HEX_LEN} lowercase hex characters")]
    NotHex { raw: String },
}

/// A `sha256:<hex>` content address.
///
/// Parsing is what admits a digest into a storage key, so it is strict about
/// case: `sha256:AB…` and `sha256:ab…` name the same bytes, and accepting
/// both would put one blob at two paths.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Digest {
    hex: String,
}

impl Digest {
    pub fn parse(raw: &str) -> Result<Self, DigestError> {
        let (algorithm, hex) =
            raw.split_once(':').ok_or_else(|| DigestError::Malformed { raw: raw.to_string() })?;
        if algorithm != ALGORITHM {
            return Err(DigestError::UnsupportedAlgorithm { algorithm: algorithm.to_string() });
        }
        if hex.len() != HEX_LEN
            || !hex.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(DigestError::NotHex { raw: raw.to_string() });
        }
        Ok(Self { hex: hex.to_string() })
    }

    /// The digest of `bytes`, as the registry would compute it to verify an
    /// upload.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        Self { hex: format!("{:x}", Sha256::digest(bytes)) }
    }

    /// The blob's single path segment in the store. `:` cannot appear in a
    /// storage key, so the separator becomes `-`.
    #[must_use]
    pub fn blob_filename(&self) -> String {
        format!("{ALGORITHM}-{}", self.hex)
    }

    #[must_use]
    pub fn hex(&self) -> &str {
        &self.hex
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{ALGORITHM}:{}", self.hex)
    }
}

impl TryFrom<String> for Digest {
    type Error = DigestError;

    fn try_from(raw: String) -> Result<Self, DigestError> {
        Self::parse(&raw)
    }
}

impl From<Digest> for String {
    fn from(digest: Digest) -> Self {
        digest.to_string()
    }
}
