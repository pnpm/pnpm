//! Reconciling a SQL-sequenced transparency log against the tiles, entry
//! bundles, and checkpoint stored in `.transparency-log/`.

use super::{
    ImportedLogEntry, LOG_NAMESPACE, LogId, Storage, StoredCheckpoint, TileReader, integrated_head,
    log_error,
};
use pnpm_release_record::{LogEntry, RecordIdentity};
use pnpm_transparency_log::{
    CheckpointPolicy, Hash, KeySet, NoteSigner, TILE_WIDTH, TilePath, TreeHead, decode_bundle,
    integrate, prove_consistency, verify_consistency,
};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::Ecosystem;

struct BundledTail {
    start: u64,
    end: u64,
    entries: Vec<Vec<u8>>,
}

impl Storage {
    /// Whether `.transparency-log/` holds any stored tile, bundle, or checkpoint.
    pub async fn has_any_stored_log(&self) -> Result<bool> {
        Ok(!self.root.list_record_keys(LOG_NAMESPACE).await?.is_empty())
    }

    /// Whether `.transparency-log/` holds any stored tile, bundle, or checkpoint
    /// for `log`.
    pub async fn has_stored_log(&self, log: &LogId) -> Result<bool> {
        let namespace = format!("{LOG_NAMESPACE}/{}", log.path());
        Ok(!self.root.list_record_keys(&namespace).await?.is_empty())
    }

    /// Reconcile `log` in the metadata database against `.transparency-log/`
    /// in the hosted store: verify that stored tiles, entry bundles, and any
    /// checkpoint agree with the database's log, and re-import entries from
    /// stored entry bundles when the database was restored from an earlier
    /// snapshot than the hosted store.
    ///
    /// # Errors
    ///
    /// A tile, bundle, or checkpoint cannot be read, or `.transparency-log/`
    /// is malformed, missing entry bundles for its tree, signed by another
    /// key, or not a prefix or extension of the database's log.
    pub async fn reconcile_log(&self, log: &LogId, key: Option<&NoteSigner>) -> Result<()> {
        let signer = key
            .map(|key| log.signer(key))
            .transpose()?;
        let origin = signer
            .as_ref()
            .map(|signer| signer.verifier_key().name().to_string())
            .unwrap_or_else(|| log.path());
        let state = self.sql_metadata().log_head_and_size(log).await?;
        let head = integrated_head(log, &origin, state.head)?;
        if state.sequenced < head.size {
            return Err(RegistryError::Internal {
                reason: format!(
                    "the log {log} has {} sequenced entries, fewer than its integrated tree of {}",
                    state.sequenced, head.size,
                ),
            });
        }
        let checkpoint = self.log_checkpoint(log).await?;
        if let Some(stored) = &checkpoint {
            verify_stored_checkpoint(log, stored, signer.as_ref())?;
        }
        if head.size > 0 {
            self.verify_stored_root(log, head.size, head.hash).await?;
        }
        let tail = self.read_bundled_tail(log, head.size / u64::from(TILE_WIDTH)).await?;
        self.reconcile_tail(log, &head, state.sequenced, checkpoint.as_ref(), &tail).await
    }

    pub(super) async fn verify_stored_root(
        &self,
        log: &LogId,
        size: u64,
        expected: Hash,
    ) -> Result<()> {
        let reader = TileReader::new(self.clone(), log.clone(), size);
        tokio::task::spawn_blocking(move || {
            if size > 0 && tlog_tiles::tree_hash(size, &reader).map_err(log_error)? != expected {
                return Err(RegistryError::Internal {
                    reason:
                        "the stored tiles of a transparency log disagree with its integrated tree"
                            .to_string(),
                });
            }
            Ok(())
        })
        .await?
    }

    pub(super) async fn verify_checkpoint_consistency(
        &self,
        log: &LogId,
        older: &TreeHead,
        newer: &TreeHead,
    ) -> Result<()> {
        let reader = TileReader::new(self.clone(), log.clone(), newer.size);
        let (older, newer) = (older.clone(), newer.clone());
        let log_name = log.to_string();
        tokio::task::spawn_blocking(move || {
            let proof = prove_consistency(&reader, older.size, newer.size).map_err(log_error)?;
            verify_consistency(&proof, &older, &newer).map_err(|err| RegistryError::Internal {
                reason: format!(
                    "the stored checkpoint of the log {log_name} is not a prefix of its integrated tree: {err}",
                ),
            })
        })
        .await?
    }

    async fn read_bundled_tail(&self, log: &LogId, start_bundle: u64) -> Result<BundledTail> {
        let start = start_bundle * u64::from(TILE_WIDTH);
        let mut entries = Vec::new();
        let mut index = start_bundle;
        while let Some(bytes) =
            self.log_tile(log, TilePath::Entries { index, width: TILE_WIDTH }).await?
        {
            let path = TilePath::Entries { index, width: TILE_WIDTH };
            append_decoded_bundle(log, path, &bytes, &mut entries)?;
            index += 1;
        }
        if let Some(width) = self.max_partial_bundle_width(log, index).await? {
            let path = TilePath::Entries { index, width };
            let bytes = self
                .log_tile(log, path)
                .await?
                .ok_or_else(|| RegistryError::Internal {
                    reason: format!("the partial entry bundle {path} of the log {log} is missing"),
                })?;
            append_decoded_bundle(log, path, &bytes, &mut entries)?;
        } else if index == start_bundle && start_bundle > 0 {
            self.verify_previous_full_bundle(log, start_bundle - 1).await?;
        }
        let end = start + u64::try_from(entries.len()).unwrap_or_default();
        Ok(BundledTail { start, end, entries })
    }

