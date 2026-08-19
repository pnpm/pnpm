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
//! pass the way a lockfile-bearing install does.
//!
//! The memo's invariant: it records only pnpmfile-free resolutions,
//! and it only ever answers pnpmfile-free installs. Custom resolvers
//! and fetchers shape resolution but are invisible to the lockfile's
//! `pnpmfileChecksum` (only hook-exporting pnpmfiles are recorded), so
//! nothing in the freshness check can attest a memo across a
//! pnpmfile-regime change — in either direction. Both sides are
//! therefore gated on "no pnpmfile loaded": a fresh resolve persists a
//! memo only when it ran without one, and the synthesis chain consults
//! the memo only when the current install runs without one. The
//! regenerate-the-lockfile paths never write the memo at all: what
//! they save was synthesized, and its provenance is the earlier
//! install that recorded it, not this one.

use std::{
    io,
    path::{Path, PathBuf},
};

use pnpm_lockfile::Lockfile;

/// Where the memo for `workspace_root` lives under `cache_dir`. Keyed
/// by a hash of the workspace root so machine-global cache directories
/// keep one memo per project, and versioned by directory so a future
/// format change can move aside without parsing old copies.
///
/// The key hashes the path's own bytes: a lossy UTF-8 conversion maps
/// distinct non-UTF-8 paths to one replacement-charactered string, and
/// two such projects must not share a memo.
pub(crate) fn memo_path(cache_dir: &Path, workspace_root: &Path) -> PathBuf {
    #[cfg(unix)]
    let key = {
        use std::os::unix::ffi::OsStrExt;
        pnpm_crypto_hash::create_hex_hash_bytes(workspace_root.as_os_str().as_bytes())
    };
    // Windows paths are UTF-16; unpaired surrogates are possible but a
    // path that survives a round trip through the filesystem APIs in
    // practice doesn't carry them, and the lossy form is stable for a
    // given path either way.
    #[cfg(not(unix))]
    let key = pnpm_crypto_hash::create_hex_hash(&workspace_root.to_string_lossy());
    cache_dir.join("lockfile-memo").join("v1").join(format!("{key}.yaml"))
}

/// The largest memo the loader will read. Real lockfiles run to a few
/// megabytes; the bound exists so a corrupt or planted cache entry can
/// cost at most this much memory and parser time before it is
/// rejected, never an OOM.
const MAX_MEMO_BYTES: u64 = 128 * 1024 * 1024;

/// The memoized lockfile, when one is recorded and parses. Every
/// failure shape — absent, not a regular file, oversized, unreadable,
/// unparsable, stale version — reads as "no memo": the caller's
/// fallback is a fresh resolve, which regenerates the memo.
///
/// The read is hardened on the descriptor itself
/// ([`pnpm_fs::read_regular_file_capped`]): symlinks are refused at
/// open, a planted FIFO fails instead of blocking, and the size bound
/// holds on the bytes actually read — a stat-then-open pair would be a
/// race a concurrent writer in the cache directory could win.
pub(crate) fn load(cache_dir: &Path, workspace_root: &Path) -> Option<Lockfile> {
    let path = memo_path(cache_dir, workspace_root);
    let bytes = pnpm_fs::read_regular_file_capped(&path, MAX_MEMO_BYTES).ok()??;
    let content = String::from_utf8(bytes).ok()?;
    Lockfile::parse(&content, &path).ok()?
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
    let bytes = pnpm_fs::read_regular_file_capped(&source, MAX_MEMO_BYTES)?
        .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
    // `write_atomic` stages through an exclusively-created sibling temp
    // file and renames over the target, so a concurrent install reading
    // the memo sees the old copy or the new one, never a torn write.
    pnpm_fs::write_atomic(&memo_path(cache_dir, workspace_root), &bytes)
}

#[cfg(test)]
mod tests;
