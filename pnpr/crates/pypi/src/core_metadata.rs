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
        !matches!(self, Self::Available(false))
    }

    pub(super) fn html_value(&self) -> Option<String> {
        match self {
            Self::Available(false) => None,
            Self::Available(true) => Some("true".to_string()),
            Self::Hashes(hashes) => Some(
                hashes
                    .get_key_value("sha256")
                    .or_else(|| hashes.first_key_value())
                    .map_or_else(
                        || "true".to_string(),
                        |(algorithm, digest)| format!("{algorithm}={digest}"),
                    ),
            ),
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
