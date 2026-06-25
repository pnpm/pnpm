//! Global package install support for pacquet.
//!
//! Ports pnpm's `@pnpm/global.packages` (the on-disk layout: scanning,
//! the hash-symlink helpers, the cache key) and the pure helpers of
//! `@pnpm/global.commands` (bin-conflict detection, the `list -g`
//! renderers). The command orchestration that drives an install per group
//! lives in the CLI crate, alongside the install pipeline it needs —
//! mirroring how pnpm's `global.commands` sit above `installing.deps-installer`.

mod cache_key;
mod check_bin_conflicts;
mod global_package_dir;
mod list;
mod scan;

use serde_json::Value;
use std::path::Path;

pub use cache_key::create_global_cache_key;
pub use check_bin_conflicts::{
    CheckGlobalBinConflictsError, GlobalBinConflictError, check_global_bin_conflicts,
};
pub use global_package_dir::{create_install_dir, get_hash_link, resolve_install_dir};
pub use list::{ListReportAs, list_global_packages};
pub use scan::{
    GlobalPackageInfo, InstalledGlobalPackage, clean_orphaned_install_dirs, find_global_package,
    get_global_package_details, get_installed_bin_names, read_installed_packages,
    scan_global_packages,
};

/// Read and parse a `package.json` from `dir`, returning `None` on any
/// read or parse failure. Mirrors pnpm's `safeReadPackageJsonFromDir`.
pub(crate) fn read_package_json(dir: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(dir.join("package.json")).ok()?;
    serde_json::from_str(&text).ok()
}
