//! The transparency logs of the release records a hosted namespace accepts
//! ([release records](https://github.com/pnpm/rfcs/blob/92d2b281af321ecf8f1e5fea63f698716ba4e9e7/pnpr/text/0000-release-records.md#transparency-log)),
//! one per namespace and ecosystem.
//!
//! A record's log entry is sequenced in the SQL metadata store, in the
//! transaction that accepts its version. Integration runs after commit: it
//! reads the entries the tree it last integrated does not cover yet, checks
//! the stored tiles against that tree's root, writes the
//! [tlog-tiles](https://c2sp.org/tlog-tiles) hash tiles and entry bundles the
//! entries add, records the larger tree in the SQL store, and then signs and
//! writes a checkpoint of it. Tiles and
//! checkpoints are kept in the hosted store, beside the archives, so a log
//! needs no database to be served.
//!
//! Every tile path is written once: a partial tile that grows is published
//! again under its new width, and a full tile never changes. Integration is
//! idempotent, so integrators of one log, in this process or another, may run
//! at once: they write the same bytes to the same tiles, and the checkpoint
//! only moves to a larger tree.

pub use checkpoint::StoredCheckpoint;

mod checkpoint;
mod migrate;
mod reconcile;

use crate::Storage;
use checkpoint::CheckpointStatus;
use pnpm_transparency_log::{
    BundleRange, Hash, HashReader, NoteSigner, TILE_HEIGHT, TilePath, TreeHead, encode_bundle,
    integrate, prove_inclusion,
};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::Ecosystem;
use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Mutex, PoisonError},
};
use tlog_tiles::Tile;

/// The hosted-store record directory of the logs' tiles and checkpoints. A
/// leading `.` keeps it apart from every namespace and package name.
pub const LOG_NAMESPACE: &str = ".transparency-log";

/// The most entries one round of integration reads.
const INTEGRATION_BATCH: u64 = 4096;

/// One transparency log: the records of one hosted namespace and ecosystem.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LogId {
    /// The hosted namespace; empty for the default one.
    pub namespace: String,
    pub ecosystem: Ecosystem,
}

impl LogId {
    /// The log's path in the store and in its origin: its ecosystem, and for
    /// a namespace other than the default one, `~<namespace>`.
    fn path(&self) -> String {
        if self.namespace.is_empty() {
            self.ecosystem.as_str().to_string()
        } else {
            format!("{}/~{}", self.ecosystem.as_str(), self.namespace)
        }
    }

    /// The log's origin, the name its checkpoints carry and its checkpoint
    /// key is named after: `<name>/<path>`, where `name` is the name of the
    /// registry's log signing key.
    #[must_use]
    pub fn origin(&self, name: &str) -> String {
        format!("{name}/{}", self.path())
    }

    /// The signer of this log's checkpoints: `key` under the log's origin.
    pub fn signer(&self, key: &NoteSigner) -> Result<NoteSigner> {
        key.renamed(&self.origin(key.verifier_key().name()))
            .map_err(|err| RegistryError::InvalidConfig {
                reason: format!("the log signing key cannot sign the log {self}: {err}"),
            })
    }

    fn key(&self, resource: &str) -> String {
        format!("{}/{resource}", self.path())
    }
}

impl fmt::Display for LogId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.path())
    }
}

/// An entry's inclusion proof against a stored checkpoint.
#[derive(Debug, Clone)]
pub struct InclusionProof {
    pub checkpoint: StoredCheckpoint,
    pub index: u64,
    pub proof: Vec<Hash>,
}

/// A log entry re-imported from a stored entry bundle, and the npm package
/// version it permanently claims when it is revision 0 of an npm package.
#[derive(Debug, Clone)]
pub(crate) struct ImportedLogEntry {
    pub(crate) bytes: Vec<u8>,
    pub(crate) npm_version: Option<(String, String)>,
}

