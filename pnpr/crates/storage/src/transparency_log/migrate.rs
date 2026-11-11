//! Copying `.transparency-log` records between hosted backends during storage
//! migration.

use super::{LOG_NAMESPACE, LogId, Storage, checkpoint::tree_head_of};
use crate::DocumentWrite;
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
    pub async fn store_log_record_copy(&self, key: &str, bytes: &[u8]) -> Result<()> {
        if self.root.create_record(LOG_NAMESPACE, key, bytes).await? {
            return Ok(());
        }
        if let Some(log) = log_id_of_checkpoint_key(key) {
            return self.replace_checkpoint_copy(&log, key, bytes).await;
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

    async fn replace_checkpoint_copy(&self, log: &LogId, key: &str, bytes: &[u8]) -> Result<()> {
        let source_head = tree_head_of(bytes)?;
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
            let target_head = tree_head_of(&current)?;
            check_checkpoint_copy_advance(log, &target_head, &source_head)?;
            self.verify_stored_checkpoint_head(log, &source_head, &target_head).await?;
            let replaced =
                self.root.replace_record_if_current(LOG_NAMESPACE, key, &current, bytes).await?;
            if replaced == DocumentWrite::Written {
                return Ok(());
            }
        }
    }
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
