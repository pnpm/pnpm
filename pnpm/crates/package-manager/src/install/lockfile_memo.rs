//! The cache directory's copy of the last wanted lockfile pnpm wrote
//! for a project.
//!
//! pnpm already reconstructs a deleted `pnpm-lock.yaml` from the copy
//! the virtual store keeps (`<virtual_store_dir>/lock.yaml`) when the
//! recorded resolution still satisfies the manifest — that is the
//! `withWarmModules` fast path, and it commits pnpm to the semantics
//! that a no-lockfile install may reuse its own prior resolution
//! instead of re-resolving. This module extends the same rule to the
//! state where `node_modules` is gone as well: every wanted lockfile
//! written also lands here, keyed by the workspace root, and an
//! install that has neither `pnpm-lock.yaml` nor a virtual store to
//! synthesize from may synthesize from this copy — gated by the same
//! freshness check, so a manifest or settings change falls through to
//! a fresh resolve exactly as it would on the virtual-store path.
//!
//! What this buys: reinstalling with a warm cache but no lockfile (a
//! CI cache restore, a deleted-lockfile reinstall) skips the resolve
//! pass the way a lockfile-bearing install does, instead of re-walking
//! the whole graph to arrive at the picks the mirror would give it
//! anyway.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use pnpm_lockfile::Lockfile;

/// Where the memo for `workspace_root` lives under `cache_dir`. Keyed
/// by a hash of the workspace root so machine-global cache directories
/// keep one memo per project, and versioned by directory so a future
/// format change can move aside without parsing old copies.
pub(crate) fn memo_path(cache_dir: &Path, workspace_root: &Path) -> PathBuf {
    let key = pnpm_crypto_hash::create_hex_hash(&workspace_root.to_string_lossy());
    cache_dir.join("lockfile-memo").join("v1").join(format!("{key}.yaml"))
}

/// The memoized lockfile, when one is recorded and parses. Every
/// failure shape — absent, unreadable, unparsable, stale version —
/// reads as "no memo": the caller's fallback is a fresh resolve, which
/// regenerates the memo.
pub(crate) fn load(cache_dir: &Path, workspace_root: &Path) -> Option<Lockfile> {
    Lockfile::load_from_file(&memo_path(cache_dir, workspace_root)).ok().flatten()
}

/// Record the wanted lockfile that was just written for
/// `workspace_root`. Best-effort: the memo is a cache, and an install
/// that produced a correct `pnpm-lock.yaml` must not fail because the
/// cache directory is read-only or full — a failed write only costs
/// the next no-lockfile install a resolve it would have paid anyway.
pub(crate) fn persist(cache_dir: &Path, workspace_root: &Path) {
    if let Err(error) = try_persist(cache_dir, workspace_root) {
        tracing::debug!(
            target: "pacquet::install",
            error = %error,
            "couldn't record the lockfile memo; the next no-lockfile install re-resolves",
        );
    }
}

fn try_persist(cache_dir: &Path, workspace_root: &Path) -> io::Result<()> {
    let source = workspace_root.join(Lockfile::FILE_NAME);
    let target = memo_path(cache_dir, workspace_root);
    let dir = target.parent().expect("memo path always has a parent");
    fs::create_dir_all(dir)?;
    // Copy through a sibling temp file so a concurrent install reading
    // the memo sees the old copy or the new one, never a torn write.
    let temp = target.with_extension(format!("tmp{}", std::process::id()));
    fs::copy(&source, &temp)?;
    fs::rename(&temp, &target).inspect_err(|_| {
        let _ = fs::remove_file(&temp);
    })
}

#[cfg(test)]
mod tests;
