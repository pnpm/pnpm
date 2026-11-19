//! Thin wrapper around [`record_verification`](crate::cache::record_verification)
//! for callers that hold a freshly-written lockfile.
//!
//! After resolution writes a new lockfile, the install path uses this
//! wrapper to mark the new lockfile as already-verified so the
//! *next* install can take the cache fast path. Skipping the gate on
//! the next run is safe: fresh local picks went through the
//! resolver's per-version filter, and any carried-over entries
//! already passed the gate at the top of the same install.
//!
//! The function is a no-op when caching is disabled, when no
//! verifiers are active, or when the lockfile has no `packages:`
//! section.

use std::{path::Path, sync::Arc};

use pnpm_lockfile::Lockfile;
use pnpm_resolving_resolver_base::ResolutionVerifier;

use crate::{
    PendingVerificationRecord, cache::CachePrecomputed, hash_lockfile,
    verify_lockfile_resolutions::with_offline_check_cache_identities,
};

#[cfg(test)]
mod tests;

/// Persist the post-resolution lockfile as already-verified.
/// `cache_dir` enables the cache, `lockfile_path` is the absolute
/// path of the file the next install will read, `lockfile` is the
/// canonical in-memory shape that round-trips through the writer
/// (NOT the raw write object, since YAML drops `undefined` fields
/// and a hash of the raw shape would never match the parsed shape on
/// the next install).
pub fn record_lockfile_verified(
    cache_dir: Option<&Path>,
    lockfile_path: &Path,
    lockfile: &Lockfile,
    verifiers: &[Arc<dyn ResolutionVerifier>],
) {
    if let Some(pending) = pending_lockfile_verified(cache_dir, lockfile_path, lockfile, verifiers)
    {
        pending.record();
    }
}

/// [`record_lockfile_verified`] for a caller that goes on to run lifecycle
/// scripts, which records the returned value after them.
pub fn pending_lockfile_verified(
    cache_dir: Option<&Path>,
    lockfile_path: &Path,
    lockfile: &Lockfile,
    verifiers: &[Arc<dyn ResolutionVerifier>],
) -> Option<PendingVerificationRecord> {
    let cache_dir = cache_dir?;
    if verifiers.is_empty() || lockfile.packages.is_none() {
        return None;
    }
    Some(PendingVerificationRecord {
        cache_dir: cache_dir.to_path_buf(),
        lockfile_path: lockfile_path.to_path_buf(),
        verifiers: with_offline_check_cache_identities(verifiers),
        hash: hash_lockfile(lockfile),
        precomputed: CachePrecomputed::default(),
    })
}
