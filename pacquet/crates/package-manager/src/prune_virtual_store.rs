//! Surplus virtual-store cleanup: remove the
//! `node_modules/.pnpm/<dir>` entries the wanted lockfile no longer
//! references.
//!
//! Port of the virtual-store sweep in pnpm's `prune`
//! (<https://github.com/pnpm/pnpm/blob/e1e29c1520/installing/linking/modules-cleaner/src/prune.ts#L173-L231>)
//! together with the throttle that decides whether the sweep runs this
//! install
//! (<https://github.com/pnpm/pnpm/blob/74a2dc9027/installing/deps-installer/src/install/index.ts#L471-L473>).
//!
//! Only the virtual-store sweep is ported here. The rest of upstream's
//! `prune` — removing changed direct dependencies from importer
//! `node_modules`, the hoisted-dependency removal (pacquet handles that
//! in [`crate::link_hoisted_modules`]), and the `pnpm:stats` `removed`
//! count derived from the current-vs-wanted orphan diff — is not part
//! of this slice.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime};

use pacquet_lockfile::{Lockfile, PkgNameVerPeer};

use crate::SkippedSnapshots;

/// Decide whether the virtual-store sweep should run this install.
///
/// Port of pnpm's `pruneVirtualStore` gate at
/// <https://github.com/pnpm/pnpm/blob/74a2dc9027/installing/deps-installer/src/install/index.ts#L471-L473>:
/// never prune under the global virtual store; otherwise prune unless a
/// recorded `prunedAt` is still within `modules_cache_max_age`. A
/// missing prior `.modules.yaml` (`prior_pruned_at == None`), an empty
/// timestamp, or a non-positive max-age each force a prune, matching the
/// upstream falsy checks.
#[must_use]
pub fn should_prune_virtual_store(
    enable_global_virtual_store: bool,
    prior_pruned_at: Option<&str>,
    modules_cache_max_age: u64,
    now: SystemTime,
) -> bool {
    if enable_global_virtual_store {
        return false;
    }
    match prior_pruned_at {
        Some(pruned_at) if !pruned_at.is_empty() && modules_cache_max_age > 0 => {
            cache_expired(pruned_at, modules_cache_max_age, now)
        }
        _ => true,
    }
}

/// `true` when `pruned_at` is older than `max_age_minutes`. Port of
/// pnpm's `cacheExpired` at
/// <https://github.com/pnpm/pnpm/blob/74a2dc9027/installing/deps-installer/src/install/index.ts#L1180-L1182>.
///
/// An unparseable timestamp counts as *not* expired, matching upstream:
/// `new Date("garbage").valueOf()` is `NaN`, and `NaN > max_age` is
/// `false`, so pnpm skips the prune. A timestamp in the future also
/// counts as fresh (upstream's subtraction goes negative there, never
/// `>` `max_age`).
fn cache_expired(pruned_at: &str, max_age_minutes: u64, now: SystemTime) -> bool {
    let Ok(pruned_at) = httpdate::parse_http_date(pruned_at) else {
        return false;
    };
    let Ok(elapsed) = now.duration_since(pruned_at) else {
        return false;
    };
    elapsed > Duration::from_secs(max_age_minutes.saturating_mul(60))
}

