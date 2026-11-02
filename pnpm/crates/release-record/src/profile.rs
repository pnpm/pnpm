//! Interpretation profiles: the rules a record's metadata was derived under.

use derive_more::{Display, Error};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

/// A profile, always `<name>/<version>` with each part in `[a-z0-9.-]+`, so
/// it is two URL path segments and never needs encoding.
///
/// A correction to a derivation is a new profile; records are never
/// re-derived under the profile they were accepted with. A client supports a
/// set of profiles and treats any other as a capability error.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Profile(String);

/// Why a profile name was rejected.
#[derive(Debug, Display, Error, Clone, PartialEq, Eq)]
#[display("not a <name>/<version> profile: {text}")]
pub struct ProfileError {
    #[error(not(source))]
    text: String,
}

impl Profile {
    /// The npm profile: one record per version, covering its primary archive
    /// and every platform variant.
    pub const NPM_INSTALL_MANIFEST: &str = "npm-install-manifest/1";
    /// The Cargo profile: one record per crate version, whose manifest is the
    /// index entry Cargo resolves from.
    pub const CARGO_INDEX_ENTRY: &str = "cargo-index-entry/1";
    /// The Python profile: one record per distribution file.
    pub const PYTHON_CORE_METADATA: &str = "python-core-metadata/1";
    /// The OCI profile: one record per setting of a tag.
    pub const OCI_TAG: &str = "oci-tag/1";

    /// The profile's text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether this is the OCI tag profile, whose records bind a tag rather
    /// than describe a released artifact.
    #[must_use]
    pub fn is_oci_tag(&self) -> bool {
        self.0 == Profile::OCI_TAG
    }

    /// The Package URL type the records of a known profile describe.
    #[must_use]
    pub fn purl_type(&self) -> Option<&'static str> {
        match self.0.as_str() {
            Profile::NPM_INSTALL_MANIFEST => Some("npm"),
            Profile::CARGO_INDEX_ENTRY => Some("cargo"),
            Profile::PYTHON_CORE_METADATA => Some("pypi"),
            Profile::OCI_TAG => Some("oci"),
            _ => None,
        }
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for Profile {
    type Err = ProfileError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        text.to_owned().try_into()
    }
}

impl TryFrom<String> for Profile {
    type Error = ProfileError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        let is_part = |part: &str| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-'))
        };
        let is_profile = matches!(
            text.split_once('/'),
            Some((name, version)) if is_part(name) && is_part(version),
        );
        if is_profile { Ok(Profile(text)) } else { Err(ProfileError { text }) }
    }
}

impl From<Profile> for String {
    fn from(profile: Profile) -> Self {
        profile.0
    }
}

#[cfg(test)]
mod tests;
