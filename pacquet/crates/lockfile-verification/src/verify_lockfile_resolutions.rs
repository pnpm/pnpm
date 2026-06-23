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
use pacquet_resolving_parse_wanted_dependency::is_valid_old_npm_package_name;
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
/// upstream's `DEFAULT_CONCURRENCY = 64` (the floor of pnpm's
/// `package-requester` network-concurrency formula).
const DEFAULT_CONCURRENCY: usize = 64;

/// Options bundle for [`verify_lockfile_resolutions`]. Mirrors
/// upstream's
/// [`VerifyLockfileResolutionsOptions`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/installing/deps-installer/src/install/verifyLockfileResolutions.ts#L34-L47).
#[derive(Debug, Default, Clone)]
pub struct VerifyLockfileResolutionsOptions<'a> {
    /// Cap on concurrent verifier futures. `None` falls back to
    /// the internal `DEFAULT_CONCURRENCY` (`64`, matching upstream).
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
/// `lockfile.packages`.
///
/// The failure variant of the terminal emit is fired from the drop
/// guard so the reporter never leaves a hanging "Verifying..." frame
/// even if the fan-out panics.
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

    let cache_verifiers = with_offline_check_cache_identities(verifiers);

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

    let lockfile_path_str = opts.lockfile_path.map(|path| path.to_string_lossy().into_owned());

    let mut cache_precomputed: CachePrecomputed = CachePrecomputed::default();
    if let Some((cache_dir, lockfile_path)) = cache_inputs {
        let result = try_lockfile_verification_cache(
            cache_dir,
            lockfile_path,
            &cache_verifiers,
            &mut hash_once,
        );
        if result.hit {
            // A silent short-circuit looks like the policy gate never
            // ran (pnpm/pnpm#12324), so surface the reused verdict —
            // but only when policy verifiers are active; the
            // shape-only run that every install performs stays quiet.
            if !verifiers.is_empty() {
                emit::<Reporter>(
                    LogLevel::Debug,
                    LockfileVerificationMessage::Cached {
                        verified_at: result.verified_at,
                        lockfile_path: lockfile_path_str,
                    },
                );
            }
            return Ok(());
        }
        cache_precomputed = result.precomputed;
    }

    let (candidates, shape_violations, invalid_aliases) = collect_candidates(lockfile);
    if !invalid_aliases.is_empty() {
        return Err(VerifyError::invalid_dependency_aliases(&invalid_aliases));
    }
    if !shape_violations.is_empty() {
        return Err(build_verification_error(shape_violations));
    }
    if verifiers.is_empty() {
        return Ok(());
    }
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

    let violations = match run_fan_out(candidates, verifiers, opts.concurrency).await {
        Ok(violations) => violations,
        // The registry couldn't be reached to verify an entry: abort with its
        // own error (already credential-redacted) instead of a policy batch.
        // `emit_guard` is still armed to emit `failed` on drop.
        Err(message) => return Err(VerifyError::RegistryMetaFetchFailed { message }),
    };
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
) -> Result<Vec<ResolutionPolicyViolation>, String> {
    if verifiers.is_empty() || lockfile.packages.is_none() {
        return Ok(Vec::new());
    }
    // Shape violations and invalid aliases are deliberately not
    // collected here: they are hard tampering failures, not policy
    // picks a caller may auto-exclude.
    let (candidates, _shape_violations, _invalid_aliases) = collect_candidates(lockfile);
    // `Err(message)` is a transport failure the caller must surface rather than
    // treat as "no violations" — see [`run_fan_out`].
    run_fan_out(candidates, verifiers, concurrency).await
}

pub const RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE: &str = "RESOLUTION_SHAPE_MISMATCH";

/// Cache-key participant for an always-on offline structural check
/// (resolution-shape, dependency-alias): a record written before the
/// check's rule existed lacks its `flag`, so `can_trust_past_check`
/// rejects it and forces a re-verification. `verify` is never invoked —
/// the identity is appended only to the verifier lists handed to the
/// cache lookup and recorder.
struct OfflineCheckCacheIdentity {
    policy: serde_json::Map<String, serde_json::Value>,
    flag: &'static str,
}

fn resolution_shape_cache_identity() -> Arc<dyn ResolutionVerifier> {
    let mut policy = serde_json::Map::new();
    policy.insert("resolutionShapeCheck".to_string(), serde_json::Value::Bool(true));
    Arc::new(OfflineCheckCacheIdentity { policy, flag: "resolutionShapeCheck" })
}

fn dependency_alias_cache_identity() -> Arc<dyn ResolutionVerifier> {
    let mut policy = serde_json::Map::new();
    policy.insert("dependencyAliasCheck".to_string(), serde_json::Value::Bool(true));
    Arc::new(OfflineCheckCacheIdentity { policy, flag: "dependencyAliasCheck" })
}