/// A log's integrated head (`(size, root)`) and sequenced entry count in SQL.
#[derive(Debug, Clone)]
pub(crate) struct RawLogState {
    pub(crate) head: Option<(u64, Vec<u8>)>,
    pub(crate) sequenced: u64,
}

impl Storage {
    /// The tile or entry bundle at `path` of `log`, once integration wrote
    /// it.
    pub async fn log_tile(&self, log: &LogId, path: TilePath) -> Result<Option<Vec<u8>>> {
        self.root.read_record(LOG_NAMESPACE, &log.key(&path.to_string())).await
    }

    /// Integrate every entry of `log` sequenced so far that its integrated
    /// tree does not cover, sign a checkpoint of the tree they make with
    /// `key` under the log's origin, and return its tree head.
    ///
    /// A log with no sequenced entry signs no checkpoint: its tree head is the
    /// empty tree's, and its checkpoint stays `None`.
    ///
    /// The tree a log was integrated to is recorded in the SQL metadata store,
    /// and the stored tiles must hash to its root before integration extends
    /// it: tiles altered in the hosted store are refused, never signed over.
    ///
    /// # Errors
    ///
    /// An entry or tile cannot be read or written, or a stored tile disagrees with the tree the store recorded
    /// or with the one integration computes.
    pub async fn integrate_log(&self, log: &LogId, key: &NoteSigner) -> Result<TreeHead> {
        let verified = self.reconcile_log(log, Some(key)).await?;
        let (head, _) = self.integrate_reconciled_log(log, key, verified).await?;
        Ok(head)
    }

    /// Integrate every entry of `log` sequenced after its integrated tree, once
    /// [`Self::reconcile_log`] has already reconciled `log` and observed
    /// `verified_checkpoint`.
    pub async fn integrate_reconciled_log(
        &self,
        log: &LogId,
        key: &NoteSigner,
        mut verified_checkpoint: Option<StoredCheckpoint>,
    ) -> Result<(TreeHead, Option<StoredCheckpoint>)> {
        let signer = log.signer(key)?;
        let origin = signer.verifier_key().name().to_string();
        let database = self.sql_database();
        loop {
            let state = database.log_head_and_size(log).await?;
            let sequenced = state.sequenced;
            let head = integrated_head(log, &origin, state.head)?;
            check_sequenced_at_least_head(log, sequenced, head.size)?;
            if sequenced == head.size {
                match self.publish_checkpoint(log, None, &head, &signer).await? {
                    CheckpointStatus::Published(stored) => return Ok((head, stored)),
                    CheckpointStatus::BehindStored(stored) => {
                        verified_checkpoint = self.reconcile_behind_stored(
                            log,
                            key,
                            sequenced,
                            verified_checkpoint,
                            stored,
                        )
                        .await?;
                        continue;
                    }
                }
            }
            let end = sequenced.min(head.size + INTEGRATION_BATCH);
            let entries = database.log_entries(log, head.size..end).await?;
            let tree_head = self.integrate_batch(log, &head, &entries).await?;
            database.advance_log_head(log, tree_head.size, tree_head.hash.0).await?;
            if let CheckpointStatus::BehindStored(stored) =
                self.publish_checkpoint(log, Some(&head), &tree_head, &signer).await?
            {
                verified_checkpoint =
                    self.reconcile_behind_stored(log, key, sequenced, verified_checkpoint, stored)
                        .await?;
            }
        }
    }

    async fn reconcile_behind_stored(
        &self,
        log: &LogId,
        key: &NoteSigner,
        sequenced: u64,
        verified: Option<StoredCheckpoint>,
        stored: TreeHead,
    ) -> Result<Option<StoredCheckpoint>> {
        if verified
            .as_ref()
            .is_some_and(|checkpoint| checkpoint.tree_head == stored)
            && sequenced >= stored.size
        {
            return Ok(verified);
        }
        self.catch_up_with_stored_checkpoint(log, key, stored.size).await
    }

