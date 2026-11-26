use crate::base32;
use derive_more::{
    Display,
    Error,
};
use miette::Diagnostic;
use serde::{
    Deserialize,
    Serialize,
};

/// The only store directory pnpz supports. Binaries built by nixpkgs embed
/// it, so a path under another directory would not run.
pub const STORE_DIR: &str = "/nix/store";

const HASH_LEN: usize = 32;
const MAX_NAME_LEN: usize = 211;

/// A path directly under [`STORE_DIR`], held as its `<hash>-<name>` base name.
#[derive(Debug, Display, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[display("{STORE_DIR}/{base_name}")]
#[serde(try_from = "String", into = "String")]
pub struct StorePath {
    base_name: String,
}

#[derive(Debug, Display, Error, Diagnostic, Clone, PartialEq, Eq)]
#[display("{text:?} is not a Nix store path: {reason}")]
#[diagnostic(code(ERR_PNPZ_INVALID_STORE_PATH))]
pub struct ParseStorePathError {
    text: String,
    reason: &'static str,
}

impl StorePath {
    /// Parses `/nix/store/<hash>-<name>`.
    pub fn from_absolute(text: &str) -> Result<Self, ParseStorePathError> {
        let invalid = |reason| ParseStorePathError { text: text.to_string(), reason };
        let base_name = text
            .strip_prefix(STORE_DIR)
            .and_then(|rest| rest.strip_prefix('/'))
            .ok_or_else(|| invalid("it is not under /nix/store"))?;
        Self::from_base_name(base_name).map_err(|error| invalid(error.reason))
    }

    /// Parses `<hash>-<name>`, the form a `.narinfo` lists references in.
    pub fn from_base_name(text: &str) -> Result<Self, ParseStorePathError> {
        let invalid = |reason| ParseStorePathError { text: text.to_string(), reason };
        let (hash, name) = text
            .split_at_checked(HASH_LEN)
            .and_then(|(hash, rest)| Some((hash, rest.strip_prefix('-')?)))
            .ok_or_else(|| invalid("it does not start with a 32-character hash and a dash"))?;
        if !base32::is_valid(hash) {
            return Err(invalid("its hash is not Nix base32"));
        }
        if name.is_empty() || name.len() > MAX_NAME_LEN || !name.bytes().all(is_name_byte) {
            return Err(invalid("its name is empty, too long, or has a character Nix rejects"));
        }
        Ok(Self { base_name: text.to_string() })
    }

    #[must_use]
    pub fn base_name(&self) -> &str {
        &self.base_name
    }

    /// The hash part, which is the name of the path's `.narinfo`.
    #[must_use]
    pub fn hash_part(&self) -> &str {
        &self.base_name[..HASH_LEN]
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.base_name[HASH_LEN + 1..]
    }
}

impl TryFrom<String> for StorePath {
    type Error = ParseStorePathError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::from_absolute(&text)
    }
}

impl From<StorePath> for String {
    fn from(path: StorePath) -> Self {
        path.to_string()
    }
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"+-._?=".contains(&byte)
}

#[cfg(test)]
mod tests;
