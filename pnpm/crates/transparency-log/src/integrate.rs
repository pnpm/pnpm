//! Integrating sequenced entries into the tree.
//!
//! The registry assigns each entry its index when it commits the entry, in
//! the same transaction that accepts what the entry describes. Integration
//! then runs after commit: it computes the hashes to store, the tiles to
//! publish, and the new tree head. Its output depends only on the stored
//! hashes and the entries, so running it again after a crash writes the same
//! values.

use crate::{
    bundle::MAX_ENTRY_SIZE,
    proof::LogError,
    tiles::{TILE_HEIGHT, TILE_WIDTH, TilePath},
};
use std::{collections::BTreeMap, ops::Range};
use tlog_tiles::{Hash, HashReader, Tile, stored_hash_index, stored_hashes, tree_hash};

/// A hash to persist at its storage index, as Go's `tlog.StoredHashIndex`
/// numbers them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StoredHash {
    /// The storage index.
    pub index: u64,
    /// The hash.
    pub hash: Hash,
}

/// The result of integrating entries into a tree.
#[derive(Debug, Clone, PartialEq)]
pub struct Integration {
    /// The tree size before integration.
    pub previous_size: u64,
    /// The tree size after integration.
    pub size: u64,
    /// The Merkle tree hash at [`Integration::size`].
    pub root: Hash,
    /// The hashes to persist, in storage index order.
    pub stored_hashes: Vec<StoredHash>,
}

/// An entry bundle to publish and the entry indexes it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleRange {
    /// The bundle's path.
    pub path: TilePath,
    /// The indexes of the entries it holds, in order.
    pub entries: Range<u64>,
}

/// Integrates `entries`, sequenced at indexes `tree_size..`, into a tree of
/// `tree_size` entries whose hashes `store` holds.
pub fn integrate<Store: HashReader, Entry: AsRef<[u8]>>(
    store: &Store,
    tree_size: u64,
    entries: &[Entry],
) -> Result<Integration, LogError> {
    if let Some(entry) = entries
        .iter()
        .find(|entry| entry.as_ref().len() > MAX_ENTRY_SIZE)
    {
        return Err(LogError::EntryTooLarge { size: entry.as_ref().len() });
    }
    let mut overlay = Overlay { store, pending: BTreeMap::new() };
    for (index, entry) in (tree_size..).zip(entries) {
        let hashes = stored_hashes(index, entry.as_ref(), &overlay).map_err(LogError::from)?;
        let first = stored_hash_index(0, index);
        overlay.pending.extend((first..).zip(hashes));
    }
    let size = tree_size + entries.len() as u64;
    let root = tree_hash(size, &overlay).map_err(LogError::from)?;
    let stored_hashes = overlay.pending
        .into_iter()
        .map(|(index, hash)| StoredHash { index, hash })
        .collect();
    Ok(Integration { previous_size: tree_size, size, root, stored_hashes })
}

impl Integration {
    /// The hash tiles to publish for this integration, with their contents.
    /// Hashes not stored by this integration are read from `store`.
    pub fn tiles<Store: HashReader>(
        &self,
        store: &Store,
    ) -> Result<Vec<(TilePath, Vec<u8>)>, LogError> {
        let pending = self.stored_hashes
            .iter()
            .map(|stored| (stored.index, stored.hash))
            .collect();
        let overlay = Overlay { store, pending };
        Tile::new_tiles(TILE_HEIGHT, self.previous_size, self.size)
            .iter()
            .map(|tile| {
                let path = TilePath::of_tile(tile).ok_or(LogError::InvalidTile)?;
                Ok((path, tile.read_data(&overlay).map_err(LogError::from)?))
            })
            .collect()
    }

    /// The entry bundles to publish for this integration. A partial bundle
    /// that gained entries is published again under its new width.
    #[must_use]
    pub fn entry_bundles(&self) -> Vec<BundleRange> {
        let width = u64::from(TILE_WIDTH);
        if self.size == self.previous_size {
            return Vec::new();
        }
        (self.previous_size / width..self.size.div_ceil(width))
            .map(|index| {
                let start = index * width;
                let end = self.size.min(start + width);
                let path = TilePath::Entries { index, width: (end - start) as u32 };
                BundleRange { path, entries: start..end }
            })
            .collect()
    }
}

/// Reads hashes from the integration in progress first, then from the store.
struct Overlay<'a, Store> {
    store: &'a Store,
    pending: BTreeMap<u64, Hash>,
}

impl<Store: HashReader> HashReader for Overlay<'_, Store> {
    fn read_hashes(&self, indexes: &[u64]) -> Result<Vec<Hash>, tlog_tiles::Error> {
        let missing: Vec<u64> = indexes
            .iter()
            .copied()
            .filter(|index| !self.pending.contains_key(index))
            .collect();
        let mut fetched = read_checked(self.store, &missing)?.into_iter();
        indexes
            .iter()
            .map(|index| match self.pending.get(index) {
                Some(hash) => Ok(*hash),
                None => fetched.next().ok_or(tlog_tiles::Error::BadMath),
            })
            .collect()
    }
}

/// Reads `indexes` from `store`, rejecting a store that returns the wrong
/// number of hashes, which [`tlog_tiles`] would otherwise panic on.
pub(crate) fn read_checked<Store: HashReader + ?Sized>(
    store: &Store,
    indexes: &[u64],
) -> Result<Vec<Hash>, tlog_tiles::Error> {
    if indexes.is_empty() {
        return Ok(Vec::new());
    }
    let hashes = store.read_hashes(indexes)?;
    if hashes.len() != indexes.len() {
        return Err(tlog_tiles::Error::InvalidInput(format!(
            "hash store returned {} hashes for {} indexes",
            hashes.len(),
            indexes.len(),
        )));
    }
    Ok(hashes)
}

#[cfg(test)]
mod tests;
