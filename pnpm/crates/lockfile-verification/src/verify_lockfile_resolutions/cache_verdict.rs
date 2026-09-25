use std::{path::Path, sync::Arc};

use pnpm_lockfile::{Lockfile, LockfileResolution};
use pnpm_reporter::{LockfileVerificationMessage, LogLevel, Reporter};
use pnpm_resolving_resolver_base::{
    ResolutionVerification, ResolutionVerifier, VerifyCtx, VerifyFuture,
};

use crate::{
    cache::{
        CachePrecomputed, lockfile_verification_is_cached_by_hash, record_verification,
        try_lockfile_verification_cache,
    },
    hash_lockfile,
};

use super::{dependency_names::verify_lockfile_dependency_names, progress::emit};

/// What the verification cache had to say about this lockfile.
pub(super) enum CacheOutcome {
    Hit,
    Miss(CachePrecomputed),
}

/// What a cache lookup needs to report a hit.
#[derive(Clone, Copy)]
pub(super) struct CachedVerdict<'a> {
    pub(super) has_policy_verifiers: bool,
    pub(super) lockfile_path: Option<&'a String>,
}

/// Whether a recorded verification already covers `lockfile` as it sits
/// at `lockfile_path` under the policy `verifiers` currently demand.
pub fn lockfile_verification_is_cached(
    cache_dir: &Path,
    lockfile_path: &Path,
    lockfile: &Lockfile,
    verifiers: &[Arc<dyn ResolutionVerifier>],
) -> bool {
    if verify_lockfile_dependency_names(lockfile).is_err() {
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
/// `lockfile` content under the currently active policy.
pub fn lockfile_verification_is_cached_by_content(
    cache_dir: &Path,
    lockfile: &Lockfile,
    verifiers: &[Arc<dyn ResolutionVerifier>],
) -> bool {
    if verify_lockfile_dependency_names(lockfile).is_err() {
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

/// Share the lazy hash between cache lookup and recording.
pub(super) fn memoized_lockfile_hash(lockfile: &Lockfile) -> impl FnMut() -> String + Send + '_ {
    let mut cached_hash: Option<String> = None;
    move || cached_hash.get_or_insert_with(|| hash_lockfile(lockfile)).clone()
}

/// Reuse a previous verdict for this lockfile, if the cache holds one.
pub(super) fn reuse_cached_verdict<Reporter: self::Reporter>(
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
pub(super) fn record_verdict(
    cache_inputs: Option<(&Path, &Path)>,
    cache_verifiers: &[Arc<dyn ResolutionVerifier>],
    hash_once: &mut impl FnMut() -> String,
    precomputed: CachePrecomputed,
) {
    if let Some((cache_dir, lockfile_path)) = cache_inputs {
        record_verification(cache_dir, lockfile_path, cache_verifiers, hash_once, precomputed);
    }
}

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

/// Appends the always-on offline structural check identities to `verifiers`.
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
