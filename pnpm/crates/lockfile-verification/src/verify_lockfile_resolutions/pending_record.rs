use std::{
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};

use pnpm_resolving_resolver_base::ResolutionVerifier;

use crate::{cache::CachePrecomputed, record_verification};

/// A verification that passed but has not been written to the log yet.
///
/// The log is read by the *next* install, and the log is last-writer-wins
/// per lockfile. An install's own dependency lifecycle scripts run before
/// it finishes and can append to the same file, so a verdict written ahead
/// of them is a verdict they can supersede. Holding the record back until
/// everything else is done keeps the install's verdict its last word on
/// the lockfile it verified.
///
/// This is ordering, not integrity: a lifecycle script runs with the same
/// privileges as pnpm and can rewrite the log — or the lockfile, or
/// `node_modules` — however it likes, and nothing marks a line in the log
/// as pnpm's. Whether such a script runs at all is what `allowBuilds`
/// governs.
///
/// Dropping the value without calling [`record`](Self::record) persists
/// nothing — an install that fails after verification leaves no verdict
/// behind.
#[must_use = "the verification is only persisted by calling record()"]
pub struct PendingVerificationRecord {
    pub(crate) cache_dir: PathBuf,
    pub(crate) lockfile_path: PathBuf,
    pub(crate) verifiers: Vec<Arc<dyn ResolutionVerifier>>,
    pub(crate) hash: String,
    pub(crate) precomputed: CachePrecomputed,
}

/// Hand-written because the verifiers are trait objects; the fields that
/// identify *which* verification this is are the interesting ones anyway.
impl fmt::Debug for PendingVerificationRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingVerificationRecord")
            .field("cache_dir", &self.cache_dir)
            .field("lockfile_path", &self.lockfile_path)
            .field("hash", &self.hash)
            .field("verifiers", &self.verifiers.len())
            .finish_non_exhaustive()
    }
}

impl PendingVerificationRecord {
    /// Append the verdict to the log. Call once the install has finished
    /// every dependency lifecycle script it runs.
    pub fn record(self) {
        record_verification(
            &self.cache_dir,
            &self.lockfile_path,
            &self.verifiers,
            || self.hash.clone(),
            self.precomputed,
        );
    }
}

/// Capture what [`PendingVerificationRecord::record`] needs, or `None`
/// when the caller runs without a cache (tests, and callers that pass
/// no `cache_dir`/`lockfile_path`).
pub(super) fn pending_record(
    cache_inputs: Option<(&Path, &Path)>,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    hash_once: &mut impl FnMut() -> String,
    precomputed: CachePrecomputed,
) -> Option<PendingVerificationRecord> {
    let (cache_dir, lockfile_path) = cache_inputs?;
    Some(PendingVerificationRecord {
        cache_dir: cache_dir.to_path_buf(),
        lockfile_path: lockfile_path.to_path_buf(),
        verifiers: verifiers.to_vec(),
        hash: hash_once(),
        precomputed,
    })
}
