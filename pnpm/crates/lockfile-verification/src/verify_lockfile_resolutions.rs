//! Fan-out runner for the lockfile-verification gate.
//!
//! Walks every entry in `lockfile.packages`, dedupes by
//! `(name, version, resolution)`, and asks every active verifier to
//! evaluate each candidate. Verifiers handle their own protocol
//! short-circuit by returning [`ResolutionVerification::Ok`] for
//! resolutions outside their scope; the runner is policy-neutral and
//! dispatch-free at this layer.

pub use exotic_subdeps::{
    EXOTIC_SUBDEP_VIOLATION_CODE,
    ExoticSubdepsPolicy,
    ExoticSubdepsScope,
};
pub(crate) use offline_checks::with_offline_and_exotic_cache_identities;
pub use offline_checks::{
    RESOLUTION_SHAPE_CACHE_VERSION,
    RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE,
    can_trust_prepared_policy,
    collect_resolution_shape_violations,
    snapshot_verification_policy,
    verify_lockfile_dependency_names,
    verify_lockfile_importer_snapshot_links,
    verify_lockfile_resolution_shapes,
    with_offline_check_cache_identities,
};

use std::{
    collections::BTreeMap,
    path::Path,
    sync::Arc,
};

use futures_util::{
    StreamExt,
    stream::FuturesUnordered,
};
use pnpm_lockfile::{
    Lockfile,
    LockfileResolution,
    PkgName,
};
use pnpm_reporter::{
    LockfileVerificationMessage,
    LogLevel,
    Reporter,
};
use pnpm_resolving_resolver_base::{
    ResolutionPolicyViolation,
    ResolutionVerification,
    ResolutionVerifier,
    VerifyCtx,
};
use tokio::sync::Semaphore;

use crate::{
    cache::{
        CachePrecomputed,
        LockfileStat,
        lockfile_verification_is_cached_by_hash,
        record_verification,
        record_verification_by_hash,
        try_lockfile_verification_cache,
        try_lockfile_verification_cache_by_hash,
        try_lockfile_verification_cache_with_stat,
    },
    errors::{
        RenderedViolation,
        VerifyError,
    },
    hash_lockfile,
};

/// Default concurrency cap for the per-candidate fan-out: `64`, the
/// floor of the `package-requester` network-concurrency formula.
const DEFAULT_CONCURRENCY: usize = 64;

/// Options bundle for [`verify_lockfile_resolutions`].
#[derive(Debug, Default, Clone)]
pub struct VerifyLockfileResolutionsOptions<'a> {
    /// Cap on concurrent verifier futures. `None` falls back to
    /// an internal default of `64`.
    pub concurrency: Option<usize>,
    /// Absolute path of the lockfile being verified. Surfaced in the
    /// `pnpm:lockfile-verification` reporter payload, and enables the
    /// on-disk stat shortcut when paired with [`Self::lockfile_stat`].
    pub lockfile_path: Option<&'a Path>,
    /// Metadata captured from the open file handle when `lockfile` was
    /// loaded. When `None`, cache lookup and recording use the content-only
    /// index rather than statting `lockfile_path` after the fact.
    pub lockfile_stat: Option<LockfileStat>,
    /// The on-disk cache directory. When set, a successful run is
    /// memoised in `<cache_dir>/lockfile-verified.jsonl` (by stat +
    /// content hash when `lockfile_path` is set, or by content hash
    /// alone when `lockfile_path` is `None`) and the gate
    /// short-circuits on a repeat run against an unchanged lockfile
    /// under the same or stricter policy.
    pub cache_dir: Option<&'a Path>,
    /// Entries the install re-resolves instead of reusing. See
    /// [`ReplacedEntries`].
    pub replaced: Option<ReplacedEntries<'a>>,
    /// The install repairs the lockfile (`--fix-lockfile`), which re-resolves
    /// an importer reference to a missing snapshot, so such a reference is
    /// not an error.
    pub repairs_lockfile: bool,
    /// Whether `blockExoticSubdeps` is enforced on lockfile subdependencies.
    pub exotic_subdeps: ExoticSubdepsPolicy,
}

