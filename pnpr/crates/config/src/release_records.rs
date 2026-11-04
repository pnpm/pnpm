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
    /// The key that signs the registry's statements about its records, such
    /// as that it accepted a record. Its `<name>+<key hash>` is each DSSE
    /// signature's `keyid`. `None` signs no statements.
    pub statement_signing_key: Option<Arc<NoteSigner>>,
}

impl fmt::Debug for ReleaseRecordsConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let key = self.log_signing_key
            .as_ref()
            .map(|signer| signer.verifier_key().to_string());
        let statement_key = self.statement_signing_key
            .as_ref()
            .map(|signer| signer.verifier_key().to_string());
        formatter
            .debug_struct("ReleaseRecordsConfig")
            .field("log_signing_key", &key)
            .field("statement_signing_key", &statement_key)
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
    /// A signed-note private key, as for `log_signing_key`.
    #[serde(default)]
    statement_signing_key: Option<String>,
}

pub(super) fn build_release_records_config(
    file: Option<ReleaseRecordsFile>,
) -> Result<ReleaseRecordsConfig, RegistryError> {
    let file = file.unwrap_or_default();
    Ok(ReleaseRecordsConfig {
        log_signing_key: signer("logSigningKey", file.log_signing_key)?,
        statement_signing_key: signer("statementSigningKey", file.statement_signing_key)?,
    })
}

fn signer(field: &str, key: Option<String>) -> Result<Option<Arc<NoteSigner>>, RegistryError> {
    let Some(key) = key else { return Ok(None) };
    let signer = NoteSigner::from_private_key(key.trim()).map_err(|err| {
        RegistryError::InvalidConfig {
            reason: format!(
                "releaseRecords.{field} is not a PRIVATE+KEY+<name>+<hash>+<key> signed-note key: {err}",
            ),
        }
    })?;
    Ok(Some(Arc::new(signer)))
}
