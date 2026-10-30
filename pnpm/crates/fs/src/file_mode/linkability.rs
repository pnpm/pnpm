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
//! A mode change must not leave a stale answer behind, and a `stat` must not
//! hold up the other links that are being made at the same time. The two
//! are reconciled with a version counter: a mode change bumps it under the
//! map's lock before the `chmod` and drops the inode's answer after it, and
//! a `stat` that fills the map is remembered only if the counter did not
//! move while it ran. A `stat` therefore runs outside the lock, in parallel
//! with the others, and an answer computed from a mode that a change then
//! superseded is never published.
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
    sync::{
        Mutex, MutexGuard, OnceLock, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
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

/// How many mode changes have started, so a `stat` can tell whether one
/// began while it ran.
fn epoch() -> &'static AtomicU64 {
    static EPOCH: AtomicU64 = AtomicU64::new(0);
    &EPOCH
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
/// The `stat` this pays is the one a cache hit avoids, and it runs without
/// the map's lock: concurrent misses do not queue behind each other. The
/// answer is only remembered if no mode change started meanwhile, so a
/// `chmod` through a hardlink cannot leave this one behind.
pub fn source_mode_is_linkable(path: &Path, desired: u32) -> io::Result<bool> {
    let before = epoch().load(Ordering::Acquire);
    let metadata = fs::metadata(path)?;
    let linkable = super::store_inode_mode_is_linkable(metadata.mode(), desired);
    let inode = (metadata.dev(), metadata.ino());
    let mut cache = lock();
    if epoch().load(Ordering::Acquire) == before {
        cache.paths.insert(path.to_path_buf(), inode);
        cache.inodes.insert(inode, linkable);
    }
    Ok(linkable)
}

/// Run `change`, a mode change on `path`, and drop the answer remembered for
/// the inode it changes.
///
/// A `chmod` through a hardlink changes the mode of every name of the inode,
/// the store entry among them, so the remembered answer no longer holds.
/// The counter is bumped before the change and the entry dropped after it,
/// with the map locked throughout, so a `stat` that ran while the change
/// happened is not remembered, and one that finished before it has its
/// answer dropped here. The `stat` this pays is confined to mode changes,
/// not to the links a cache hit avoids.
///
/// `change` runs under the lock and so must not call back into this module.
pub fn with_mode_change<Value>(path: &Path, change: impl FnOnce() -> Value) -> Value {
    let inode = fs::metadata(path)
        .ok()
        .map(|metadata| (metadata.dev(), metadata.ino()));
    let mut cache = lock();
    epoch().fetch_add(1, Ordering::Release);
    let result = change();
    if let Some(inode) = inode {
        cache.inodes.remove(&inode);
    }
    result
}
