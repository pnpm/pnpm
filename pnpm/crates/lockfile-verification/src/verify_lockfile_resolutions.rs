//! Fan-out runner for the lockfile-verification gate.
//!
//! Walks every entry in `lockfile.packages`, dedupes by
//! `(name, version, resolution)`, and asks every active verifier to
//! evaluate each candidate. Verifiers handle their own protocol
//! short-circuit by returning [`ResolutionVerification::Ok`] for
//! resolutions outside their scope; the runner is policy-neutral and
//! dispatch-free at this layer.
//!
//! [`ResolutionVerification::Ok`]: pnpm_resolving_resolver_base::ResolutionVerification::Ok

pub(crate) use cache_verdict::with_offline_check_cache_identities;
pub use cache_verdict::{
    lockfile_verification_is_cached, lockfile_verification_is_cached_by_content,
};
pub use candidates::RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE;
pub use dependency_names::verify_lockfile_dependency_names;

use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use pnpm_lockfile::{Lockfile, PkgName};
use pnpm_reporter::{LockfileVerificationMessage, LogLevel, Reporter};
use pnpm_resolving_resolver_base::{ResolutionPolicyViolation, ResolutionVerifier};

use crate::{cache::CachePrecomputed, errors::VerifyError};

use cache_verdict::{
    CacheOutcome, CachedVerdict, memoized_lockfile_hash, record_verdict, reuse_cached_verdict,
};
use candidates::{
    Candidate, build_verification_error, collect_candidates, collect_candidates_to_verify,
    run_fan_out,
};
use progress::{TerminalEmitGuard, emit, progress_reporter};

mod cache_verdict;
mod candidates;
mod dependency_names;
mod progress;

#[cfg(test)]
mod tests;

/// Default concurrency cap for the per-candidate fan-out: `64`, the
/// floor of the `package-requester` network-concurrency formula.
const DEFAULT_CONCURRENCY: usize = 64;

/// Options bundle for [`verify_lockfile_resolutions`].
#[derive(Debug, Default, Clone)]
pub struct VerifyLockfileResolutionsOptions<'a> {
    /// Cap on concurrent verifier futures. `None` falls back to
    /// the internal `DEFAULT_CONCURRENCY` (`64`).
    pub concurrency: Option<usize>,
    /// Absolute path of the lockfile being verified.
    pub lockfile_path: Option<&'a Path>,
    /// The on-disk cache directory.
    pub cache_dir: Option<&'a Path>,
    /// Entries the install re-resolves instead of reusing.
    pub replaced: Option<ReplacedEntries<'a>>,
}

/// Matches the lockfile entries, by name and version, that the install
/// re-resolves instead of reusing.
#[derive(Clone, Copy)]
pub struct ReplacedEntries<'a>(pub &'a (dyn Fn(&PkgName, &str) -> bool + Send + Sync));

impl std::fmt::Debug for ReplacedEntries<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReplacedEntries(..)")
    }
}

/// Run every active [`ResolutionVerifier`] against every entry in
/// `lockfile.packages`.
pub async fn verify_lockfile_resolutions<Reporter: self::Reporter>(
    lockfile: &Lockfile,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    opts: &VerifyLockfileResolutionsOptions<'_>,
) -> Result<(), VerifyError> {
    verify_lockfile_dependency_names(lockfile)?;
    if lockfile.packages.is_none() {
        return Ok(());
    }

    let cache_inputs = opts.cache_dir.zip(opts.lockfile_path);
    let cache_verifiers = with_offline_check_cache_identities(verifiers);
    let mut hash_once = memoized_lockfile_hash(lockfile);
    let path_str = opts.lockfile_path.map(Path::to_string_lossy).map(std::borrow::Cow::into_owned);

    let Some(precomputed) = check_verification_cache::<Reporter>(
        cache_inputs,
        &cache_verifiers,
        &mut hash_once,
        !verifiers.is_empty(),
        path_str.as_ref(),
    ) else {
        return Ok(());
    };

    let (candidates, skipped_replaced) = collect_candidates_to_verify(lockfile, opts.replaced)?;
    if verifiers.is_empty() {
        return Ok(());
    }
    execute_verification::<Reporter>(candidates, verifiers, opts.concurrency, path_str).await?;
    let cache_inputs = cache_inputs.filter(|_| !skipped_replaced);
    record_verdict(cache_inputs, &cache_verifiers, &mut hash_once, precomputed);
    Ok(())
}