/// One deduplicated lockfile entry offered to [`ReplacedEntries`].
#[derive(Debug, Clone, Copy)]
pub struct ReplacedCandidate<'a> {
    pub name: &'a PkgName,
    pub version: &'a str,
    pub registry_name: Option<&'a str>,
}

/// Matches the lockfile entries that the install re-resolves instead of
/// reusing, such as the targets of `pnpm update <pkg>` or plain keys whose
/// scope route moved. The resolver applies the policies to whatever it
/// picks for them, so the verifiers skip their locked versions, which the
/// registry may no longer serve. The offline shape and alias checks still
/// cover them. A run that skips an entry does not record the lockfile as
/// verified.
#[derive(Clone, Copy)]
pub struct ReplacedEntries<'a>(pub &'a (dyn Fn(ReplacedCandidate<'_>) -> bool + Send + Sync));

impl std::fmt::Debug for ReplacedEntries<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReplacedEntries(..)")
    }
}

/// Proof that `lockfile` passed offline dependency-name, importer-snapshot
/// link (except in [`Self::verify_for_repair`]), and resolution-shape
/// validation, allowing a subsequent verification-cache lookup to skip
/// repeating those scans.
#[derive(Debug, Clone, Copy)]
pub struct VerifiedOfflineLockfile<'a>(&'a Lockfile);

impl<'a> VerifiedOfflineLockfile<'a> {
    /// Validate offline dependency names, importer-snapshot links, and
    /// resolution shapes of `lockfile`.
    pub fn verify(lockfile: &'a Lockfile) -> Result<Self, VerifyError> {
        verify_offline_structural_checks(lockfile)?;
        verify_lockfile_resolution_shapes(lockfile)?;
        Ok(Self(lockfile))
    }

    /// Validate offline dependency names and resolution shapes of `lockfile`
    /// while allowing missing importer-snapshot links that `--fix-lockfile`
    /// re-resolves.
    pub fn verify_for_repair(lockfile: &'a Lockfile) -> Result<Self, VerifyError> {
        verify_lockfile_dependency_names(lockfile)?;
        verify_lockfile_resolution_shapes(lockfile)?;
        Ok(Self(lockfile))
    }

    /// Whether `lockfile-verified.jsonl` already covers this offline-verified
    /// lockfile at `lockfile_path` under `verifiers` and the given [`ExoticSubdepsPolicy`].
    #[must_use]
    pub fn is_cached(
        self,
        cache_dir: &Path,
        lockfile_path: &Path,
        verifiers: &[Arc<dyn ResolutionVerifier>],
        exotic_subdeps: &ExoticSubdepsPolicy,
    ) -> bool {
        if self.0.packages.is_none() {
            return packages_absent_is_clean(self.0, exotic_subdeps);
        }
        try_lockfile_verification_cache(
            cache_dir,
            lockfile_path,
            &with_offline_and_exotic_cache_identities(verifiers, exotic_subdeps),
            || hash_lockfile(self.0),
        )
        .hit
    }

    /// [`Self::is_cached`] using a pre-captured [`LockfileStat`].
    #[must_use]
    pub fn is_cached_with_stat(
        self,
        cache_dir: &Path,
        lockfile_path: &Path,
        stat: LockfileStat,
        verifiers: &[Arc<dyn ResolutionVerifier>],
        exotic_subdeps: &ExoticSubdepsPolicy,
    ) -> bool {
        if self.0.packages.is_none() {
            return packages_absent_is_clean(self.0, exotic_subdeps);
        }
        try_lockfile_verification_cache_with_stat(
            cache_dir,
            lockfile_path,
            stat,
            &with_offline_and_exotic_cache_identities(verifiers, exotic_subdeps),
            || hash_lockfile(self.0),
        )
        .hit
    }

    /// Content-only counterpart of [`Self::is_cached`].
    #[must_use]
    pub fn is_cached_by_content(
        self,
        cache_dir: &Path,
        verifiers: &[Arc<dyn ResolutionVerifier>],
        exotic_subdeps: &ExoticSubdepsPolicy,
    ) -> bool {
        if self.0.packages.is_none() {
            return packages_absent_is_clean(self.0, exotic_subdeps);
        }
        lockfile_verification_is_cached_by_hash(
            cache_dir,
            &hash_lockfile(self.0),
            &with_offline_and_exotic_cache_identities(verifiers, exotic_subdeps),
        )
    }
}