    async fn max_partial_bundle_width(&self, log: &LogId, index: u64) -> Result<Option<u32>> {
        let prefix = TilePath::partial_entries_prefix(index);
        let namespace = format!("{LOG_NAMESPACE}/{}", log.key(&prefix));
        let keys = self.root.list_record_keys(&namespace).await?;
        let max_width = keys
            .into_iter()
            .filter_map(|name| format!("{prefix}/{name}").parse::<TilePath>().ok())
            .filter_map(|path| match path {
                TilePath::Entries { index: found, width }
                    if found == index && width < TILE_WIDTH =>
                {
                    Some(width)
                }
                TilePath::Hashes { .. } | TilePath::Entries { .. } => None,
            })
            .max();
        Ok(max_width)
    }

    async fn verify_previous_full_bundle(&self, log: &LogId, index: u64) -> Result<()> {
        let path = TilePath::Entries { index, width: TILE_WIDTH };
        let bytes = self
            .log_tile(log, path)
            .await?
            .ok_or_else(|| RegistryError::Internal {
                reason: format!("the stored entry bundle {path} of the log {log} is missing"),
            })?;
        let mut decoded = Vec::new();
        append_decoded_bundle(log, path, &bytes, &mut decoded)
    }

    async fn reconcile_tail(
        &self,
        log: &LogId,
        head: &TreeHead,
        sequenced: u64,
        checkpoint: Option<&StoredCheckpoint>,
        tail: &BundledTail,
    ) -> Result<()> {
        check_bundle_bounds(log, head.size, checkpoint, tail.end)?;
        self.verify_bundled_overlap(log, sequenced, tail).await?;
        if let Some(stored) = checkpoint {
            self.verify_stored_checkpoint_against_tail(log, head, &stored.tree_head, tail).await?;
        }
        if tail.end <= sequenced {
            return Ok(());
        }
        let offset = usize::try_from(sequenced - tail.start).unwrap_or_default();
        let imported = parse_imported_entries(log, sequenced, &tail.entries[offset..])?;
        self.sql_metadata().import_log_entries(log, sequenced, imported).await
    }

    async fn verify_bundled_overlap(
        &self,
        log: &LogId,
        sequenced: u64,
        tail: &BundledTail,
    ) -> Result<()> {
        let overlap_end = sequenced.min(tail.end);
        if overlap_end <= tail.start {
            return Ok(());
        }
        let existing = self
            .sql_metadata()
            .log_entries(log, tail.start..overlap_end)
            .await?;
        for (offset, existing_bytes) in existing.iter().enumerate() {
            if existing_bytes.as_slice() != tail.entries[offset].as_slice() {
                let index = tail.start + u64::try_from(offset).unwrap_or_default();
                return Err(RegistryError::Internal {
                    reason: format!(
                        "the stored entry bundle of the log {log} disagrees with sequenced entry {index}",
                    ),
                });
            }
        }
        Ok(())
    }

    async fn verify_stored_checkpoint_against_tail(
        &self,
        log: &LogId,
        head: &TreeHead,
        checkpoint: &TreeHead,
        tail: &BundledTail,
    ) -> Result<()> {
        if checkpoint.size < head.size {
            self.verify_stored_root(log, checkpoint.size, checkpoint.hash).await?;
            let newer =
                TreeHead { origin: checkpoint.origin.clone(), size: head.size, hash: head.hash };
            return self.verify_checkpoint_consistency(log, checkpoint, &newer).await;
        }
        if checkpoint.size == head.size {
            if checkpoint.hash != head.hash {
                return Err(RegistryError::Internal {
                    reason: format!(
                        "the stored checkpoint of the log {log} disagrees with its integrated tree",
                    ),
                });
            }
            return Ok(());
        }
        self.verify_checkpoint_extension(log, head.size, checkpoint, tail).await
    }

    async fn verify_checkpoint_extension(
        &self,
        log: &LogId,
        head_size: u64,
        checkpoint: &TreeHead,
        tail: &BundledTail,
    ) -> Result<()> {
        let start = usize::try_from(head_size - tail.start).unwrap_or_default();
        let end = usize::try_from(checkpoint.size - tail.start).unwrap_or_default();
        let extension = tail.entries[start..end].to_vec();
        let reader = TileReader::new(self.clone(), log.clone(), head_size);
        let expected = checkpoint.hash;
        let log_name = log.to_string();
        tokio::task::spawn_blocking(move || {
            let integration = integrate(&reader, head_size, &extension).map_err(log_error)?;
            if integration.root != expected {
                return Err(RegistryError::Internal {
                    reason: format!(
                        "the stored checkpoint of the log {log_name} is not an extension of its integrated tree",
                    ),
                });
            }
            Ok(())
        })
        .await??;
        self.verify_stored_root(log, checkpoint.size, checkpoint.hash).await
    }
}

