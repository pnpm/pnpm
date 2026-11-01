//! The on-disk modes of store inodes this process has already read.

use dashmap::DashMap;
use std::{
    path::{Path, PathBuf},
    sync::LazyLock,
};

/// Keyed by CAS path. A store blob is content-addressed and only ever
/// replaced by equal bytes, so the mode read once stands for every later
/// import of the same path in this process. The bytes match but the mode
/// may not: another process's atomic rewrite carries its own mode, and a
/// `chmod` through a hardlink changes the shared inode. Neither is seen here.
///
/// Process-global rather than install-scoped, like [`crate::cas_write_lock`]:
/// the store verifier that stats every blob and the importer that needs the
/// mode sit in different crates, and every hop between them is on the hot
/// path. It holds one entry per store blob the process touched.
static OBSERVED_STORE_MODES: LazyLock<DashMap<PathBuf, u32>> = LazyLock::new(DashMap::new);

/// Remember that `store_path`'s inode carries `mode`, so a later
/// [`observed_store_inode_mode`] answers without a `stat`.
pub fn record_store_inode_mode(store_path: &Path, mode: u32) {
    OBSERVED_STORE_MODES.insert(store_path.to_path_buf(), mode);
}

/// Drop what [`record_store_inode_mode`] remembered for `store_path`. The
/// verifier calls this when a blob fails: its re-fetch may land a new inode
/// with this process's mode, which the importer must `stat` afresh.
pub fn forget_store_inode_mode(store_path: &Path) {
    OBSERVED_STORE_MODES.remove(store_path);
}

/// The mode [`record_store_inode_mode`] last remembered for `store_path`,
/// or `None` when this process has not read it yet.
#[must_use]
pub fn observed_store_inode_mode(store_path: &Path) -> Option<u32> {
    OBSERVED_STORE_MODES.get(store_path).map(|entry| *entry)
}