fn packages_absent_is_clean(lockfile: &Lockfile, exotic_subdeps: &ExoticSubdepsPolicy) -> bool {
    match exotic_subdeps.scope() {
        None => true,
        Some(scope) => {
            collect_exotic_subdep_violations(lockfile, None, scope).violations.is_empty()
        }
    }
}

/// Whether a recorded verification already covers `lockfile` as it sits
/// at `lockfile_path` under the policy `verifiers` currently demand.
pub fn lockfile_verification_is_cached(
    cache_dir: &Path,
    lockfile_path: &Path,
    lockfile: &Lockfile,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    exotic_subdeps: &ExoticSubdepsPolicy,
) -> bool {
    let Ok(verified) = VerifiedOfflineLockfile::verify(lockfile) else {
        return false;
    };
    verified.is_cached(cache_dir, lockfile_path, verifiers, exotic_subdeps)
}

/// [`lockfile_verification_is_cached`] using a [`LockfileStat`] captured when
/// the lockfile was read rather than re-statting `lockfile_path`.
pub fn lockfile_verification_is_cached_with_stat(
    cache_dir: &Path,
    lockfile_path: &Path,
    stat: LockfileStat,
    lockfile: &Lockfile,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    exotic_subdeps: &ExoticSubdepsPolicy,
) -> bool {
    let Ok(verified) = VerifiedOfflineLockfile::verify(lockfile) else {
        return false;
    };
    verified.is_cached_with_stat(cache_dir, lockfile_path, stat, verifiers, exotic_subdeps)
}

/// Whether a recorded verification covers the exact in-memory
/// `lockfile` content under the currently active policy.
pub fn lockfile_verification_is_cached_by_content(
    cache_dir: &Path,
    lockfile: &Lockfile,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    exotic_subdeps: &ExoticSubdepsPolicy,
) -> bool {
    let Ok(verified) = VerifiedOfflineLockfile::verify(lockfile) else {
        return false;
    };
    verified.is_cached_by_content(cache_dir, verifiers, exotic_subdeps)
}

/// Run every active [`ResolutionVerifier`] against every entry in
/// `lockfile.packages`.
pub async fn verify_lockfile_resolutions<Reporter: self::Reporter>(
    lockfile: &Lockfile,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    opts: &VerifyLockfileResolutionsOptions<'_>,
) -> Result<(), VerifyError> {
    verify_structure_before_install(lockfile, opts)?;
    if lockfile.packages.is_none() {
        return verify_packages_absent(lockfile, opts);
    }
    let has_policy_verifiers = !verifiers.is_empty() || opts.exotic_subdeps.is_enforced();
    let cache_target = resolve_cache_target(opts, verifiers);
    let cache_verifiers = with_offline_and_exotic_cache_identities(verifiers, &opts.exotic_subdeps);
    let mut hash_once = memoized_lockfile_hash(lockfile);
    let lockfile_path_str =
        opts.lockfile_path.map(Path::to_string_lossy).map(std::borrow::Cow::into_owned);

    let CacheOutcome::Miss(cache_precomputed) = reuse_cached_verdict::<Reporter>(
        cache_target,
        &cache_verifiers,
        &mut hash_once,
        CachedVerdict { has_policy_verifiers, lockfile_path: lockfile_path_str.as_ref() },
    ) else {
        return Ok(());
    };

    // Checked after the verification cache so a lockfile freshly resolved on
    // this machine by a local custom resolver (recorded via
    // `record_lockfile_verified_by_content`) can reinstall cleanly, while any
    // uncached lockfile or cacheless check rejects shape mismatches even with
    // empty `verifiers`.
    verify_lockfile_resolution_shapes(lockfile)?;
    if !has_policy_verifiers {
        return Ok(());
    }

    let CollectedCandidates { candidates, skipped_replaced } =
        collect_candidates_to_verify(lockfile, opts.replaced, &opts.exotic_subdeps)?;
    let cache_target = cache_target.filter(|_| !skipped_replaced);
    if verifiers.is_empty() || candidates.is_empty() {
        record_verdict(cache_target, &cache_verifiers, &mut hash_once, cache_precomputed);
        return Ok(());
    }

    let violations =
        verify_candidates::<Reporter>(candidates, verifiers, opts.concurrency, lockfile_path_str)
            .await?;
    if violations.is_empty() {
        record_verdict(cache_target, &cache_verifiers, &mut hash_once, cache_precomputed);
        return Ok(());
    }
    Err(build_verification_error(violations))
}

