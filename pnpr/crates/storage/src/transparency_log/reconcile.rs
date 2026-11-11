//! Reconciling a SQL-sequenced transparency log against the tiles, entry
//! bundles, and checkpoint stored in `.transparency-log/`.

mod bundles;

use super::{
    LOG_NAMESPACE, LogId, Storage, StoredCheckpoint, TileReader, check_sequenced_at_least_head,
    checkpoint::verify_stored_checkpoint, integrated_head, log_error,
};
use bundles::{
    check_authenticated_bound, check_bundle_bounds, check_prior_full_bundle_keys,
    decode_bundle_exact, max_partial_width_of_keys, parse_imported_entries,
};
use pnpm_transparency_log::{
    Hash, NoteSigner, TILE_WIDTH, TilePath, TreeHead, integrate, prove_consistency,
    verify_consistency,
};
use pnpr_error::{RegistryError, Result};

struct ReconcileTarget<'signer> {
    head: TreeHead,
    sequenced: u64,
    checkpoint: Option<StoredCheckpoint>,
    signer: Option<&'signer NoteSigner>,
}

impl ReconcileTarget<'_> {
    fn authenticated_end(&self) -> u64 {
        let checkpoint_size =
            self.checkpoint.as_ref().map_or(0, |stored| stored.tree_head.size);
        self.sequenced.max(checkpoint_size)
    }
}

impl Storage {
    /// Whether `.transparency-log/` holds any stored tile, bundle, or checkpoint.
    pub async fn has_any_stored_log(&self) -> Result<bool> {
        self.root.has_any_record(LOG_NAMESPACE).await
    }

    /// Whether `.transparency-log/` holds any stored tile, bundle, or checkpoint
    /// for `log`.
    pub async fn has_stored_log(&self, log: &LogId) -> Result<bool> {
        let namespace = format!("{LOG_NAMESPACE}/{}", log.path());
        self.root.has_any_record(&namespace).await
    }

    /// Reconcile `log` in the metadata database against `.transparency-log/`
    /// in the hosted store: verify that stored tiles, entry bundles, and any
    /// checkpoint agree with the database's log, re-import checkpoint-signed
    /// entries from stored entry bundles when the database was restored from an
    /// earlier snapshot than the hosted store, and return the verified stored
    /// checkpoint when one exists.
    ///
    /// # Errors
    ///
    /// A tile, bundle, or checkpoint cannot be read, or `.transparency-log/`
    /// is malformed, missing entry bundles for its tree, contains uncheckpointed
    /// entries beyond the database, lacks a signing key for a stored checkpoint,
    /// is signed by another key, or is not a prefix or extension of the
    /// database's log.
    pub async fn reconcile_log(
        &self,
        log: &LogId,
        key: Option<&NoteSigner>,
    ) -> Result<Option<StoredCheckpoint>> {
        let signer = key
            .map(|key| log.signer(key))
            .transpose()?;
        let origin = signer
            .as_ref()
            .map(|signer| signer.verifier_key().name().to_string())
            .unwrap_or_else(|| log.path());
        let checkpoint = self.log_checkpoint(log).await?;
        if let Some(stored) = &checkpoint {
            verify_stored_checkpoint(log, stored, signer.as_ref())?;
        }
        let state = self.sql_metadata().log_head_and_size(log).await?;
        let head = integrated_head(log, &origin, state.head)?;
        check_sequenced_at_least_head(log, state.sequenced, head.size)?;
        let mut target = ReconcileTarget {
            head,
            sequenced: state.sequenced,
            checkpoint,
            signer: signer.as_ref(),
        };
        self.reconcile_against_target(log, &mut target).await?;
        Ok(target.checkpoint)
    }

    async fn reconcile_against_target(
        &self,
        log: &LogId,
        target: &mut ReconcileTarget<'_>,
    ) -> Result<()> {
        if target.head.size > 0 {
            self.verify_stored_root(log, target.head.size, target.head.hash).await?;
        }
        if let Some(stored) = &target.checkpoint {
            self.verify_stored_checkpoint_head(log, &target.head, &stored.tree_head).await?;
        }
        let start_bundle = target.head.size / u64::from(TILE_WIDTH);
        self.verify_prior_full_bundles(log, start_bundle).await?;
        let bundled_end = self.stream_tail_bundles(log, target, start_bundle).await?;
        check_bundle_bounds(log, target.head.size, target.checkpoint.as_ref(), bundled_end)
    }

