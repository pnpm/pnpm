use super::{Deserialize, RegistryError};
use pnpm_transparency_log::NoteSigner;
use std::{fmt, sync::Arc};

/// How pnpr logs the release records of the versions it hosts
/// ([release records](https://github.com/pnpm/rfcs/blob/92d2b281af321ecf8f1e5fea63f698716ba4e9e7/pnpr/text/0000-release-records.md)).
///
/// Every record's log entry is sequenced when its version is accepted,
/// whether or not a key is configured. With a key, pnpr integrates the
/// sequenced entries into each log's tiles and signs its checkpoints.
#[derive(Default, Clone)]
pub struct ReleaseRecordsConfig {
    /// The key that signs every log's checkpoints. Its name is the name
    /// every log's origin starts with, and each log signs under its own
    /// origin with the same Ed25519 key. `None` publishes no logs.
    pub log_signing_key: Option<Arc<NoteSigner>>,
}

impl fmt::Debug for ReleaseRecordsConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let key = self.log_signing_key
            .as_ref()
            .map(|signer| signer.verifier_key().to_string());
        formatter
            .debug_struct("ReleaseRecordsConfig")
            .field("log_signing_key", &key)
            .finish()
    }
}

/// Disk shape of the `releaseRecords:` block.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ReleaseRecordsFile {
    /// A signed-note private key, `PRIVATE+KEY+<name>+<key hash>+<key>`, as
    /// Go's `note.GenerateKey` writes one.
    #[serde(default)]
    log_signing_key: Option<String>,
}

pub(super) fn build_release_records_config(
    file: Option<ReleaseRecordsFile>,
) -> Result<ReleaseRecordsConfig, RegistryError> {
    let Some(key) = file.and_then(|file| file.log_signing_key) else {
        return Ok(ReleaseRecordsConfig::default());
    };
    let signer = NoteSigner::from_private_key(key.trim()).map_err(|err| {
        RegistryError::InvalidConfig {
            reason: format!(
                "releaseRecords.logSigningKey is not a PRIVATE+KEY+<name>+<hash>+<key> signed-note key: {err}",
            ),
        }
    })?;
    Ok(ReleaseRecordsConfig { log_signing_key: Some(Arc::new(signer)) })
}