    async fn catch_up_with_stored_checkpoint(
        &self,
        log: &LogId,
        key: &NoteSigner,
        stored_size: u64,
    ) -> Result<Option<StoredCheckpoint>> {
        let verified = self.reconcile_log(log, Some(key)).await?;
        let sequenced = self.sql_database().log_size(log).await?;
        if sequenced < stored_size {
            return Err(RegistryError::Internal {
                reason: format!(
                    "the stored checkpoint of the log {log} covers {stored_size} entries, more than its {sequenced} sequenced entries",
                ),
            });
        }
        Ok(verified)
    }

    /// Write the entry bundles and hash tiles that `entries`, sequenced after
    /// the tree `head` describes, add to it, and return the new tree head.
    /// Entry bundles are persisted before hash tiles so a store snapshot never
    /// holds a tile without the entries that hash to it.
    async fn integrate_batch(
        &self,
        log: &LogId,
        head: &TreeHead,
        entries: &[Vec<u8>],
    ) -> Result<TreeHead> {
        let size = head.size;
        let expected = head.hash;
        let reader = TileReader::new(self.clone(), log.clone(), size);
        let entries_owned = entries.to_vec();
        let (integration, tiles) = tokio::task::spawn_blocking(move || {
            if size > 0 && tlog_tiles::tree_hash(size, &reader).map_err(log_error)? != expected {
                return Err(RegistryError::Internal {
                    reason:
                        "the stored tiles of a transparency log disagree with its integrated tree"
                            .to_string(),
                });
            }
            let integration = integrate(&reader, size, &entries_owned).map_err(log_error)?;
            let tiles = integration.tiles(&reader).map_err(log_error)?;
            Ok::<_, RegistryError>((integration, tiles))
        })
        .await??;
        let bundles = self.entry_bundles(log, size, entries, &integration.entry_bundles()).await?;
        for (path, bytes) in bundles.into_iter().chain(tiles) {
            self.write_tile(log, path, &bytes).await?;
        }
        Ok(TreeHead { origin: head.origin.clone(), size: integration.size, hash: integration.root })
    }

    /// The encoded entry bundles `ranges` name. A partial bundle that gained
    /// entries holds the ones it had before, which are read back.
    async fn entry_bundles(
        &self,
        log: &LogId,
        size: u64,
        new: &[Vec<u8>],
        ranges: &[BundleRange],
    ) -> Result<Vec<(TilePath, Vec<u8>)>> {
        let database = self.sql_database();
        let mut bundles = Vec::with_capacity(ranges.len());
        for range in ranges {
            let mut entries = Vec::new();
            if range.entries.start < size {
                entries = database.log_entries(log, range.entries.start..size).await?;
            }
            let first_new =
                usize::try_from(range.entries.start.max(size) - size).unwrap_or_default();
            let last_new = usize::try_from(range.entries.end - size).unwrap_or_default();
            entries.extend_from_slice(&new[first_new..last_new]);
            let bundle = encode_bundle(&entries)
                .map_err(|err| RegistryError::Internal {
                    reason: format!("an entry bundle of the log {log} cannot be encoded: {err}"),
                })?;
            bundles.push((range.path, bundle));
        }
        Ok(bundles)
    }

    /// Write a tile, which never changes once written: a tile already there
    /// must hold the same bytes.
    async fn write_tile(&self, log: &LogId, path: TilePath, bytes: &[u8]) -> Result<()> {
        let key = log.key(&path.to_string());
        if self.root.create_record(LOG_NAMESPACE, &key, bytes).await? {
            return Ok(());
        }
        match self.root.read_record(LOG_NAMESPACE, &key).await? {
            Some(stored) if stored == bytes => Ok(()),
            _ => Err(RegistryError::Internal {
                reason: format!("the stored tile {path} of the log {log} differs from its entries"),
            }),
        }
    }