fn resolve_cache_target<'a>(
    opts: &'a VerifyLockfileResolutionsOptions<'a>,
    verifiers: &[Arc<dyn ResolutionVerifier>],
) -> Option<(&'a Path, Option<(&'a Path, &'a LockfileStat)>)> {
    let stat_source = opts.lockfile_path.zip(opts.lockfile_stat.as_ref());
    let has_policy_verifiers = !verifiers.is_empty() || opts.exotic_subdeps.is_enforced();
    opts.cache_dir
        .filter(|_| stat_source.is_some() || has_policy_verifiers)
        .map(|cache_dir| (cache_dir, stat_source))
}

fn verify_packages_absent(
    lockfile: &Lockfile,
    opts: &VerifyLockfileResolutionsOptions<'_>,
) -> Result<(), VerifyError> {
    if let Some(scope) = opts.exotic_subdeps.scope() {
        let scan = collect_exotic_subdep_violations(lockfile, opts.replaced, scope);
        if !scan.violations.is_empty() {
            return Err(build_verification_error(scan.violations));
        }
    }
    Ok(())
}

struct CollectedCandidates {
    candidates: Vec<Candidate>,
    skipped_replaced: bool,
}

/// The lockfile entries the policy verifiers check, after the offline
/// shape and exotic-subdep checks pass. Entries `replaced` matches are
/// left out, and `skipped_replaced` tells whether any was.
fn collect_candidates_to_verify(
    lockfile: &Lockfile,
    replaced: Option<ReplacedEntries<'_>>,
    exotic_subdeps: &ExoticSubdepsPolicy,
) -> Result<CollectedCandidates, VerifyError> {
    let mut skipped_replaced = false;
    if let Some(scope) = exotic_subdeps.scope() {
        let scan = collect_exotic_subdep_violations(lockfile, replaced, scope);
        if !scan.violations.is_empty() {
            return Err(build_verification_error(scan.violations));
        }
        skipped_replaced |= scan.skipped_replaced;
    }
    let mut candidates = collect_candidates(lockfile);
    let Some(ReplacedEntries(is_replaced)) = replaced else {
        return Ok(CollectedCandidates { candidates, skipped_replaced });
    };
    let entries = candidates.len();
    candidates.retain(|candidate| {
        !is_replaced(ReplacedCandidate {
            name: &candidate.name,
            version: &candidate.version,
            registry_name: candidate.registry_name.as_deref(),
        })
    });
    skipped_replaced |= candidates.len() < entries;
    Ok(CollectedCandidates { candidates, skipped_replaced })
}

/// Run the verifiers over every candidate, reporting the run to the
/// reporter as a [`VerificationProgress`].
async fn verify_candidates<Reporter: self::Reporter>(
    candidates: Vec<Candidate>,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    concurrency: Option<usize>,
    lockfile_path_str: Option<String>,
) -> Result<Vec<ResolutionPolicyViolation>, VerifyError> {
    let mut progress =
        VerificationProgress::<Reporter>::start(candidates.len() as u64, lockfile_path_str);
    let violations =
        run_fan_out(candidates, verifiers, concurrency, &mut || progress.entry_checked())
            .await
            .map_err(|message| VerifyError::RegistryMetaFetchFailed { message })?;
    if violations.is_empty() {
        progress.pass();
    }
    Ok(violations)
}

/// What the verification cache had to say about this lockfile.
enum CacheOutcome {
    /// A previous run already verified it under the same identities.
    Hit,
    /// Nothing reusable; carries what the recorder need not recompute.
    Miss(CachePrecomputed),
}

