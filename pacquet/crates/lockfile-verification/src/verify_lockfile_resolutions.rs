//! Fan-out runner for the lockfile-verification gate.
//!
//! Verbatim port of pnpm's
//! [`verifyLockfileResolutions.ts`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/installing/deps-installer/src/install/verifyLockfileResolutions.ts).
//!
//! Walks every entry in `lockfile.packages`, dedupes by
//! `(name, version, resolution)`, and asks every active verifier to
//! evaluate each candidate. Verifiers handle their own protocol
//! short-circuit by returning [`ResolutionVerification::Ok`] for
//! resolutions outside their scope; the runner is policy-neutral and
//! dispatch-free at this layer.
//!
//! Cache lookup / record are out of scope for this slice (Phase 6
//! splits the runner from the JSONL cache). The shape that supports
//! the cache — `lockfile_path` on the options bag, the runner-side
//! emit boundaries — is in place so the cache slice only needs to
//! plug into the existing call sites.

use std::{collections::BTreeMap, path::Path, sync::Arc, time::Instant};

use futures_util::{StreamExt, stream::FuturesUnordered};
use pacquet_lockfile::{Lockfile, LockfileResolution, PkgName, is_git_hosted_tarball_url};
use pacquet_reporter::{
    LockfileVerificationLog, LockfileVerificationMessage, LogEvent, LogLevel, Reporter,
};
use pacquet_resolving_resolver_base::{
    ResolutionPolicyViolation, ResolutionVerification, ResolutionVerifier, VerifyCtx, VerifyFuture,
};
use tokio::sync::Semaphore;

use crate::{
    cache::{CachePrecomputed, record_verification, try_lockfile_verification_cache},
    errors::{RenderedViolation, VerifyError},
    hash_lockfile,
};

/// Default concurrency cap for the per-candidate fan-out. Mirrors
/// upstream's `DEFAULT_CONCURRENCY = 16` (the floor of pnpm's
/// `package-requester` network-concurrency formula).
const DEFAULT_CONCURRENCY: usize = 16;

/// Options bundle for [`verify_lockfile_resolutions`]. Mirrors
/// upstream's
/// [`VerifyLockfileResolutionsOptions`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/installing/deps-installer/src/install/verifyLockfileResolutions.ts#L34-L47).
#[derive(Debug, Default, Clone)]
pub struct VerifyLockfileResolutionsOptions<'a> {
    /// Cap on concurrent verifier futures. `None` falls back to
    /// the internal `DEFAULT_CONCURRENCY` (`16`, matching upstream).
    pub concurrency: Option<usize>,
    /// Absolute path of the lockfile being verified. Required for
    /// the on-disk verification cache (the stat shortcut + per-path
    /// index key off it) and surfaced in the
    /// `pnpm:lockfile-verification` reporter payload.
    pub lockfile_path: Option<&'a Path>,
    /// Pnpm's on-disk cache directory. When set together with
    /// `lockfile_path`, a successful run is memoised in
    /// `<cache_dir>/lockfile-verified.jsonl` and the gate
    /// short-circuits on a repeat run against an unchanged lockfile
    /// (under the same or stricter policy). Omitting either field
    /// disables the cache (every call rehashes + reruns the gate).
    pub cache_dir: Option<&'a Path>,
}

