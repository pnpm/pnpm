use super::{
    Arc, Footprint, Identity, Lockfile, LockfileVerifyContext, Mutex, OsvIndex, ResolutionVerifier,
    Resolver, VerifyFailure, request_timeout_response,
    verdict_cache::{VerdictLookup, VerdictRecord},
};

pub(super) fn compute_verdict_hash(
    context: &LockfileVerifyContext<'_>,
    raw_lockfile: &Lockfile,
) -> Result<String, VerifyFailure> {
    let expired = || tokio::time::Instant::now() >= context.deadline;
    context.tarball_router
        .verification_verdict_hash_until(context.config, raw_lockfile, &expired)
        .ok_or_else(|| VerifyFailure::Internal(request_timeout_response()))
}

pub(super) fn apply_cached_verdict(
    runtime: &Resolver,
    context: &LockfileVerifyContext<'_>,
    hash: &str,
    epoch: Option<&str>,
    verifiers: &[Arc<dyn ResolutionVerifier>],
) -> bool {
    let Some(cached) = past_verdict_footprint(runtime, context.identity, hash, epoch, verifiers)
    else {
        return false;
    };
    context.footprint
        .lock()
        .expect("footprint poisoned")
        .extend(&cached);
    true
}

pub(super) fn record_verdict(
    runtime: &Resolver,
    hash: &str,
    epoch: Option<&str>,
    footprint: &Mutex<Footprint>,
    verifiers: &[Arc<dyn ResolutionVerifier>],
) {
    let Some(epoch) =
        epoch.filter(|pinned| runtime.local_registry.verdict_epoch().as_deref() == Some(*pinned))
    else {
        return;
    };
    let Some(cache) = runtime.cache.verdicts.as_ref() else {
        return;
    };
    let recorded = footprint
        .lock()
        .expect("footprint poisoned")
        .clone();
    cache.record(VerdictRecord {
        hash,
        footprint: &recorded,
        secret: &runtime.cache.secret,
        epoch,
        policy: merge_policies(verifiers, runtime.osv_index.as_ref()),
    });
}

/// Whole-lockfile verdict cache: an O(1) hit when this exact lockfile
/// already passed in the current grant/graph epoch under a footprint this
/// caller satisfies and a policy we still trust skips the whole fan-out.
pub(in crate::resolver) fn past_verdict_footprint(
    runtime: &Resolver,
    identity: &Identity,
    hash: &str,
    epoch: Option<&str>,
    verifiers: &[Arc<dyn ResolutionVerifier>],
) -> Option<Footprint> {
    let epoch = epoch?;
    let cache = runtime.cache.verdicts.as_ref()?;
    let lookup = VerdictLookup {
        hash,
        epoch,
        secret: &runtime.cache.secret,
        route_context: &runtime.route_context,
        identity,
    };
    cache.verified_footprint(&lookup, |policy| {
        verifiers
            .iter()
            .all(|verifier| verifier.can_trust_past_check(policy))
            && runtime.osv_index
                .as_ref()
                .is_none_or(|index| index.can_trust_policy(policy))
    })
}

/// Merge every active verifier's policy snapshot into one bag, the key
/// the verdict cache stores alongside the lockfile hash. Later verifiers
/// overwrite earlier ones on a shared key — mirrors the local cache's
/// [`merge_policies`] so a verdict recorded here is comparable to one the
/// client's own cache would write.
pub(in crate::resolver) fn merge_policies(
    verifiers: &[Arc<dyn ResolutionVerifier>],
    osv_index: Option<&Arc<OsvIndex>>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut merged = serde_json::Map::new();
    for verifier in verifiers {
        for (key, value) in verifier.policy() {
            merged.insert(key.clone(), value.clone());
        }
    }
    if let Some(osv_index) = osv_index {
        merged.extend(osv_index.policy());
    }
    merged
}
