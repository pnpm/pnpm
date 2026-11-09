//! The staging directories an install builds global virtual store packages in.

use super::VirtualStoreLayout;
use pnpm_lockfile::PackageKey;
use std::path::PathBuf;

/// A token for this install's staging directories: 32 random bits, short
/// enough for [`pnpm_store_dir::staging_dir_name`]. Only installs building
/// the same package at once can collide, which takes one in four billion.
pub(super) fn new_staging_token() -> String {
    use std::hash::{BuildHasher as _, Hasher as _};
    // Each `RandomState` gets fresh keys, seeded from the operating system's
    // random source.
    let random = std::collections::hash_map::RandomState::new().build_hasher().finish();
    format!("{:08x}", random & u64::from(u32::MAX))
}

impl VirtualStoreLayout {
    /// Where this install builds `key`'s package before renaming it into
    /// its global virtual store slot. See [`pnpm_store_dir::staging_dir_name`].
    pub(crate) fn staging_package_dir(&self, key: &PackageKey) -> PathBuf {
        let package_dir = crate::build_modules::slots::virtual_store_dir_for_key(self, key);
        package_dir.with_file_name(pnpm_store_dir::staging_dir_name(&self.staging_token))
    }
}