    /// The inclusion proof of entry `index` of `log` against its latest
    /// checkpoint, `None` while no checkpoint covers the entry yet.
    pub async fn log_inclusion(&self, log: &LogId, index: u64) -> Result<Option<InclusionProof>> {
        let Some(checkpoint) = self.log_checkpoint(log).await? else { return Ok(None) };
        let size = checkpoint.tree_head.size;
        if index >= size {
            return Ok(None);
        }
        let reader = TileReader::new(self.clone(), log.clone(), size);
        let proof = tokio::task::spawn_blocking(move || {
            prove_inclusion(&reader, size, index).map_err(log_error)
        })
        .await??;
        Ok(Some(InclusionProof { checkpoint, index, proof }))
    }
}

/// Reads stored hashes from the hash tiles of a tree of `size` entries,
/// fetching each tile once. Blocking: it runs on the blocking pool and waits
/// for the store on the runtime it was made on.
struct TileReader {
    storage: Storage,
    log: LogId,
    size: u64,
    runtime: tokio::runtime::Handle,
    tiles: Mutex<HashMap<TilePath, Arc<Vec<u8>>>>,
}

impl TileReader {
    fn new(storage: Storage, log: LogId, size: u64) -> Self {
        let runtime = tokio::runtime::Handle::current();
        Self { storage, log, size, runtime, tiles: Mutex::new(HashMap::new()) }
    }

    fn tile_data(&self, tile: Tile) -> std::result::Result<Arc<Vec<u8>>, tlog_tiles::Error> {
        let path = TilePath::of_tile(&tile).ok_or(tlog_tiles::Error::InvalidTile)?;
        if let Some(data) = self.tiles
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&path)
        {
            return Ok(Arc::clone(data));
        }
        let read = self.runtime.block_on(self.storage.log_tile(&self.log, path));
        let data = match read {
            Ok(Some(data)) => Arc::new(data),
            Ok(None) => {
                return Err(tlog_tiles::Error::InvalidInput(format!("missing tile {path}")));
            }
            Err(err) => return Err(tlog_tiles::Error::InvalidInput(err.to_string())),
        };
        self.tiles
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(path, Arc::clone(&data));
        Ok(data)
    }
}

impl HashReader for TileReader {
    fn read_hashes(&self, indexes: &[u64]) -> std::result::Result<Vec<Hash>, tlog_tiles::Error> {
        indexes
            .iter()
            .map(|&index| {
                // The tile the tree of `size` entries stores the hash in.
                let tile = Tile::from_index(TILE_HEIGHT, index)
                    .parent(0, self.size)
                    .ok_or(tlog_tiles::Error::IndexesNotInTree)?;
                tile.hash_at_index(&self.tile_data(tile)?, index)
            })
            .collect()
    }
}

/// The tree `log` was integrated to, from what the SQL metadata store
/// recorded of it.
fn integrated_head(log: &LogId, origin: &str, head: Option<(u64, Vec<u8>)>) -> Result<TreeHead> {
    let Some((size, root)) = head else {
        return Ok(TreeHead { origin: origin.to_string(), size: 0, hash: empty_root() });
    };
    let hash = <[u8; 32]>::try_from(root.as_slice())
        .map_err(|_| RegistryError::Internal {
            reason: format!("the integrated root of the log {log} is not a SHA-256 hash"),
        })?;
    Ok(TreeHead { origin: origin.to_string(), size, hash: Hash(hash) })
}

pub(super) fn check_sequenced_at_least_head(
    log: &LogId,
    sequenced: u64,
    head_size: u64,
) -> Result<()> {
    if sequenced < head_size {
        return Err(RegistryError::Internal {
            reason: format!(
                "the log {log} has {sequenced} sequenced entries, fewer than its integrated tree of {head_size}",
            ),
        });
    }
    Ok(())
}

/// The hash of the empty tree.
const fn empty_root() -> Hash {
    pnpm_transparency_log::EMPTY_HASH
}

fn log_error(error: impl fmt::Display) -> RegistryError {
    RegistryError::Internal { reason: format!("transparency log: {error}") }
}