pub(super) fn verify_stored_checkpoint(
    log: &LogId,
    stored: &StoredCheckpoint,
    signer: Option<&NoteSigner>,
) -> Result<()> {
    if stored.tree_head.size == 0 {
        return Err(RegistryError::Internal {
            reason: format!("the stored checkpoint of the log {log} signs an empty tree"),
        });
    }
    if let Some(signer) = signer {
        let origin = signer.verifier_key().name();
        let log_keys = KeySet::new(vec![signer.verifier_key().clone()], 1).map_err(log_error)?;
        let policy = CheckpointPolicy::new(origin, log_keys, KeySet::empty()).map_err(log_error)?;
        policy
            .verify(&stored.note)
            .map_err(|err| RegistryError::Internal {
                reason: format!(
                    "the stored checkpoint of the log {log} failed verification: {err}",
                ),
            })?;
        return Ok(());
    }
    let valid_origin = stored.tree_head.origin
        .strip_suffix(&format!("/{}", log.path()))
        .is_some_and(|key_name| !key_name.is_empty());
    if !valid_origin {
        return Err(RegistryError::Internal {
            reason: format!(
                "the stored checkpoint of the log {log} has origin {:?}, which does not match this log",
                stored.tree_head.origin,
            ),
        });
    }
    Ok(())
}

fn check_bundle_bounds(
    log: &LogId,
    head_size: u64,
    checkpoint: Option<&StoredCheckpoint>,
    bundled_end: u64,
) -> Result<()> {
    if bundled_end < head_size {
        return Err(RegistryError::Internal {
            reason: format!(
                "the stored entry bundles of the log {log} cover {bundled_end} entries, fewer than its integrated tree of {head_size}",
            ),
        });
    }
    if let Some(stored) = checkpoint
        && stored.tree_head.size > bundled_end
    {
        return Err(RegistryError::Internal {
            reason: format!(
                "the stored checkpoint of the log {log} covers {} entries, more than its stored entry bundles ({bundled_end})",
                stored.tree_head.size,
            ),
        });
    }
    Ok(())
}

fn append_decoded_bundle(
    log: &LogId,
    path: TilePath,
    bytes: &[u8],
    out: &mut Vec<Vec<u8>>,
) -> Result<()> {
    let expected = match path {
        TilePath::Entries { width, .. } => usize::try_from(width).unwrap_or_default(),
        TilePath::Hashes { .. } => 0,
    };
    let decoded = decode_bundle(bytes)
        .map_err(|err| RegistryError::Internal {
            reason: format!("the stored entry bundle {path} of the log {log} is malformed: {err}"),
        })?;
    if decoded.len() != expected {
        return Err(RegistryError::Internal {
            reason: format!(
                "the stored entry bundle {path} of the log {log} holds {} entries, expected {expected}",
                decoded.len(),
            ),
        });
    }
    out.extend(decoded.into_iter().map(<[u8]>::to_vec));
    Ok(())
}

fn parse_imported_entries(
    log: &LogId,
    start: u64,
    raw_entries: &[Vec<u8>],
) -> Result<Vec<ImportedLogEntry>> {
    raw_entries
        .iter()
        .enumerate()
        .map(|(offset, bytes)| {
            let index = start + u64::try_from(offset).unwrap_or_default();
            parse_one_imported_entry(log, index, bytes)
        })
        .collect()
}

fn parse_one_imported_entry(log: &LogId, index: u64, bytes: &[u8]) -> Result<ImportedLogEntry> {
    let entry = LogEntry::parse(bytes)
        .map_err(|err| RegistryError::Internal {
            reason: format!(
                "entry {index} in the stored bundle of the log {log} is invalid: {err}",
            ),
        })?;
    let key = entry.key();
    if entry_ecosystem(&key.identity) != Some(log.ecosystem) {
        return Err(RegistryError::Internal {
            reason: format!(
                "entry {index} in the stored bundle of the log {log} describes {}, outside {}",
                key.subject_name(),
                log.ecosystem,
            ),
        });
    }
    let npm_version = match (&key.identity, key.revision) {
        (RecordIdentity::Package(purl), Some(0)) if log.ecosystem == Ecosystem::Npm => {
            purl.npm_package()
        }
        _ => None,
    };
    Ok(ImportedLogEntry { bytes: bytes.to_vec(), npm_version })
}

fn entry_ecosystem(identity: &RecordIdentity) -> Option<Ecosystem> {
    match identity {
        RecordIdentity::Package(purl) => match purl.purl_type() {
            "npm" => Some(Ecosystem::Npm),
            "cargo" => Some(Ecosystem::Cargo),
            "pypi" => Some(Ecosystem::Pypi),
            _ => None,
        },
        RecordIdentity::OciTag { .. } => Some(Ecosystem::Oci),
    }
}
