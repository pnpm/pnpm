//! Fan-out runner for the lockfile-verification gate.
//!
//! Walks every entry in `lockfile.packages`, dedupes by
//! `(name, version, resolution)`, and asks every active verifier to
//! evaluate each candidate. Verifiers handle their own protocol
//! short-circuit by returning [`ResolutionVerification::Ok`] for
//! resolutions outside their scope; the runner is policy-neutral and
//! dispatch-free at this layer.

pub use offline_checks::{
    RESOLUTION_SHAPE_CACHE_VERSION,
    RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE,
    collect_resolution_shape_violations,
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
        lockfile_verification_is_cached_by_hash,
        record_verification,
        try_lockfile_verification_cache,
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
    /// Absolute path of the lockfile being verified. Required for
    /// the on-disk verification cache (the stat shortcut + per-path
    /// index key off it) and surfaced in the
    /// `pnpm:lockfile-verification` reporter payload.
    pub lockfile_path: Option<&'a Path>,
    /// The on-disk cache directory. When set together with
    /// `lockfile_path`, a successful run is memoised in
    /// `<cache_dir>/lockfile-verified.jsonl` and the gate
    /// short-circuits on a repeat run against an unchanged lockfile
    /// (under the same or stricter policy). Omitting either field
    /// disables the cache (every call rehashes + reruns the gate).
    pub cache_dir: Option<&'a Path>,
    /// Entries the install re-resolves instead of reusing. See
    /// [`ReplacedEntries`].
    pub replaced: Option<ReplacedEntries<'a>>,
    /// The install repairs the lockfile (`--fix-lockfile`), which re-resolves
    /// an importer reference to a missing snapshot, so such a reference is
    /// not an error.
    pub repairs_lockfile: bool,
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
    /// lockfile at `lockfile_path` under `verifiers`.
    #[must_use]
    pub fn is_cached(
        self,
        cache_dir: &Path,
        lockfile_path: &Path,
        verifiers: &[Arc<dyn ResolutionVerifier>],
    ) -> bool {
        if self.0.packages.is_none() {
            return true;
        }
        try_lockfile_verification_cache(
            cache_dir,
            lockfile_path,
            &with_offline_check_cache_identities(verifiers),
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
    ) -> bool {
        if self.0.packages.is_none() {
            return true;
        }
        lockfile_verification_is_cached_by_hash(
            cache_dir,
            &hash_lockfile(self.0),
            &with_offline_check_cache_identities(verifiers),
        )
    }
}

/// Whether a recorded verification already covers `lockfile` as it sits
/// at `lockfile_path` under the policy `verifiers` currently demand —
/// i.e. whether a [`verify_lockfile_resolutions`] call would
/// short-circuit on the cache without running the fan-out. The offline
/// structural dependency-name gate runs here too (it is cheap and the
/// cache never covers a lockfile that fails it), so a `true` return
/// carries the same guarantee as the runner's cache hit.
///
/// Lets the pnpr client decide locally whether the server-side
/// `verify_lockfile` round trip can be skipped
/// ([pnpm/pnpm#13904](https://github.com/pnpm/pnpm/issues/13904)).
pub fn lockfile_verification_is_cached(
    cache_dir: &Path,
    lockfile_path: &Path,
    lockfile: &Lockfile,
    verifiers: &[Arc<dyn ResolutionVerifier>],
) -> bool {
    if verify_offline_structural_checks(lockfile).is_err() {
        return false;
    }
    if lockfile.packages.is_none() {
        return true;
    }
    try_lockfile_verification_cache(
        cache_dir,
        lockfile_path,
        &with_offline_check_cache_identities(verifiers),
        || hash_lockfile(lockfile),
    )
    .hit
}

/// Whether a recorded verification covers the exact in-memory
/// `lockfile` content under the currently active policy. Unlike
/// [`lockfile_verification_is_cached`], this never consults path or
/// filesystem metadata.
pub fn lockfile_verification_is_cached_by_content(
    cache_dir: &Path,
    lockfile: &Lockfile,
    verifiers: &[Arc<dyn ResolutionVerifier>],
) -> bool {
    if verify_offline_structural_checks(lockfile).is_err() {
        return false;
    }
    if lockfile.packages.is_none() {
        return true;
    }
    lockfile_verification_is_cached_by_hash(
        cache_dir,
        &hash_lockfile(lockfile),
        &with_offline_check_cache_identities(verifiers),
    )
}

/// Run every active [`ResolutionVerifier`] against every entry in
/// `lockfile.packages`.
pub async fn verify_lockfile_resolutions<Reporter: self::Reporter>(
    lockfile: &Lockfile,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    opts: &VerifyLockfileResolutionsOptions<'_>,
) -> Result<(), VerifyError> {
    // Offline structural gate first: reject invalid dependency names
    // or missing importer snapshot links before the `packages`-absent
    // short-circuit and the cache lookup.
    verify_structure_before_install(lockfile, opts)?;

    if lockfile.packages.is_none() {
        return Ok(());
    }

    let cache_inputs = opts.cache_dir.zip(opts.lockfile_path);

    let cache_verifiers = with_offline_check_cache_identities(verifiers);

    let mut hash_once = memoized_lockfile_hash(lockfile);

    let lockfile_path_str =
        opts.lockfile_path.map(Path::to_string_lossy).map(std::borrow::Cow::into_owned);

    let cache_precomputed = match reuse_cached_verdict::<Reporter>(
        cache_inputs,
        &cache_verifiers,
        &mut hash_once,
        CachedVerdict {
            has_policy_verifiers: !verifiers.is_empty(),
            lockfile_path: lockfile_path_str.as_ref(),
        },
    ) {
        CacheOutcome::Hit => return Ok(()),
        CacheOutcome::Miss(precomputed) => precomputed,
    };

    // Checked after the verification cache so a lockfile freshly resolved on
    // this machine by a local custom resolver (recorded via
    // `record_lockfile_verified`) can reinstall cleanly, while any uncached
    // lockfile or cacheless check rejects shape mismatches even with empty
    // `verifiers`.
    verify_lockfile_resolution_shapes(lockfile)?;
    if verifiers.is_empty() {
        return Ok(());
    }

    let CandidatesToVerify { candidates, skipped_replaced } =
        collect_candidates_to_verify(lockfile, opts.replaced);
    let cache_inputs = cache_inputs.filter(|_| !skipped_replaced);
    if candidates.is_empty() {
        // Persist the success so the next install can stat-only the
        // lockfile. An empty fan-out is still a successful run.
        record_verdict(cache_inputs, &cache_verifiers, &mut hash_once, cache_precomputed);
        return Ok(());
    }

    let violations =
        verify_candidates::<Reporter>(candidates, verifiers, opts.concurrency, lockfile_path_str)
            .await?;
    if violations.is_empty() {
        record_verdict(cache_inputs, &cache_verifiers, &mut hash_once, cache_precomputed);
        return Ok(());
    }
    Err(build_verification_error(violations))
}

struct CandidatesToVerify {
    candidates: Vec<Candidate>,
    skipped_replaced: bool,
}

/// The lockfile entries the policy verifiers check, after the offline
/// shape check passes. Entries `replaced` matches are left out.
fn collect_candidates_to_verify(
    lockfile: &Lockfile,
    replaced: Option<ReplacedEntries<'_>>,
) -> CandidatesToVerify {
    let mut candidates = collect_candidates(lockfile);
    let Some(ReplacedEntries(is_replaced)) = replaced else {
        return CandidatesToVerify { candidates, skipped_replaced: false };
    };
    let entries = candidates.len();
    candidates.retain(|candidate| {
        !is_replaced(ReplacedCandidate {
            name: &candidate.name,
            version: &candidate.version,
            registry_name: candidate.registry_name.as_deref(),
        })
    });
    let skipped_replaced = candidates.len() < entries;
    CandidatesToVerify { candidates, skipped_replaced }
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
    cache_inputs: Option<(&Path, &Path)>,
    cache_verifiers: &[Arc<dyn ResolutionVerifier>],
    hash_once: &mut impl FnMut() -> String,
    verdict: CachedVerdict<'_>,
) -> CacheOutcome {
    let Some((cache_dir, lockfile_path)) = cache_inputs else {
        return CacheOutcome::Miss(CachePrecomputed::default());
    };
    let result =
        try_lockfile_verification_cache(cache_dir, lockfile_path, cache_verifiers, hash_once);
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
    cache_inputs: Option<(&Path, &Path)>,
    cache_verifiers: &[Arc<dyn ResolutionVerifier>],
    hash_once: &mut impl FnMut() -> String,
    precomputed: CachePrecomputed,
) {
    if let Some((cache_dir, lockfile_path)) = cache_inputs {
        record_verification(cache_dir, lockfile_path, cache_verifiers, hash_once, precomputed);
    }
}

/// Collect-mode sibling of [`verify_lockfile_resolutions`] that
/// returns violations as data instead of throwing on the first batch.
/// No reporter emits, no cache wiring — for callers that need to
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
    // treat as "no violations" — see [`run_fan_out`].
    run_fan_out(collect_candidates(lockfile), verifiers, concurrency, &mut || {}).await
}

#[cfg(test)]
mod tests;

mod candidates;
mod offline_checks;
mod progress;

use candidates::{
    Candidate,
    build_verification_error,
    collect_candidates,
    run_fan_out,
};
use offline_checks::{
    verify_offline_structural_checks,
    verify_structure_before_install,
};
use progress::{
    VerificationProgress,
    emit,
};