/// Run every active [`ResolutionVerifier`] against every entry in
/// `lockfile.packages`. No-op when `verifiers` is empty or the
/// lockfile carries no `packages:` map.
///
/// Verifiers fan out across candidates with the runner's concurrency
/// cap; each candidate stops at the first verifier that rejects it,
/// so a multi-verifier setup never emits duplicate violations for
/// the same `(name, version)` pair.
///
/// Reporter events fire only when the fan-out actually runs — an
/// empty candidate set skips both `Started` and `Done`. On the
/// non-empty path, a `Started` always pairs with exactly one
/// terminal `Done` (success) or `Failed` (rejection), even if the
/// fan-out panics; the failure variant of the emit is fired from the
/// drop guard so the reporter never leaves a hanging "Verifying..."
/// frame.
pub async fn verify_lockfile_resolutions<Reporter: self::Reporter>(
    lockfile: &Lockfile,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    opts: &VerifyLockfileResolutionsOptions<'_>,
) -> Result<(), VerifyError> {
    if lockfile.packages.is_none() {
        return Ok(());
    }

    // Caching activates only when both `cache_dir` and
    // `lockfile_path` are supplied. Production wiring always passes
    // both; tests that skip them exercise the gate without
    // memoization (and still cover the runner's emit + violation
    // logic via the same code path).
    let cache_inputs = opts.cache_dir.zip(opts.lockfile_path);

    let cache_verifiers = with_resolution_shape_cache_identity(verifiers);

    // Memoised content hash. Used by both the lookup (when the
    // stat-shortcut doesn't apply) and the recorder (after the
    // gate passes). The closure is `FnMut` so multiple lazy calls
    // share the computed string.
    let mut cached_hash: Option<String> = None;
    let mut hash_once = || {
        if let Some(hash) = cached_hash.as_ref() {
            return hash.clone();
        }
        let hash = hash_lockfile(lockfile);
        cached_hash = Some(hash.clone());
        hash
    };

    let mut cache_precomputed: CachePrecomputed = CachePrecomputed::default();
    if let Some((cache_dir, lockfile_path)) = cache_inputs {
        let result = try_lockfile_verification_cache(
            cache_dir,
            lockfile_path,
            &cache_verifiers,
            &mut hash_once,
        );
        if result.hit {
            return Ok(());
        }
        cache_precomputed = result.precomputed;
    }

    let (candidates, shape_violations) = collect_candidates(lockfile);
    if !shape_violations.is_empty() {
        return Err(build_verification_error(shape_violations));
    }
    if verifiers.is_empty() {
        return Ok(());
    }
    let lockfile_path_str = opts.lockfile_path.map(|path| path.to_string_lossy().into_owned());
    if candidates.is_empty() {
        // Persist the success so the next install can stat-only the
        // lockfile. Matches upstream's behavior at
        // `verifyLockfileResolutions.ts:124-132` — empty fan-out is
        // still a successful run.
        if let Some((cache_dir, lockfile_path)) = cache_inputs {
            record_verification(
                cache_dir,
                lockfile_path,
                &cache_verifiers,
                &mut hash_once,
                cache_precomputed,
            );
        }
        return Ok(());
    }

    let entries = candidates.len() as u64;
    let started_at = Instant::now();
    emit::<Reporter>(
        LogLevel::Debug,
        LockfileVerificationMessage::Started { entries, lockfile_path: lockfile_path_str.clone() },
    );

    // The drop guard fires `Failed` for early-return / panic paths.
    // The success path replaces it with the `Done` payload before
    // returning, so the guard's drop only fires on a panic or on the
    // throw-violations branch.
    let mut emit_guard =
        TerminalEmitGuard::<Reporter>::failed(entries, started_at, lockfile_path_str.clone());

    let violations = run_fan_out(candidates, verifiers, opts.concurrency).await;
    if violations.is_empty() {
        emit_guard.cancel(LockfileVerificationMessage::Done {
            entries,
            elapsed_ms: started_at.elapsed().as_millis() as u64,
            lockfile_path: lockfile_path_str,
        });
        if let Some((cache_dir, lockfile_path)) = cache_inputs {
            record_verification(
                cache_dir,
                lockfile_path,
                &cache_verifiers,
                &mut hash_once,
                cache_precomputed,
            );
        }
        return Ok(());
    }
    Err(build_verification_error(violations))
}

/// Collect-mode sibling of [`verify_lockfile_resolutions`] that
/// returns violations as data instead of throwing on the first batch.
/// No reporter emits, no cache wiring — for callers that need to
/// inspect violations (auto-collect into `minimumReleaseAgeExclude`,
/// strict-mode prompts, future custom policies).
pub async fn collect_resolution_policy_violations(
    lockfile: &Lockfile,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    concurrency: Option<usize>,
) -> Vec<ResolutionPolicyViolation> {
    if verifiers.is_empty() || lockfile.packages.is_none() {
        return Vec::new();
    }
    // Shape violations are deliberately not collected here: they are
    // hard tampering failures, not policy picks a caller may
    // auto-exclude.
    let (candidates, _shape_violations) = collect_candidates(lockfile);
    run_fan_out(candidates, verifiers, concurrency).await
}

