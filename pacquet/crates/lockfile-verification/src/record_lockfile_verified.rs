//! Thin wrapper around [`record_verification`] for callers that hold
//! a freshly-written lockfile.
//!
//! Mirrors pnpm's
//! [`recordLockfileVerified.ts`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/installing/deps-installer/src/install/recordLockfileVerified.ts).
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
//! section — same guards upstream uses.

use std::{path::Path, sync::Arc};

use pacquet_lockfile::Lockfile;
use pacquet_resolving_resolver_base::ResolutionVerifier;

use crate::{
    cache::record_verification, hash_lockfile,
    verify_lockfile_resolutions::with_resolution_shape_cache_identity,
};

/// Persist the post-resolution lockfile as already-verified.
/// Inputs match upstream's `RecordLockfileVerifiedOptions`:
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
    let Some(cache_dir) = cache_dir else { return };
    if verifiers.is_empty() {
        return;
    }
    if lockfile.packages.is_none() {
        return;
    }
    record_verification(
        cache_dir,
        lockfile_path,
        &with_resolution_shape_cache_identity(verifiers),
        || hash_lockfile(lockfile),
        crate::cache::CachePrecomputed::default(),
    );
}
