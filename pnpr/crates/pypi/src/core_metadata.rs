use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;

/// Availability and optional digests of a PEP 658 metadata sidecar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CoreMetadata {
    Available(bool),
    Hashes(BTreeMap<String, String>),
}

impl CoreMetadata {
    #[must_use]
    pub fn is_available(&self) -> bool {
        !matches!(self, Self::Available(false)) && self.is_supported()
    }

    #[must_use]
    pub fn digest(&self) -> Option<(&'static str, &str)> {
        let Self::Hashes(hashes) = self else { return None };
        ["sha512", "sha384", "sha256"]
            .into_iter()
            .find_map(|algorithm| {
                hashes
                    .get(algorithm)
                    .map(|digest| (algorithm, digest.as_str()))
            })
    }

    pub(super) fn is_supported(&self) -> bool {
        match self {
            Self::Available(_) => true,
            Self::Hashes(_) => self.digest().is_some(),
        }
    }

    pub(super) fn html_value(&self) -> Option<String> {
        match self {
            Self::Available(false) => None,
            Self::Available(true) => Some("true".to_string()),
            Self::Hashes(_) => self
                .digest()
                .map(|(algorithm, digest)| format!("{algorithm}={digest}")),
        }
    }
}

pub(super) fn deserialize_optional_metadata<'de, Input: Deserializer<'de>>(
    input: Input,
) -> Result<Option<CoreMetadata>, Input::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Declaration {
        Known(CoreMetadata),
        Unknown(serde::de::IgnoredAny),
    }
    Ok(match Declaration::deserialize(input)? {
        Declaration::Known(metadata) => Some(metadata),
        Declaration::Unknown(_) => None,
    })
}