pub const RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE: &str = "RESOLUTION_SHAPE_MISMATCH";

/// Cache-key participant for the offline resolution-shape pass: a
/// record written before the rule existed lacks the flag, so
/// `can_trust_past_check` rejects it and forces a re-verification.
/// `verify` is never invoked — the identity is appended only to the
/// verifier lists handed to the cache lookup and recorder.
struct ResolutionShapeCacheIdentity {
    policy: serde_json::Map<String, serde_json::Value>,
}

fn resolution_shape_cache_identity() -> Arc<dyn ResolutionVerifier> {
    let mut policy = serde_json::Map::new();
    policy.insert("resolutionShapeCheck".to_string(), serde_json::Value::Bool(true));
    Arc::new(ResolutionShapeCacheIdentity { policy })
}

/// Every verifier list that flows into the verification cache must
/// carry the resolution-shape identity, so records written before the
/// shape rule existed cannot stat-fast-path around it. Used by the
/// gate itself and by [`crate::record_lockfile_verified()`], whose
/// freshly-resolved lockfile satisfies the shape invariant by
/// construction (the writer derives every key from the resolution it
/// just produced).
pub(crate) fn with_resolution_shape_cache_identity(
    verifiers: &[Arc<dyn ResolutionVerifier>],
) -> Vec<Arc<dyn ResolutionVerifier>> {
    verifiers.iter().cloned().chain(std::iter::once(resolution_shape_cache_identity())).collect()
}

impl ResolutionVerifier for ResolutionShapeCacheIdentity {
    fn verify<'a>(
        &'a self,
        _resolution: &'a LockfileResolution,
        _ctx: VerifyCtx<'a>,
    ) -> VerifyFuture<'a> {
        Box::pin(async { ResolutionVerification::Ok })
    }

    fn policy(&self) -> &serde_json::Map<String, serde_json::Value> {
        &self.policy
    }

    fn can_trust_past_check(&self, cached: &serde_json::Map<String, serde_json::Value>) -> bool {
        cached.get("resolutionShapeCheck") == Some(&serde_json::Value::Bool(true))
    }
}

/// Mirrors upstream's `isRegistryShapedResolution`: a plain tarball
/// resolution is registry-shaped because the npm verifier unconditionally
/// binds explicit tarball URLs of semver-keyed entries to the registry's
/// own `dist.tarball`. A git-hosted tarball is not — and trust is gated on
/// the tarball URL rather than the `gitHosted` flag alone, so a tampered
/// entry that clears the flag (`gitHosted: false`) on a git-host URL is
/// still rejected.
fn is_registry_shaped_resolution(resolution: &LockfileResolution) -> bool {
    match resolution {
        LockfileResolution::Registry(_) => true,
        LockfileResolution::Tarball(tarball) => {
            // The tarball URL must be an http(s) registry artifact and not
            // git-hosted. The npm verifier's tarball-URL binding skips
            // non-http(s) schemes (file:, etc.), so a `file:` tarball under a
            // name@semver key would otherwise be trusted with no safety net.
            is_http_tarball_url(&tarball.tarball)
                && tarball.git_hosted != Some(true)
                && !is_git_hosted_tarball_url(&tarball.tarball)
        }
        LockfileResolution::Variations(variations) => variations
            .variants
            .iter()
            .all(|variant| is_registry_shaped_resolution(&variant.resolution)),
        LockfileResolution::Directory(_)
        | LockfileResolution::Git(_)
        | LockfileResolution::Binary(_) => false,
    }
}

/// Whether a tarball URL uses an http(s) scheme — the only schemes a
/// registry artifact is served over. Case-insensitive to reject a
/// tampered uppercase scheme.
fn is_http_tarball_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

