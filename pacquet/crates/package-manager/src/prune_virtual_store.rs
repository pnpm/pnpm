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
//! in [`crate::link_hoisted_modules()`]), and the `pnpm:stats` `removed`
//! count derived from the current-vs-wanted orphan diff — is not part
//! of this slice.

use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use pacquet_lockfile::{Lockfile, PkgNameVerPeer};

use crate::SkippedSnapshots;

/// Decide whether the virtual-store sweep should run this install.
///
/// Port of pnpm's `pruneVirtualStore` gate at
/// <https://github.com/pnpm/pnpm/blob/74a2dc9027/installing/deps-installer/src/install/index.ts#L471-L473>.
///
/// `is_global_virtual_store` is decided by the caller from the resolved
/// paths (see [`same_dir`]), not the `enableGlobalVirtualStore` flag
/// alone: the global store is shared across projects, so a config that
/// points `virtualStoreDir` at it must not be pruned even when the flag
/// is off.
#[must_use]
pub fn should_prune_virtual_store(
    is_global_virtual_store: bool,
    prior_pruned_at: Option<&str>,
    modules_cache_max_age: u64,
    now: SystemTime,
) -> bool {
    if is_global_virtual_store {
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
/// longer needs.
///
/// Returns `Some(removed)` with the number of directories removed when
/// the store was enumerated, or `None` when enumeration failed (a real
/// `read_dir` error other than `NotFound`). The caller uses `None` to
/// avoid stamping a fresh `prunedAt` for a sweep that never actually
/// ran, which would otherwise throttle the next real sweep.
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
) -> Option<usize> {
    let entries = read_virtual_store_dir(virtual_store_dir)?;
    let needed = needed_virtual_store_names(snapshot_keys, skipped, virtual_store_dir_max_length);
    let mut removed = 0;
    for entry_name in entries {
        if needed.contains(&entry_name) {
            continue;
        }
        if try_remove_pkg(&virtual_store_dir.join(entry_name)) {
            removed += 1;
        }
    }
    Some(removed)
}

/// The resolved directory the sweep is allowed to delete from, or `None`
/// when `virtual_store_dir` is not a safe prune target. A safe target must
/// resolve to a strict descendant of `modules_dir`. The sweep deletes
/// directories, and `virtual_store_dir` can come from repo-controlled
/// workspace config, so a path that escapes `node_modules` (via `..`, an
/// absolute location, or a symlink) — or that *is* `node_modules` itself —
/// is refused to avoid destructive deletes outside the managed tree.
///
/// Paths are resolved to a symlink-free, absolute form so symlinked
/// escapes are caught, and the returned resolved path is what the caller
/// must enumerate and delete from: validating one path and then operating
/// on the original (still-symlinkable) path would leave a
/// time-of-check/time-of-use gap.
///
/// A `virtual_store_dir` that does not exist yet (e.g. a first install) is
/// resolved through its nearest existing ancestor and still containment-
/// checked, so a missing path that points outside `node_modules` is
/// refused even though it could be created mid-install.
#[must_use]
pub fn prune_target_within_modules(
    virtual_store_dir: &Path,
    modules_dir: &Path,
) -> Option<PathBuf> {
    let modules_dir = fs::canonicalize(modules_dir).ok()?;
    let virtual_store_dir = resolve_through_existing_ancestor(virtual_store_dir)?;
    (virtual_store_dir != modules_dir && virtual_store_dir.starts_with(&modules_dir))
        .then_some(virtual_store_dir)
}

/// Resolve `path` to an absolute, symlink-free form even when its trailing
/// components don't exist yet: canonicalize the deepest existing ancestor
/// and re-append the missing tail. Returns `None` when no ancestor can be
/// canonicalized, or when a trailing component is not a normal name (e.g.
/// `..`), so an unprovable path is refused rather than trusted.
fn resolve_through_existing_ancestor(path: &Path) -> Option<PathBuf> {
    let mut tail = Vec::new();
    let mut current = path;
    loop {
        if let Ok(mut base) = fs::canonicalize(current) {
            base.extend(tail.iter().rev());
            return Some(base);
        }
        tail.push(current.file_name()?.to_owned());
        current = current.parent()?;
    }
}

/// Whether two paths refer to the same directory. Compares canonicalized
/// forms when both resolve (so symlinks and `.`/`..` segments don't hide
/// a match), falling back to a lexical comparison otherwise.
#[must_use]
pub fn same_dir(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
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
/// nothing to prune). Any other read error returns `None` so the sweep
/// can't delete packages it failed to enumerate, and the caller knows
/// the sweep didn't run.
fn read_virtual_store_dir(virtual_store_dir: &Path) -> Option<Vec<String>> {
    let entries = match fs::read_dir(virtual_store_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Some(Vec::new()),
        Err(error) => {
            tracing::warn!(
                ?error,
                virtual_store_dir = %virtual_store_dir.display(),
                "failed to read virtual store directory",
            );
            return None;
        }
    };
    Some(
        entries
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect(),
    )
}

/// `rimraf` a surplus virtual-store entry, returning whether the entry is
/// gone afterwards. Mirrors pnpm's `tryRemovePkg`
/// (<https://github.com/pnpm/pnpm/blob/e1e29c1520/installing/linking/modules-cleaner/src/prune.ts#L219-L231>):
/// a removal failure is logged and swallowed — a leftover entry is less
/// harmful than aborting the install, and the next sweep retries. A `false`
/// return lets the caller keep its removed count accurate (it must not
/// count an entry a swallowed error left behind).
///
/// Surplus entries are normally package directories, but a stray file or
/// symlink could appear; upstream's `rimraf` removes any of them, so this
/// does too. `symlink_metadata` keeps the file/symlink branch from
/// following a link into a real directory.
fn try_remove_pkg(path: &Path) -> bool {
    let result = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) => Err(error),
    };
    match result {
        Ok(()) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => true,
        Err(error) => {
            tracing::warn!(?error, path = %path.display(), "failed to remove virtual store entry");
            false
        }
    }
}

#[cfg(test)]
mod tests;