    async fn verify_stored_checkpoint_head(
        &self,
        log: &LogId,
        head: &TreeHead,
        checkpoint: &TreeHead,
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
        self.verify_stored_root(log, checkpoint.size, checkpoint.hash).await?;
        if head.size > 0 {
            let older =
                TreeHead { origin: checkpoint.origin.clone(), size: head.size, hash: head.hash };
            self.verify_checkpoint_consistency(log, &older, checkpoint).await?;
        }
        Ok(())
    }

    async fn verify_prior_full_bundles(&self, log: &LogId, start_bundle: u64) -> Result<()> {
        if start_bundle == 0 {
            return Ok(());
        }
        let namespace = format!("{LOG_NAMESPACE}/{}", log.key("tile/entries"));
        let keys = self.root.list_record_keys(&namespace).await?;
        check_prior_full_bundle_keys(log, start_bundle, keys)
    }

    async fn stream_tail_bundles(
        &self,
        log: &LogId,
        target: &mut ReconcileTarget<'_>,
        start_bundle: u64,
    ) -> Result<u64> {
        let mut running_head = target.head.clone();
        let mut index = start_bundle;
        loop {
            let path = TilePath::Entries { index, width: TILE_WIDTH };
            let Some(bytes) = self.log_tile(log, path).await? else { break };
            let bundle_start = index * u64::from(TILE_WIDTH);
            let bundle_end = bundle_start + u64::from(TILE_WIDTH);
            self.ensure_bundle_authenticated(log, target, &running_head, path, bundle_end).await?;
            let entries = decode_bundle_exact(log, path, &bytes)?;
            self.reconcile_one_bundle(log, target, &mut running_head, bundle_start, &entries)
                .await?;
            index += 1;
        }
        self.stream_partial_tail_bundle(log, target, &mut running_head, index).await
    }

    async fn stream_partial_tail_bundle(
        &self,
        log: &LogId,
        target: &mut ReconcileTarget<'_>,
        running_head: &mut TreeHead,
        index: u64,
    ) -> Result<u64> {
        let bundle_start = index * u64::from(TILE_WIDTH);
        let Some(width) = self.max_partial_bundle_width(log, index).await? else {
            return Ok(bundle_start);
        };
        let path = TilePath::Entries { index, width };
        let bundle_end = bundle_start + u64::from(width);
        self.ensure_bundle_authenticated(log, target, running_head, path, bundle_end).await?;
        let bytes = self
            .log_tile(log, path)
            .await?
            .ok_or_else(|| RegistryError::Internal {
                reason: format!("the partial entry bundle {path} of the log {log} is missing"),
            })?;
        let entries = decode_bundle_exact(log, path, &bytes)?;
        self.reconcile_one_bundle(log, target, running_head, bundle_start, &entries).await?;
        Ok(bundle_end)
    }

    async fn ensure_bundle_authenticated(
        &self,
        log: &LogId,
        target: &mut ReconcileTarget<'_>,
        running_head: &TreeHead,
        path: TilePath,
        bundle_end: u64,
    ) -> Result<()> {
        if bundle_end > target.authenticated_end() {
            let sequenced = self.sql_metadata().log_size(log).await?;
            check_sequenced_at_least_head(log, sequenced, target.head.size)?;
            target.sequenced = target.sequenced.max(sequenced);
            let checkpoint = self.log_checkpoint(log).await?;
            self.refresh_target_checkpoint(log, target, running_head, checkpoint).await?;
        }
        check_authenticated_bound(log, path, bundle_end, target.authenticated_end())
    }

    async fn refresh_target_checkpoint(
        &self,
        log: &LogId,
        target: &mut ReconcileTarget<'_>,
        running_head: &TreeHead,
        checkpoint: Option<StoredCheckpoint>,
    ) -> Result<()> {
        let Some(stored) = checkpoint else {
            if target.checkpoint.is_some() {
                return Err(RegistryError::Internal {
                    reason: format!(
                        "the stored checkpoint of the log {log} disappeared during reconciliation",
                    ),
                });
            }
            return Ok(());
        };
        if target.checkpoint.as_ref() == Some(&stored) {
            return Ok(());
        }
        verify_stored_checkpoint(log, &stored, target.signer)?;
        if let Some(prior) = &target.checkpoint {
            if stored.tree_head.size < prior.tree_head.size {
                return Err(RegistryError::Internal {
                    reason: format!(
                        "the stored checkpoint of the log {log} regressed from {} to {} entries",
                        prior.tree_head.size, stored.tree_head.size,
                    ),
                });
            }
            self.verify_stored_checkpoint_head(log, &prior.tree_head, &stored.tree_head).await?;
        } else {
            self.verify_stored_checkpoint_head(log, running_head, &stored.tree_head).await?;
        }
        target.checkpoint = Some(stored);
        Ok(())
    }