/// One `(name, version, resolution)` tuple deduplicated from
/// `lockfile.packages`. Mirrors upstream's inline `Candidate`
/// interface.
struct Candidate {
    name: PkgName,
    version: String,
    resolution: LockfileResolution,
}

/// Walk `lockfile.packages` and dedupe by
/// `(name, version, resolution-json)`. Mirrors upstream's
/// [`collectCandidates`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/installing/deps-installer/src/install/verifyLockfileResolutions.ts#L248-L261).
///
/// The serialized resolution is part of the key so two entries that
/// share a `(name, version)` but differ in *what* was resolved (npm
/// vs git URL under the same alias) don't collapse into one.
/// `BTreeMap` over a serialized key gives deterministic iteration
/// order for tests; the fan-out runs across the value iter so order
/// doesn't affect correctness, only the reproducibility of failures.
fn collect_candidates(lockfile: &Lockfile) -> (Vec<Candidate>, Vec<ResolutionPolicyViolation>) {
    let Some(packages) = lockfile.packages.as_ref() else {
        return (Vec::new(), Vec::new());
    };
    let mut deduped: BTreeMap<String, Candidate> = BTreeMap::new();
    let mut shape_violations = Vec::new();
    for (key, metadata) in packages {
        let name = key.name.clone();
        let version = key.suffix.version().to_string();
        // A registry-style dep path (`name@semver`, no `runtime:`-style
        // prefix) must be backed by a registry-shaped resolution: the
        // allowBuilds policy derives a trusted package identity from
        // that key shape, which is only sound while this invariant
        // holds. The check is offline, so it applies even when no
        // policy verifiers are active.
        if key.suffix.prefix() == pacquet_lockfile::Prefix::None
            && matches!(key.suffix.version(), pacquet_lockfile::VersionPart::Semver(_))
            && !is_registry_shaped_resolution(&metadata.resolution)
        {
            shape_violations.push(ResolutionPolicyViolation {
                name: name.clone(),
                version: version.clone(),
                resolution: metadata.resolution.clone(),
                code: RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE,
                reason: "a registry-style dependency path is backed by a non-registry resolution"
                    .to_string(),
            });
        }
        // Every `LockfileResolution` variant derives `Serialize`, and
        // the wire shape never contains non-string keys or non-finite
        // numbers — the only way this `expect` could fire is a future
        // variant that breaks the contract. Fail loudly rather than
        // skipping the candidate, which would silently bypass
        // verification for that lockfile entry.
        let resolution_json = serde_json::to_string(&metadata.resolution)
            .expect("LockfileResolution must serialize for candidate dedupe");
        let key = format!("{name}@{version}@{resolution_json}");
        deduped.entry(key).or_insert_with(|| Candidate {
            name,
            version,
            resolution: metadata.resolution.clone(),
        });
    }
    (deduped.into_values().collect(), shape_violations)
}

/// Run every active verifier against every candidate with a
/// concurrency cap. Each candidate stops at the first verifier that
/// rejects it.
async fn run_fan_out(
    candidates: Vec<Candidate>,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    concurrency: Option<usize>,
) -> Vec<ResolutionPolicyViolation> {
    let limit = concurrency.unwrap_or(DEFAULT_CONCURRENCY).max(1);
    let semaphore = Arc::new(Semaphore::new(limit));
    let mut futures = FuturesUnordered::new();
    for candidate in candidates {
        let verifiers: Vec<Arc<dyn ResolutionVerifier>> = verifiers
            .iter()
            .filter_map(|verifier| {
                let ctx = VerifyCtx { name: &candidate.name, version: &candidate.version };
                verifier.might_verify(&candidate.resolution, ctx).then(|| Arc::clone(verifier))
            })
            .collect();
        if verifiers.is_empty() {
            continue;
        }

        let semaphore = Arc::clone(&semaphore);
        futures.push(async move {
            // Holding the permit across every verifier .await keeps
            // the effective in-flight count bounded by the semaphore.
            // Releasing per-verifier would let N candidates × M
            // verifiers race past the cap.
            let _permit = semaphore.acquire().await.expect("semaphore not closed during fan-out");
            evaluate_candidate(candidate, &verifiers).await
        });
    }
    let mut violations = Vec::new();
    while let Some(result) = futures.next().await {
        if let Some(violation) = result {
            violations.push(violation);
        }
    }
    violations
}

