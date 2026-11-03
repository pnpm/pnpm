//! The transparency logs of the release records a hosted catalog accepts
//! ([release records](https://github.com/pnpm/rfcs/blob/92d2b281af321ecf8f1e5fea63f698716ba4e9e7/pnpr/text/0000-release-records.md#transparency-log)),
//! one per catalog and ecosystem.
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

use crate::{DocumentWrite, Storage};
use pnpm_transparency_log::{
    BundleRange, Checkpoint, Hash, HashReader, NoteSigner, TILE_HEIGHT, TilePath, TreeHead,
    encode_bundle, integrate, prove_inclusion, sign_checkpoint,
};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::Ecosystem;
use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Mutex, PoisonError},
};
use tlog_tiles::Tile;

/// The hosted-store namespace of the logs' tiles and checkpoints. A leading
/// `.` keeps it apart from every organization and package name.
pub const LOG_NAMESPACE: &str = ".transparency-log";

/// The most entries one round of integration reads.
const INTEGRATION_BATCH: u64 = 4096;

/// One transparency log: the records of one hosted catalog and ecosystem.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LogId {
    /// The organization namespace; empty for the default one.
    pub catalog: String,
    pub ecosystem: Ecosystem,
}

impl LogId {
    /// The log's path in the store and in its origin: its ecosystem, and for
    /// an organization other than the default one, `~<catalog>`.
    fn path(&self) -> String {
        if self.catalog.is_empty() {
            self.ecosystem.as_str().to_string()
        } else {
            format!("{}/~{}", self.ecosystem.as_str(), self.catalog)
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

/// A log's latest checkpoint as it is stored, and the tree head it signs.
#[derive(Debug, Clone)]
pub struct StoredCheckpoint {
    /// The signed note.
    pub note: Vec<u8>,
    pub tree_head: TreeHead,
}

/// An entry's inclusion proof against a stored checkpoint.
#[derive(Debug, Clone)]
pub struct InclusionProof {
    pub checkpoint: StoredCheckpoint,
    pub index: u64,
    pub proof: Vec<Hash>,
}

impl Storage {
    /// The latest checkpoint of `log`, `None` until an integration signs one.
    pub async fn log_checkpoint(&self, log: &LogId) -> Result<Option<StoredCheckpoint>> {
        let Some(note) = self.root.read_record(LOG_NAMESPACE, &log.key("checkpoint")).await? else {
            return Ok(None);
        };
        let tree_head = tree_head_of(&note)?;
        Ok(Some(StoredCheckpoint { note, tree_head }))
    }

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
        let signer = log.signer(key)?;
        let origin = signer.verifier_key().name().to_string();
        let database = self.sql_metadata();
        loop {
            let head = integrated_head(log, &origin, database.log_head(log).await?)?;
            let sequenced = database.log_size(log).await?;
            if sequenced <= head.size {
                self.publish_checkpoint(log, &head, &signer).await?;
                return Ok(head);
            }
            let end = sequenced.min(head.size + INTEGRATION_BATCH);
            let entries = database.log_entries(log, head.size..end).await?;
            let tree_head = self.integrate_batch(log, &head, &entries).await?;
            database.advance_log_head(log, tree_head.size, tree_head.hash.0).await?;
            self.publish_checkpoint(log, &tree_head, &signer).await?;
        }
    }

    /// Sign and write a checkpoint of `tree_head`, unless the stored one
    /// already covers it.
    async fn publish_checkpoint(
        &self,
        log: &LogId,
        tree_head: &TreeHead,
        signer: &NoteSigner,
    ) -> Result<()> {
        let current = self.log_checkpoint(log).await?;
        let covered = match &current {
            Some(stored) => stored.tree_head.size >= tree_head.size,
            None => tree_head.size == 0,
        };
        if covered {
            return Ok(());
        }
        let note = sign_checkpoint(tree_head, &[signer]).map_err(log_error)?;
        self.advance_checkpoint(log, current, &note, tree_head.size).await
    }

    /// Write the tiles and entry bundles that `entries`, sequenced after the
    /// tree `head` describes, add to it, and return the new tree head. The
    /// stored tiles must hash to `head`'s root first.
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
        for (path, bytes) in tiles.into_iter().chain(bundles) {
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
        let database = self.sql_metadata();
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

    /// Replace `current` with `note`, a checkpoint of `size` entries. A
    /// checkpoint another integrator moved past `size` already covers it.
    async fn advance_checkpoint(
        &self,
        log: &LogId,
        current: Option<StoredCheckpoint>,
        note: &[u8],
        size: u64,
    ) -> Result<()> {
        let key = log.key("checkpoint");
        let mut current = current;
        loop {
            let written = match &current {
                None => self.root.create_record(LOG_NAMESPACE, &key, note).await?,
                Some(stored) if stored.tree_head.size >= size => return Ok(()),
                Some(stored) => {
                    let replaced = self.root.replace_record_if_current(
                        LOG_NAMESPACE,
                        &key,
                        &stored.note,
                        note,
                    )
                    .await?;
                    replaced == DocumentWrite::Written
                }
            };
            if written {
                return Ok(());
            }
            current = self.log_checkpoint(log).await?;
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

/// The tree head a stored checkpoint note signs: the text before the blank
/// line that starts its signatures.
fn tree_head_of(note: &[u8]) -> Result<TreeHead> {
    let malformed =
        || RegistryError::Internal { reason: "a stored log checkpoint is malformed".to_string() };
    let text_end = note
        .windows(2)
        .position(|pair| pair == b"\n\n")
        .ok_or_else(malformed)?;
    let checkpoint = Checkpoint::from_bytes(&note[..=text_end]).map_err(|_| malformed())?;
    Ok(TreeHead {
        origin: checkpoint.origin().to_string(),
        size: checkpoint.size(),
        hash: *checkpoint.hash(),
    })
}

/// The hash of the empty tree.
const fn empty_root() -> Hash {
    pnpm_transparency_log::EMPTY_HASH
}

fn log_error(error: impl fmt::Display) -> RegistryError {
    RegistryError::Internal { reason: format!("transparency log: {error}") }
}
