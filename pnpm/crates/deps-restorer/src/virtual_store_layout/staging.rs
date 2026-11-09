//! The staging directories an install builds global virtual store packages in.

use super::VirtualStoreLayout;
use pnpm_lockfile::PackageKey;
use std::path::PathBuf;

/// A token no other install holds: a counter for the installs one process
/// runs, plus 64 random bits for the processes that share a store, across
/// containers and hosts too.
pub(super) fn new_staging_token() -> String {
    use std::hash::{BuildHasher as _, Hasher as _};
    static INSTALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let install = INSTALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // Each `RandomState` is seeded from the operating system's random source.
    let random = std::collections::hash_map::RandomState::new().build_hasher().finish();
    format!("{install:x}-{random:016x}")
}

impl VirtualStoreLayout {
    /// Where this install builds `key`'s package before renaming it into
    /// its global virtual store slot. See [`pnpm_store_dir::staging_dir_name`].
    pub(crate) fn staging_package_dir(&self, key: &PackageKey) -> PathBuf {
        let package_dir = crate::build_modules::slots::virtual_store_dir_for_key(self, key);
        let name = package_dir.file_name().unwrap_or_default();
        package_dir.with_file_name(pnpm_store_dir::staging_dir_name(name, &self.staging_token))
    }
}