async fn evaluate_candidate(
    candidate: Candidate,
    verifiers: &[Arc<dyn ResolutionVerifier>],
) -> Option<ResolutionPolicyViolation> {
    for verifier in verifiers {
        let ctx = VerifyCtx { name: &candidate.name, version: &candidate.version };
        match verifier.verify(&candidate.resolution, ctx).await {
            ResolutionVerification::Ok => continue,
            ResolutionVerification::Err { code, reason } => {
                return Some(ResolutionPolicyViolation {
                    name: candidate.name,
                    version: candidate.version,
                    resolution: candidate.resolution,
                    code,
                    reason,
                });
            }
        }
    }
    None
}

/// Sort violations by `name@version` and build the matching
/// [`VerifyError`]. Mirrors upstream's
/// [`buildVerificationError`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/installing/deps-installer/src/install/verifyLockfileResolutions.ts#L172-L206).
fn build_verification_error(mut violations: Vec<ResolutionPolicyViolation>) -> VerifyError {
    violations.sort_by(|left, right| {
        format!("{}@{}", left.name, left.version).cmp(&format!("{}@{}", right.name, right.version))
    });
    let rendered: Vec<RenderedViolation> = violations
        .into_iter()
        .map(|violation| RenderedViolation {
            name: violation.name.to_string(),
            version: violation.version,
            code: violation.code,
            reason: violation.reason,
        })
        .collect();
    VerifyError::from_rendered(&rendered)
}

fn emit<Reporter: self::Reporter>(level: LogLevel, message: LockfileVerificationMessage) {
    Reporter::emit(&LogEvent::LockfileVerification(LockfileVerificationLog { level, message }));
}

/// Drop guard that fires the terminal `Failed` payload when the
/// runner panics or returns early through `?`. On the success path
/// the runner calls [`Self::cancel`] with the `Done` payload, which
/// replaces the queued message and emits it on drop instead.
struct TerminalEmitGuard<Reporter: self::Reporter> {
    pending: Option<LockfileVerificationMessage>,
    /// `Started` instant captured at runner entry. The Drop impl uses
    /// it to refresh `elapsed_ms` on the Failed branch (the field is
    /// stale on the `pending` payload — it was built at guard
    /// construction, before the fan-out ran). `Done` payloads land
    /// via [`Self::cancel`] with their own up-to-date `elapsed_ms`.
    started_at: Instant,
    _reporter: std::marker::PhantomData<Reporter>,
}

impl<Reporter: self::Reporter> TerminalEmitGuard<Reporter> {
    fn failed(entries: u64, started_at: Instant, lockfile_path: Option<String>) -> Self {
        Self {
            pending: Some(LockfileVerificationMessage::Failed {
                entries,
                // Placeholder; the Drop impl overwrites this with
                // the real elapsed when the guard actually fires.
                elapsed_ms: 0,
                lockfile_path,
            }),
            started_at,
            _reporter: std::marker::PhantomData,
        }
    }

    fn cancel(&mut self, success: LockfileVerificationMessage) {
        self.pending = Some(success);
    }
}

impl<Reporter: self::Reporter> Drop for TerminalEmitGuard<Reporter> {
    fn drop(&mut self) {
        if let Some(message) = self.pending.take() {
            // Refresh `elapsed_ms` on the Failed branch only — the
            // success branch already filled the up-to-date value via
            // `cancel(Done { elapsed_ms: <now> })`.
            let message = match message {
                LockfileVerificationMessage::Failed { entries, lockfile_path, .. } => {
                    LockfileVerificationMessage::Failed {
                        entries,
                        elapsed_ms: self.started_at.elapsed().as_millis() as u64,
                        lockfile_path,
                    }
                }
                other => other,
            };
            emit::<Reporter>(LogLevel::Debug, message);
        }
    }
}

#[cfg(test)]
mod tests;