fn check_verification_cache<Reporter: self::Reporter>(
    cache_inputs: Option<(&Path, &Path)>,
    cache_verifiers: &[Arc<dyn ResolutionVerifier>],
    hash_once: &mut impl FnMut() -> String,
    has_policy_verifiers: bool,
    lockfile_path_str: Option<&String>,
) -> Option<CachePrecomputed> {
    match reuse_cached_verdict::<Reporter>(
        cache_inputs,
        cache_verifiers,
        hash_once,
        CachedVerdict { has_policy_verifiers, lockfile_path: lockfile_path_str },
    ) {
        CacheOutcome::Hit => None,
        CacheOutcome::Miss(precomputed) => Some(precomputed),
    }
}

async fn execute_verification<Reporter: self::Reporter>(
    candidates: Vec<Candidate>,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    concurrency: Option<usize>,
    lockfile_path_str: Option<String>,
) -> Result<(), VerifyError> {
    if candidates.is_empty() {
        return Ok(());
    }

    let violations =
        verify_candidates::<Reporter>(candidates, verifiers, concurrency, lockfile_path_str).await?;
    if violations.is_empty() {
        return Ok(());
    }
    Err(build_verification_error(violations))
}

async fn verify_candidates<Reporter: self::Reporter>(
    candidates: Vec<Candidate>,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    concurrency: Option<usize>,
    lockfile_path_str: Option<String>,
) -> Result<Vec<ResolutionPolicyViolation>, VerifyError> {
    let entries = candidates.len() as u64;
    let started_at = Instant::now();
    emit::<Reporter>(
        LogLevel::Debug,
        LockfileVerificationMessage::Started { entries, lockfile_path: lockfile_path_str.clone() },
    );

    let observed_checked = AtomicU64::new(0u64);
    let mut emit_guard = TerminalEmitGuard::<Reporter>::failed(
        entries,
        started_at,
        lockfile_path_str.clone(),
        Some(&observed_checked),
    );

    let mut on_entry_checked = progress_reporter::<Reporter>(
        entries,
        started_at,
        lockfile_path_str.clone(),
        &observed_checked,
    );

    let fan_out_result =
        run_fan_out(candidates, verifiers, concurrency, Some(&mut on_entry_checked)).await;
    finish_candidates_verification(
        fan_out_result,
        entries,
        started_at,
        lockfile_path_str,
        &observed_checked,
        &mut emit_guard,
    )
}

fn finish_candidates_verification<Reporter: self::Reporter>(
    fan_out_result: Result<Vec<ResolutionPolicyViolation>, String>,
    entries: u64,
    started_at: Instant,
    lockfile_path_str: Option<String>,
    observed_checked: &AtomicU64,
    emit_guard: &mut TerminalEmitGuard<'_, Reporter>,
) -> Result<Vec<ResolutionPolicyViolation>, VerifyError> {
    let violations = match fan_out_result {
        Ok(violations) => violations,
        Err(message) => {
            emit_guard.fail(entries, observed_checked.load(Ordering::Relaxed), lockfile_path_str);
            return Err(VerifyError::RegistryMetaFetchFailed { message });
        }
    };
    if violations.is_empty() {
        emit_guard.cancel(LockfileVerificationMessage::Done {
            entries,
            checked: entries,
            elapsed_ms: started_at.elapsed().as_millis() as u64,
            lockfile_path: lockfile_path_str,
        });
    } else {
        emit_guard.fail(entries, entries, lockfile_path_str);
    }
    Ok(violations)
}

/// Collect-mode sibling of [`verify_lockfile_resolutions`] that
/// returns violations as data instead of throwing on the first batch.
pub async fn collect_resolution_policy_violations(
    lockfile: &Lockfile,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    concurrency: Option<usize>,
) -> Result<Vec<ResolutionPolicyViolation>, String> {
    if verifiers.is_empty() || lockfile.packages.is_none() {
        return Ok(Vec::new());
    }
    let (candidates, _shape_violations) = collect_candidates(lockfile);
    run_fan_out(candidates, verifiers, concurrency, None).await
}