    async fn reconcile_one_bundle(
        &self,
        log: &LogId,
        target: &mut ReconcileTarget<'_>,
        running_head: &mut TreeHead,
        bundle_start: u64,
        entries: &[Vec<u8>],
    ) -> Result<()> {
        let bundle_end = bundle_start + u64::try_from(entries.len()).unwrap_or_default();
        self.verify_bundle_overlap(log, target.sequenced, bundle_start, entries).await?;
        if let Some(stored) = &target.checkpoint
            && stored.tree_head.size > running_head.size
            && bundle_end > running_head.size
        {
            let ext_start = usize::try_from(running_head.size - bundle_start).unwrap_or_default();
            let ext_end = usize::try_from(bundle_end.min(stored.tree_head.size) - bundle_start)
                .unwrap_or_default();
            *running_head = self.verify_bundle_step(
                log,
                running_head.size,
                &entries[ext_start..ext_end],
                &stored.tree_head,
            )
            .await?;
        }
        if bundle_end <= target.sequenced {
            return Ok(());
        }
        let import_start = target.sequenced.max(bundle_start);
        let offset = usize::try_from(import_start - bundle_start).unwrap_or_default();
        let imported = parse_imported_entries(log, import_start, &entries[offset..])?;
        self.sql_metadata().import_log_entries(log, import_start, imported).await?;
        target.sequenced = bundle_end;
        Ok(())
    }

    async fn verify_bundle_overlap(
        &self,
        log: &LogId,
        sequenced: u64,
        bundle_start: u64,
        entries: &[Vec<u8>],
    ) -> Result<()> {
        let bundle_end = bundle_start + u64::try_from(entries.len()).unwrap_or_default();
        let overlap_end = sequenced.min(bundle_end);
        if overlap_end <= bundle_start {
            return Ok(());
        }
        let existing = self
            .sql_metadata()
            .log_entries(log, bundle_start..overlap_end)
            .await?;
        for (offset, existing_bytes) in existing.iter().enumerate() {
            if existing_bytes.as_slice() != entries[offset].as_slice() {
                let index = bundle_start + u64::try_from(offset).unwrap_or_default();
                return Err(RegistryError::Internal {
                    reason: format!(
                        "the stored entry bundle of the log {log} disagrees with sequenced entry {index}",
                    ),
                });
            }
        }
        Ok(())
    }

    async fn verify_bundle_step(
        &self,
        log: &LogId,
        current_size: u64,
        extension: &[Vec<u8>],
        checkpoint: &TreeHead,
    ) -> Result<TreeHead> {
        let reader = TileReader::new(self.clone(), log.clone(), checkpoint.size);
        let extension = extension.to_vec();
        let checkpoint = checkpoint.clone();
        let log_name = log.to_string();
        tokio::task::spawn_blocking(move || {
            let integration = integrate(&reader, current_size, &extension).map_err(log_error)?;
            let next_head = TreeHead {
                origin: checkpoint.origin.clone(),
                size: integration.size,
                hash: integration.root,
            };
            if next_head.size == checkpoint.size {
                if next_head.hash != checkpoint.hash {
                    return Err(RegistryError::Internal {
                        reason: format!(
                            "the stored checkpoint of the log {log_name} is not an extension of its integrated tree",
                        ),
                    });
                }
                return Ok(next_head);
            }
            let proof =
                prove_consistency(&reader, next_head.size, checkpoint.size).map_err(log_error)?;
            verify_consistency(&proof, &next_head, &checkpoint).map_err(|err| {
                RegistryError::Internal {
                    reason: format!(
                        "the stored entry bundle of the log {log_name} is not a prefix of its checkpoint: {err}",
                    ),
                }
            })?;
            Ok(next_head)
        })
        .await?
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

    async fn max_partial_bundle_width(&self, log: &LogId, index: u64) -> Result<Option<u32>> {
        let prefix = TilePath::partial_entries_prefix(index);
        let namespace = format!("{LOG_NAMESPACE}/{}", log.key(&prefix));
        let keys = self.root.list_record_keys(&namespace).await?;
        Ok(max_partial_width_of_keys(index, &prefix, keys))
    }
}