/// Remove every `<virtual_store_dir>/<dir>` the wanted lockfile no
/// longer needs, returning the number of directories removed.
///
/// Port of the virtual-store sweep in pnpm's `prune`
/// (<https://github.com/pnpm/pnpm/blob/e1e29c1520/installing/linking/modules-cleaner/src/prune.ts#L180-L190>):
/// the needed set is `node_modules` plus one
/// [`PkgNameVerPeer::to_virtual_store_name`] per non-skipped snapshot
/// key; any other on-disk entry is surplus and removed.
///
/// `snapshot_keys` are the wanted lockfile's `snapshots:` keys — the
/// peer-suffixed dep paths that name the per-package subdirectories of
/// the virtual store. `skipped` is the union of snapshots dropped from
/// the install (`--no-optional`, installability, fetch failure); their
/// directories were never materialized, so they must not count as
/// needed.
pub fn prune_virtual_store<'a>(
    virtual_store_dir: &Path,
    snapshot_keys: impl Iterator<Item = &'a PkgNameVerPeer>,
    skipped: &SkippedSnapshots,
    virtual_store_dir_max_length: usize,
) -> usize {
    let needed = needed_virtual_store_names(snapshot_keys, skipped, virtual_store_dir_max_length);
    let mut removed = 0;
    for entry_name in read_virtual_store_dir(virtual_store_dir) {
        if needed.contains(&entry_name) {
            continue;
        }
        try_remove_pkg(&virtual_store_dir.join(entry_name));
        removed += 1;
    }
    removed
}

/// Build the set of virtual-store subdirectory names the wanted
/// lockfile needs. Mirrors the `neededPkgs` set at
/// <https://github.com/pnpm/pnpm/blob/e1e29c1520/installing/linking/modules-cleaner/src/prune.ts#L180-L184>.
fn needed_virtual_store_names<'a>(
    snapshot_keys: impl Iterator<Item = &'a PkgNameVerPeer>,
    skipped: &SkippedSnapshots,
    virtual_store_dir_max_length: usize,
) -> HashSet<String> {
    // `node_modules` is the `.bin`-and-friends sibling pnpm always keeps
    // under the virtual store; it is not a package directory and must
    // never be swept.
    //
    // `lock.yaml` (the current lockfile) also lives here. Upstream's
    // `prune` deletes it and unconditionally rewrites it afterwards, but
    // pacquet's current-lockfile write is conditional (skipped when
    // `config.lockfile` is off, see `install.rs`), so deleting it in the
    // sweep could orphan it. Keeping it is end-state-equivalent whenever
    // the rewrite runs and strictly safer when it doesn't.
    let mut needed =
        HashSet::from(["node_modules".to_string(), Lockfile::CURRENT_FILE_NAME.to_string()]);
    for key in snapshot_keys {
        if skipped.contains(key) {
            continue;
        }
        needed.insert(key.to_virtual_store_name(virtual_store_dir_max_length));
    }
    needed
}

/// List the immediate entry names of the virtual store directory.
/// Mirrors pnpm's `readVirtualStoreDir`
/// (<https://github.com/pnpm/pnpm/blob/e1e29c1520/installing/linking/modules-cleaner/src/prune.ts#L204-L217>):
/// a missing directory yields an empty list (a first install has
/// nothing to prune); any other read error is logged and treated as
/// empty so a transient failure can't delete packages it failed to
/// enumerate.
fn read_virtual_store_dir(virtual_store_dir: &Path) -> Vec<String> {
    let entries = match fs::read_dir(virtual_store_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            tracing::warn!(
                ?error,
                virtual_store_dir = %virtual_store_dir.display(),
                "failed to read virtual store directory",
            );
            return Vec::new();
        }
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect()
}

/// `rimraf` a surplus virtual-store entry. Mirrors pnpm's `tryRemovePkg`
/// (<https://github.com/pnpm/pnpm/blob/e1e29c1520/installing/linking/modules-cleaner/src/prune.ts#L219-L231>):
/// a removal failure is logged and swallowed — a leftover entry is less
/// harmful than aborting the install, and the next sweep retries.
///
/// Surplus entries are normally package directories, but a stray file or
/// symlink could appear; upstream's `rimraf` removes any of them, so this
/// does too. `symlink_metadata` keeps the file/symlink branch from
/// following a link into a real directory.
fn try_remove_pkg(path: &Path) {
    let result = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) => Err(error),
    };
    match result {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            tracing::warn!(?error, path = %path.display(), "failed to remove virtual store entry");
        }
    }
}

#[cfg(test)]
mod tests;