/// What a cache lookup needs to report a hit.
#[derive(Clone, Copy)]
struct CachedVerdict<'a> {
    /// Whether any policy verifier is active. The shape-only run that every
    /// install performs reports nothing.
    has_policy_verifiers: bool,
    lockfile_path: Option<&'a String>,
}

/// Share the lazy hash between cache lookup and recording. A stat-only
/// lookup never computes it.
fn memoized_lockfile_hash(lockfile: &Lockfile) -> impl FnMut() -> String + '_ {
    let mut cached_hash: Option<String> = None;
    move || cached_hash.get_or_insert_with(|| hash_lockfile(lockfile)).clone()
}

/// Reuse a previous verdict for this lockfile, if the cache holds one.
///
/// A silent short-circuit would look like the policy gate never ran, so a
/// reused verdict is surfaced.
fn reuse_cached_verdict<Reporter: self::Reporter>(
    cache_target: Option<(&Path, Option<(&Path, &LockfileStat)>)>,
    cache_verifiers: &[Arc<dyn ResolutionVerifier>],
    hash_once: &mut impl FnMut() -> String,
    verdict: CachedVerdict<'_>,
) -> CacheOutcome {
    let Some((cache_dir, stat_source)) = cache_target else {
        return CacheOutcome::Miss(CachePrecomputed::default());
    };
    let result = match stat_source {
        Some((lockfile_path, stat)) => try_lockfile_verification_cache_with_stat(
            cache_dir,
            lockfile_path,
            stat.clone(),
            cache_verifiers,
            hash_once,
        ),
        None => try_lockfile_verification_cache_by_hash(cache_dir, cache_verifiers, hash_once),
    };
    if !result.hit {
        return CacheOutcome::Miss(result.precomputed);
    }
    if verdict.has_policy_verifiers {
        emit::<Reporter>(
            LogLevel::Debug,
            LockfileVerificationMessage::Cached {
                verified_at: result.verified_at,
                lockfile_path: verdict.lockfile_path.cloned(),
            },
        );
    }
    CacheOutcome::Hit
}

/// Persist a successful verification, when caching is wired up.
fn record_verdict(
    cache_target: Option<(&Path, Option<(&Path, &LockfileStat)>)>,
    cache_verifiers: &[Arc<dyn ResolutionVerifier>],
    hash_once: &mut impl FnMut() -> String,
    precomputed: CachePrecomputed,
) {
    match cache_target {
        Some((cache_dir, Some((lockfile_path, _)))) => {
            record_verification(cache_dir, lockfile_path, cache_verifiers, hash_once, precomputed);
        }
        Some((cache_dir, None)) => {
            record_verification_by_hash(cache_dir, cache_verifiers, hash_once, precomputed);
        }
        None => {}
    }
}

/// Collect-mode sibling of [`verify_lockfile_resolutions`] that
/// returns violations as data instead of throwing on the first batch.
/// No reporter emits, no cache wiring, for callers that need to
/// inspect violations.
pub async fn collect_resolution_policy_violations(
    lockfile: &Lockfile,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    concurrency: Option<usize>,
) -> Result<Vec<ResolutionPolicyViolation>, String> {
    if lockfile.packages.is_none() {
        return Ok(Vec::new());
    }
    let shape_violations = collect_resolution_shape_violations(lockfile);
    if !shape_violations.is_empty() || verifiers.is_empty() {
        return Ok(shape_violations);
    }
    // `Err(message)` is a transport failure the caller must surface rather than
    // treat as "no violations"; see [`run_fan_out`].
    run_fan_out(collect_candidates(lockfile), verifiers, concurrency, &mut || {}).await
}

#[cfg(test)]
mod tests;

mod candidates;
mod exotic_subdeps;
mod offline_checks;
mod progress;

use candidates::{
    Candidate,
    build_verification_error,
    collect_candidates,
    run_fan_out,
};
use exotic_subdeps::collect_exotic_subdep_violations;
use offline_checks::{
    verify_offline_structural_checks,
    verify_structure_before_install,
};
use progress::{
    VerificationProgress,
    emit,
};
