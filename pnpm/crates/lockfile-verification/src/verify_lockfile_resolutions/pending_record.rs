use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use pnpm_resolving_resolver_base::ResolutionVerifier;

use crate::cache::{CachePrecomputed, CacheRecord, build_verification_record, persist_record};

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
///
/// The record is built when the verification passes, so it carries the
/// lockfile's stat and hash as they were then. A script that rewrites the
/// lockfile before [`record`](Self::record) cannot get its version keyed
/// to this verdict.
#[must_use = "the verification is only persisted by calling record()"]
#[derive(Debug)]
pub struct PendingVerificationRecord {
    cache_dir: PathBuf,
    record: CacheRecord,
}

impl PendingVerificationRecord {
    /// Append the verdict to the log. Call once the install has finished
    /// every dependency lifecycle script it runs.
    pub fn record(self) {
        persist_record(&self.cache_dir, &self.record);
    }

    pub(crate) fn new(cache_dir: &Path, record: CacheRecord) -> Self {
        Self { cache_dir: cache_dir.to_path_buf(), record }
    }
}

/// Capture what [`PendingVerificationRecord::record`] writes, or `None`
/// when the caller runs without a cache (tests, and callers that pass
/// no `cache_dir`/`lockfile_path`), or when the cache lookup could not
/// stat the lockfile.
pub(super) fn pending_record(
    cache_inputs: Option<(&Path, &Path)>,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    hash_once: &mut impl FnMut() -> String,
    precomputed: CachePrecomputed,
) -> Option<PendingVerificationRecord> {
    let (cache_dir, lockfile_path) = cache_inputs?;
    let record = build_verification_record(
        lockfile_path,
        verifiers,
        hash_once,
        precomputed.stat,
        precomputed.hash,
    )?;
    Some(PendingVerificationRecord::new(cache_dir, record))
}
