//! Thin wrapper around [`record_verification`] for callers that hold
//! a freshly-written lockfile.
//!
//! After resolution writes a new lockfile, the install path uses this
//! wrapper to mark the new lockfile as already-verified so the
//! *next* install can take the cache fast path. Skipping the gate on
//! the next run is safe: fresh local picks went through the
//! resolver's per-version filter, and any carried-over entries
//! already passed the gate at the top of the same install.
//!
//! The function is a no-op when caching is disabled, when no
//! verifiers are active, when the lockfile has no `packages:`
//! section, or when the lockfile fails the offline dependency-name
//! check.

use std::{
    path::Path,
    sync::Arc,
};

use pnpm_lockfile::Lockfile;
use pnpm_resolving_resolver_base::ResolutionVerifier;

use crate::{
    ExoticSubdepsPolicy,
    cache::{
        LockfileStat,
        record_verification,
        record_verification_by_hash,
    },
    hash_lockfile,
    verify_lockfile_resolutions::{
        verify_lockfile_dependency_names,
        with_offline_and_exotic_cache_identities,
    },
};

#[cfg(test)]
mod tests;

/// Persist a verified lockfile using [`LockfileStat`] captured when the
/// lockfile was originally read, avoiding any post-verification `stat` race.
pub fn record_lockfile_verified_with_stat(
    cache_dir: Option<&Path>,
    lockfile_path: &Path,
    stat: LockfileStat,
    lockfile: &Lockfile,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    exotic_subdeps: &ExoticSubdepsPolicy,
) {
    let Some(cache_dir) = cache_dir else { return };
    if !can_record_lockfile(lockfile, verifiers, exotic_subdeps) {
        return;
    }
    record_verification(
        cache_dir,
        lockfile_path,
        &with_offline_and_exotic_cache_identities(verifiers, exotic_subdeps),
        || hash_lockfile(lockfile),
        crate::cache::CachePrecomputed { stat: Some(stat), hash: None },
    );
}

/// Persist a lockfile this process just saved to `saved_path`, keyed by that
/// file's path and stat. The caller guarantees the file holds `lockfile`.
///
/// A content-only record is not enough here: a lockfile format that omits
/// fields, such as v13 dropping `deprecated`, reads back with a different
/// content hash than the in-memory lockfile. Falls back to the content hash
/// when the file cannot be stat'ed.
pub fn record_saved_lockfile_verified(
    cache_dir: Option<&Path>,
    saved_path: &Path,
    lockfile: &Lockfile,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    exotic_subdeps: &ExoticSubdepsPolicy,
) {
    match std::fs::metadata(saved_path) {
        Ok(metadata) => record_lockfile_verified_with_stat(
            cache_dir,
            saved_path,
            crate::stat_from_metadata(&metadata),
            lockfile,
            verifiers,
            exotic_subdeps,
        ),
        Err(_) => {
            record_lockfile_verified_by_content(cache_dir, lockfile, verifiers, exotic_subdeps);
        }
    }
}

/// Persist the post-resolution or in-memory lockfile as already-verified by
/// content hash.
///
/// Post-save callers do not hold metadata guaranteed to come from the exact
/// written lockfile (a concurrent install may have replaced `lockfile_path`
/// before this runs), so the verdict is recorded by content hash alone;
/// the next install reading the unchanged file refreshes the per-path stat
/// shortcut from its read-time [`LockfileStat`].
pub fn record_lockfile_verified_by_content(
    cache_dir: Option<&Path>,
    lockfile: &Lockfile,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    exotic_subdeps: &ExoticSubdepsPolicy,
) {
    let Some(cache_dir) = cache_dir else { return };
    if !can_record_lockfile(lockfile, verifiers, exotic_subdeps) {
        return;
    }
    record_verification_by_hash(
        cache_dir,
        &with_offline_and_exotic_cache_identities(verifiers, exotic_subdeps),
        || hash_lockfile(lockfile),
        crate::cache::CachePrecomputed::default(),
    );
}

fn can_record_lockfile(
    lockfile: &Lockfile,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    exotic_subdeps: &ExoticSubdepsPolicy,
) -> bool {
    (!verifiers.is_empty() || exotic_subdeps.is_enforced())
        && lockfile.packages.is_some()
        && verify_lockfile_dependency_names(lockfile).is_ok()
}
