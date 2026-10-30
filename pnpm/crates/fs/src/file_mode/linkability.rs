//! The [`super::store_inode_mode_is_linkable`] answers remembered so far
//! (pnpm/pnpm#17500).
//!
//! A store entry's mode belongs to the entry, not to the target linked to
//! it, so a blob shared by many packages, a `LICENSE` say, need not be
//! `stat`ed once per link. The answer is keyed by inode because an inode is
//! what a `chmod` through a hardlink changes: `cmd-shim` widens a bin target
//! to `0o755`, and when that target shares its inode with the store entry it
//! widens the entry too.
//!
//! One mutex guards the map, the `stat` that fills it, and the `chmod` that
//! invalidates it ([`with_mode_change`]), so a read cannot straddle a mode
//! change: an answer computed from the old mode is either published before
//! the change starts and dropped by it, or computed after the change and
//! correct.
//!
//! A replacement of the entry under the same path needs no invalidation.
//! The store writes a content-addressed path once, and a repair or rewrite
//! goes through a fresh temp file created with the store-entry mode a fresh
//! write gives, which is exactly what [`super::store_entry_mode`] derives
//! for a link target. A new inode at the path therefore answers `true`
//! whether or not the old inode's answer is consulted: a stale `true` is
//! right, and a stale `false` costs a copy rather than a wrong mode. An
//! entry some other program replaced with a different mode is outside that
//! guarantee, and is why the answer is dropped on every mode change pnpm
//! makes itself.

use std::{
    collections::HashMap,
    fs, io,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, OnceLock, PoisonError},
};

struct Linkability {
    inodes: HashMap<(u64, u64), bool>,
    paths: HashMap<PathBuf, (u64, u64)>,
}

fn linkability() -> &'static Mutex<Linkability> {
    static LINKABILITY: OnceLock<Mutex<Linkability>> = OnceLock::new();
    LINKABILITY.get_or_init(|| {
        Mutex::new(Linkability { inodes: HashMap::new(), paths: HashMap::new() })
    })
}

fn lock() -> MutexGuard<'static, Linkability> {
    linkability().lock().unwrap_or_else(PoisonError::into_inner)
}

/// The remembered answer for the file at `path`, if any is remembered for
/// the inode last seen there.
///
/// `None` means "read the mode": nothing was remembered for the inode at
/// this path, or [`with_mode_change`] dropped it.
#[must_use]
pub fn cached_linkability(path: &Path) -> Option<bool> {
    let cache = lock();
    let inode = *cache.paths.get(path)?;
    cache.inodes.get(&inode).copied()
}

/// Whether the file at `path` can be linked where a fresh store write under
/// the current umask would put it, remembering the answer for its inode.
///
/// The `stat` this pays is the one a cache hit avoids. It runs under the
/// map's lock so no mode change can slip between the read and the insert.
pub fn source_mode_is_linkable(path: &Path, desired: u32) -> io::Result<bool> {
    let mut cache = lock();
    let metadata = fs::metadata(path)?;
    let linkable = super::store_inode_mode_is_linkable(metadata.mode(), desired);
    let inode = (metadata.dev(), metadata.ino());
    cache.paths.insert(path.to_path_buf(), inode);
    cache.inodes.insert(inode, linkable);
    Ok(linkable)
}

/// Run `change`, a mode change on `path`, and drop the answer remembered for
/// the inode it changes.
///
/// A `chmod` through a hardlink changes the mode of every name of the inode,
/// the store entry among them, so the remembered answer no longer holds.
/// The inode is read before the change and the entry dropped after it, with
/// the map locked across the change, so a concurrent import either reads the
/// old mode before the change and has its answer dropped here, or reads the
/// new mode after. The `stat` this pays is confined to mode changes, not to
/// the links a cache hit avoids.
///
/// `change` runs under the lock and so must not call back into this module.
pub fn with_mode_change<Value>(path: &Path, change: impl FnOnce() -> Value) -> Value {
    let inode = fs::metadata(path)
        .ok()
        .map(|metadata| (metadata.dev(), metadata.ino()));
    let mut cache = lock();
    let result = change();
    if let Some(inode) = inode {
        cache.inodes.remove(&inode);
    }
    result
}
