//! Copying `.transparency-log` records between hosted backends during storage
//! migration.

use super::{
    LOG_NAMESPACE, LogId, Storage, StoredCheckpoint,
    checkpoint::{tree_head_of, verify_stored_checkpoint},
};
use crate::DocumentWrite;
use pnpm_transparency_log::NoteSigner;
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::Ecosystem;

impl Storage {
    /// Every `.transparency-log` record key stored here, ordered with tiles and
    /// entry bundles before checkpoints so an interrupted copy never publishes
    /// a checkpoint ahead of its tree.
    pub async fn stored_log_record_keys(&self) -> Result<Vec<String>> {
        let mut keys = self.root.list_record_keys(LOG_NAMESPACE).await?;
        keys.sort_by(|left, right| {
            is_checkpoint_key(left)
                .cmp(&is_checkpoint_key(right))
                .then_with(|| left.cmp(right))
        });
        Ok(keys)
    }

    /// The bytes stored at `key` in `.transparency-log`, `None` when absent.
    pub async fn read_log_record(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.root.read_record(LOG_NAMESPACE, key).await
    }

    /// Store `bytes`, read from another backend under `key` in
    /// `.transparency-log`, under the same key here.
    pub async fn store_log_record_copy(
        &self,
        key: &str,
        bytes: &[u8],
        signer: Option<&NoteSigner>,
    ) -> Result<()> {
        if is_checkpoint_key(key) {
            let log = log_id_of_checkpoint_key(key)
                .ok_or_else(|| RegistryError::Internal {
                    reason: format!("the transparency-log checkpoint key '{key}' is invalid"),
                })?;
            return self.store_checkpoint_copy(&log, key, bytes, signer).await;
        }
        if self.root.create_record(LOG_NAMESPACE, key, bytes).await? {
            return Ok(());
        }
        match self.root.read_record(LOG_NAMESPACE, key).await? {
            Some(stored) if stored == bytes => Ok(()),
            _ => Err(RegistryError::Internal {
                reason: format!(
                    "the target transparency-log record '{key}' differs from the source store",
                ),
            }),
        }
    }

    async fn store_checkpoint_copy(
        &self,
        log: &LogId,
        key: &str,
        bytes: &[u8],
        signer: Option<&NoteSigner>,
    ) -> Result<()> {
        let source = parse_source_checkpoint(log, bytes, signer)?;
        self.verify_stored_root(log, source.head.size, source.head.hash).await?;
        if self.root.create_record(LOG_NAMESPACE, key, bytes).await? {
            return Ok(());
        }
        self.replace_checkpoint_copy(log, key, bytes, &source).await
    }

    async fn replace_checkpoint_copy(
        &self,
        log: &LogId,
        key: &str,
        bytes: &[u8],
        source: &SourceCheckpoint,
    ) -> Result<()> {
        loop {
            let Some(current) = self.root.read_record(LOG_NAMESPACE, key).await? else {
                if self.root.create_record(LOG_NAMESPACE, key, bytes).await? {
                    return Ok(());
                }
                continue;
            };
            if current == bytes {
                return Ok(());
            }
            self.verify_checkpoint_replacement(log, &current, source).await?;
            let replaced =
                self.root.replace_record_if_current(LOG_NAMESPACE, key, &current, bytes).await?;
            if replaced == DocumentWrite::Written {
                return Ok(());
            }
        }
    }

    async fn verify_checkpoint_replacement(
        &self,
        log: &LogId,
        current: &[u8],
        source: &SourceCheckpoint,
    ) -> Result<()> {
        if source.signer.is_none() {
            return Err(RegistryError::Internal {
                reason: format!(
                    "replacing the existing target checkpoint of the log {log} requires a configured log signing key",
                ),
            });
        }
        let target_head = tree_head_of(current)?;
        check_checkpoint_copy_advance(log, &target_head, &source.head)?;
        self.verify_stored_checkpoint_head(log, &source.head, &target_head).await
    }
}

struct SourceCheckpoint {
    head: pnpm_transparency_log::TreeHead,
    signer: Option<NoteSigner>,
}

fn parse_source_checkpoint(
    log: &LogId,
    bytes: &[u8],
    signer: Option<&NoteSigner>,
) -> Result<SourceCheckpoint> {
    require_note_signature_block(log, bytes)?;
    let head = tree_head_of(bytes)?;
    if head.size == 0 {
        return Err(RegistryError::Internal {
            reason: format!("the source checkpoint of the log {log} signs an empty tree"),
        });
    }
    let suffix = format!("/{log}");
    if head.origin.strip_suffix(&suffix).is_none_or(str::is_empty) {
        return Err(RegistryError::Internal {
            reason: format!(
                "the source checkpoint origin '{}' does not match the log {log}",
                head.origin,
            ),
        });
    }
    let log_signer = signer
        .map(|key| log.signer(key))
        .transpose()?;
    if let Some(log_signer) = log_signer.as_ref() {
        let stored = StoredCheckpoint { note: bytes.to_vec(), tree_head: head.clone() };
        verify_stored_checkpoint(log, &stored, Some(log_signer))?;
    }
    Ok(SourceCheckpoint { head, signer: log_signer })
}

fn require_note_signature_block(log: &LogId, note: &[u8]) -> Result<()> {
    let sep = note
        .windows(2)
        .position(|pair| pair == b"\n\n")
        .ok_or_else(|| RegistryError::Internal {
            reason: format!("the source checkpoint of the log {log} is malformed"),
        })?;
    if note[sep + 2..].trim_ascii().is_empty() {
        return Err(RegistryError::Internal {
            reason: format!("the source checkpoint of the log {log} carries no signature"),
        });
    }
    Ok(())
}

fn check_checkpoint_copy_advance(
    log: &LogId,
    target_head: &pnpm_transparency_log::TreeHead,
    source_head: &pnpm_transparency_log::TreeHead,
) -> Result<()> {
    if target_head.origin != source_head.origin || target_head.size > source_head.size {
        return Err(RegistryError::Internal {
            reason: format!(
                "the target checkpoint of the log {log} cannot be replaced by the source checkpoint",
            ),
        });
    }
    Ok(())
}

fn is_checkpoint_key(key: &str) -> bool {
    key == "checkpoint" || key.ends_with("/checkpoint")
}

fn log_id_of_checkpoint_key(key: &str) -> Option<LogId> {
    let prefix = key.strip_suffix("/checkpoint")?;
    let (ecosystem_str, namespace) = match prefix.split_once('/') {
        None => (prefix, ""),
        Some((eco, rest)) => {
            let ns = rest.strip_prefix('~')?;
            if ns.is_empty() || ns.contains('/') {
                return None;
            }
            (eco, ns)
        }
    };
    let ecosystem = Ecosystem::all().find(|eco| eco.as_str() == ecosystem_str)?;
    Some(LogId { namespace: namespace.to_string(), ecosystem })
}

#[cfg(test)]
mod tests;