/// Every verifier list that flows into the verification cache must
/// carry the always-on offline structural checks' identities, so a
/// record written before one of those rules existed cannot
/// stat-fast-path around it — its missing flag fails
/// `can_trust_past_check`, forcing a re-verification that runs the new
/// check. Used by the gate itself and by
/// [`crate::record_lockfile_verified()`], whose freshly-resolved
/// lockfile satisfies these invariants by construction (the resolver
/// validates aliases at manifest-read time and derives every resolution
/// key from the resolution it just produced).
pub(crate) fn with_offline_check_cache_identities(
    verifiers: &[Arc<dyn ResolutionVerifier>],
) -> Vec<Arc<dyn ResolutionVerifier>> {
    verifiers
        .iter()
        .cloned()
        .chain([resolution_shape_cache_identity(), dependency_alias_cache_identity()])
        .collect()
}

impl ResolutionVerifier for OfflineCheckCacheIdentity {
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
        cached.get(self.flag) == Some(&serde_json::Value::Bool(true))
    }
}

/// Mirrors upstream's `isRegistryShapedResolution`: a plain tarball
/// resolution is registry-shaped because the npm verifier unconditionally
/// binds explicit tarball URLs of semver-keyed entries to the registry's
/// own `dist.tarball`. A git-hosted tarball is not — and trust is gated on
/// the tarball URL rather than the `gitHosted` flag alone.
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

/// Add every alias in `aliases` that fails
/// `is_valid_old_npm_package_name` (the `validForOldPackages` rule
/// pnpm's `isValidDependencyAlias` applies) to `invalid`. Only pass maps
/// whose keys become `node_modules/<alias>` directories — not
/// `overrides`, `patched_dependencies`, or peer dependencies.
fn push_invalid_aliases<'alias>(
    aliases: impl Iterator<Item = &'alias PkgName>,
    invalid: &mut std::collections::BTreeSet<String>,
) {
    for alias in aliases {
        let alias = alias.to_string();
        if !is_valid_old_npm_package_name(&alias) {
            invalid.insert(alias);
        }
    }
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
fn collect_candidates(
    lockfile: &Lockfile,
) -> (Vec<Candidate>, Vec<ResolutionPolicyViolation>, Vec<String>) {
    let Some(packages) = lockfile.packages.as_ref() else {
        return (Vec::new(), Vec::new(), Vec::new());
    };
    // Pacquet keeps the alias-bearing maps in `importers` / `snapshots`,
    // separate from the `packages` metadata the loop below walks, so
    // they're scanned here in the same pass.
    let mut invalid_aliases: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for importer in lockfile.importers.values() {
        for deps in
            [&importer.dependencies, &importer.dev_dependencies, &importer.optional_dependencies]
        {
            push_invalid_aliases(
                deps.iter().flatten().map(|(alias, _)| alias),
                &mut invalid_aliases,
            );
        }
    }
    if let Some(snapshots) = lockfile.snapshots.as_ref() {
        for snapshot in snapshots.values() {
            for deps in [&snapshot.dependencies, &snapshot.optional_dependencies] {
                push_invalid_aliases(
                    deps.iter().flatten().map(|(alias, _)| alias),
                    &mut invalid_aliases,
                );
            }
        }
    }
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
    (deduped.into_values().collect(), shape_violations, invalid_aliases.into_iter().collect())
}

/// Run every active verifier against every candidate with a
/// concurrency cap. Each candidate stops at the first verifier that
/// rejects it.
async fn run_fan_out(
    candidates: Vec<Candidate>,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    concurrency: Option<usize>,
) -> Result<Vec<ResolutionPolicyViolation>, String> {
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
    // A transport failure (the registry couldn't be reached to verify an
    // entry) aborts the whole pass with the registry's own error rather than
    // collecting it as a policy violation. Drain the rest of the fan-out so no
    // in-flight task is dropped mid-await, but keep only the first abort.
    let mut violations = Vec::new();
    let mut fetch_error: Option<String> = None;
    while let Some(result) = futures.next().await {
        match result {
            Ok(Some(violation)) => violations.push(violation),
            Ok(None) => {}
            Err(message) => {
                if fetch_error.is_none() {
                    fetch_error = Some(message);
                }
            }
        }
    }
    // A registry that couldn't be reached takes precedence over collected
    // violations: the pass never finished, so the batch is incomplete and the
    // actionable failure is the transport error.
    match fetch_error {
        Some(message) => Err(message),
        None => Ok(violations),
    }
}

/// Outcome of evaluating one candidate against the active verifiers.
/// `Err(message)` is a transport failure (the registry couldn't be
/// reached to verify the entry) — the runner aborts the whole pass with
/// it rather than collecting it as a policy violation.
async fn evaluate_candidate(
    candidate: Candidate,
    verifiers: &[Arc<dyn ResolutionVerifier>],
) -> Result<Option<ResolutionPolicyViolation>, String> {
    for verifier in verifiers {
        let ctx = VerifyCtx { name: &candidate.name, version: &candidate.version };
        match verifier.verify(&candidate.resolution, ctx).await {
            ResolutionVerification::Ok => continue,
            ResolutionVerification::Err { code, reason } => {
                return Ok(Some(ResolutionPolicyViolation {
                    name: candidate.name,
                    version: candidate.version,
                    resolution: candidate.resolution,
                    code,
                    reason,
                }));
            }
            ResolutionVerification::FetchFailed { message } => return Err(message),
        }
    }
    Ok(None)
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
