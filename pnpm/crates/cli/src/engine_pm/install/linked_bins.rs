//! The marker that says an engine's bins are fully linked.
//!
//! The bin directory of an engine slot is shared by every process on the
//! host, and the engine runs from it. Linking rewrites files there, so only
//! the holder of the slot lock may link, and a process without the lock may
//! run the engine only once linking is complete. The marker is how the
//! latter tells: it is written after every bin, and names the pnpm version
//! that linked them, so a pnpm that links differently relinks under the lock.

use super::{
    EnginePackages, InstalledEngine, compute_engine_slot, package_dir, pnpm_executable_path,
};
use pnpm_config::{Config, PNPM_VERSION};
use pnpm_fs::write_atomic;
use pnpm_lockfile::EnvLockfile;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

const MARKER: &str = ".pnpm-engine-linked";

/// The engine, when its global-virtual-store slot is
/// populated and this pnpm finished linking its bins. Writes nothing, so it
/// needs no lock.
pub(super) fn linked_engine_bins(
    config: &Config,
    env: &EnvLockfile,
    package: EnginePackages,
    version: &str,
) -> Option<InstalledEngine> {
    let slot = populated_engine_slot(config, env, package, version)?;
    let bin_dir = slot.join("bin");
    if !are_current(&bin_dir) {
        return None;
    }
    let native_binary =
        package.links_native_binary.then(|| pnpm_executable_path(&slot, package.wrapper));
    Some(InstalledEngine { bin_dir, native_binary, private_install: None })
}

/// The engine's global-virtual-store slot, when it is already populated. The
/// slot is computed with the same hashing the install pipeline uses, so a
/// stale or wrong computation merely misses the cache: the idempotent install
/// then re-derives the slot from its own symlink.
pub(super) fn populated_engine_slot(
    config: &Config,
    env: &EnvLockfile,
    package: EnginePackages,
    version: &str,
) -> Option<PathBuf> {
    let slot = compute_engine_slot(config, env, package, version)?;
    package_dir(&slot, package.wrapper)
        .join("package.json")
        .exists()
        .then_some(slot)
}

/// Whether a pnpm of this version finished linking the bins in `bin_dir`.
pub(super) fn are_current(bin_dir: &Path) -> bool {
    fs::read_to_string(bin_dir.join(MARKER)).is_ok_and(|linked_by| linked_by == PNPM_VERSION)
}

/// Record that this pnpm finished linking the bins in `bin_dir`.
pub(super) fn mark_current(bin_dir: &Path) -> io::Result<()> {
    write_atomic(&bin_dir.join(MARKER), PNPM_VERSION.as_bytes())
}
