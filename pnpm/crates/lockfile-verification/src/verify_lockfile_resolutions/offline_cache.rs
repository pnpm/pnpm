use pnpm_lockfile::LockfileResolution;
use pnpm_resolving_resolver_base::{
    ResolutionVerification, ResolutionVerifier, VerifyCtx, VerifyFuture,
};
use std::sync::Arc;

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
    policy.insert(
        "resolutionShapeCheck".to_string(),
        serde_json::Value::Bool(true),
    );
    Arc::new(OfflineCheckCacheIdentity {
        policy,
        flag: "resolutionShapeCheck",
    })
}

fn dependency_alias_cache_identity() -> Arc<dyn ResolutionVerifier> {
    let mut policy = serde_json::Map::new();
    policy.insert(
        "dependencyAliasCheck".to_string(),
        serde_json::Value::Bool(true),
    );
    Arc::new(OfflineCheckCacheIdentity {
        policy,
        flag: "dependencyAliasCheck",
    })
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
        .chain([
            resolution_shape_cache_identity(),
            dependency_alias_cache_identity(),
        ])
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
