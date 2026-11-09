use serde::{Deserialize, Serialize};

/// What to do when a runtime declared through `devEngines.runtime` or
/// `engines.runtime` does not match the current process.
///
/// The `runtimeOnFail` setting overrides the manifest-level `onFail` value.
/// `download` reifies the runtime as a dependency; the other modes leave it
/// as an engine constraint only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeOnFail {
    Download,
    Error,
    Warn,
    Ignore,
}

impl RuntimeOnFail {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Download => "download",
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Ignore => "ignore",
        }
    }
}
