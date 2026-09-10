use super::{
    Arc, AuthHeaders, Bytes, Footprint, Identity, InMemoryPackageMetaCache, Lockfile, Mutex,
    ObservedDistStats, OsvIndex, PackageMetaCache, PacquetConfig, ResolutionVerifier,
    ResolveRequest, Resolver, Response, StatusCode, TOO_MANY_CONFIGS_MESSAGE, TarballRouter,
    build_resolution_verifiers, collect_resolution_policy_violations, hash_lockfile, json_error,
    ndjson_single_frame, observed_dist_stats_sink, osv_violations_for_lockfile,
    reject_inline_url_auth, reject_invalid_registries, reject_off_allowlist_fetches,
    verify_done_or_osv_violations, violations_frame,
};

/// Handle `POST /-/pnpr/v0/verify-lockfile`: verify the client's input
/// lockfile under the client's policy, returning only a terminal NDJSON
/// verdict frame. The client already knows the lockfile is fresh for
/// the current manifests, so this endpoint deliberately does not
/// resolve or echo the lockfile back.
pub(crate) async fn handle_verify_lockfile(
    runtime: &Resolver,
    identity: Identity,
    body: Bytes,
) -> Response {
    let request: ResolveRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(err) => return json_error(StatusCode::BAD_REQUEST, &err.to_string()),
    };

    if let Some(response) = reject_invalid_registries(&request) {
        return response;
    }
    if let Some(response) = reject_inline_url_auth(&request) {
        return response;
    }

    if let Some(response) = reject_off_allowlist_fetches(&request, &runtime.route_context) {
        return response;
    }

    let Some(input_lockfile) = request.lockfile.as_ref() else {
        return json_error(StatusCode::BAD_REQUEST, "`lockfile` is required");
    };

    if request.trust_lockfile {
        return verify_done_or_osv_violations(runtime.osv_index.as_ref(), input_lockfile);
    }

    let Some(config) = runtime.config_for(&request) else {
        return json_error(StatusCode::SERVICE_UNAVAILABLE, TOO_MANY_CONFIGS_MESSAGE);
    };
    // Verifier packument fetches run under the same route hook, so they
    // select the same pnpr-managed credentials and are recorded in the
    // same footprint as a resolve would be — a verifier can't read or
    // populate a cache scope a resolve wouldn't.
    let footprint = Arc::new(Mutex::new(Footprint::default()));
    let request_auth = runtime.hooked_auth(&request, &identity, &footprint);
    let tarball_router = TarballRouter::new(
        Arc::clone(&runtime.route_context),
        identity.clone(),
        runtime.public_url.clone(),
        config.resolved_registries().into_iter().collect(),
    );
    let input_lockfile = tarball_router.verification_lockfile(input_lockfile);

    match verify_input_lockfile(runtime, config, &request_auth, &input_lockfile).await {
        // The dist stats the verifier observed feed `/-/pnpr/v0/resolve`'s sized
        // `package` frames; this endpoint's client prefetches from its own
        // lockfile before the verdict arrives, so only the verdict is sent.
        Ok(_) => verify_done_or_osv_violations(runtime.osv_index.as_ref(), &input_lockfile),
        Err(VerifyFailure::Internal(response)) => response,
        Err(VerifyFailure::Violations(violations)) => {
            ndjson_single_frame(&violations_frame(&violations))
        }
    }
}

/// Why [`verify_input_lockfile`] failed: either the lockfile violated
/// the client's policy (carry the rendered violations so the caller can
/// shape them for the client's protocol) or the verifiers couldn't be
/// built at all (a ready-made error response).
pub(super) enum VerifyFailure {
    Violations(Vec<serde_json::Value>),
    Internal(Response),
}

/// Verify the client's input lockfile under the client's policy. On a
/// clean pass returns the [`ObservedDistStats`] the verifier
/// collected — `None` when the whole-lockfile verdict cache satisfied
/// the check without a fan-out (no metadata was fetched, so no sizes
/// exist). On a policy violation returns the rendered violations so
/// the caller can deliver them to the client. A build-verifiers
/// failure (e.g. an invalid exclude pattern) returns a ready-made 500.
pub(super) async fn verify_input_lockfile(
    runtime: &Resolver,
    config: &'static PacquetConfig,
    auth_headers: &Arc<AuthHeaders>,
    lockfile: &Lockfile,
) -> Result<Option<ObservedDistStats>, VerifyFailure> {
    // A fresh per-request packument cache shared with the verifier; the
    // on-disk metadata mirror under `<cache_dir>/v11/metadata-full` is
    // warm across requests and is the real verification cache.
    let meta_cache = Arc::new(InMemoryPackageMetaCache::default());
    let dist_stats = observed_dist_stats_sink();
    let verifiers = build_resolution_verifiers(
        config,
        Arc::clone(&runtime.client),
        Some(meta_cache as Arc<dyn PackageMetaCache>),
        Some(Arc::clone(auth_headers)),
        Some(Arc::clone(&dist_stats)),
        None,
    )
    .map_err(|err| {
        VerifyFailure::Internal(json_error(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()))
    })?;

    let hash = hash_lockfile(lockfile);
    if past_verdict_trusted(runtime, &hash, &verifiers) {
        return Ok(None);
    }

    // A transport failure verifying an entry (the upstream registry couldn't be
    // reached/authorized) is a gateway error, not a policy violation — surface
    // the registry's own (credential-redacted) message to the client.
    let violations = match collect_resolution_policy_violations(lockfile, &verifiers, None).await {
        Ok(violations) => violations,
        Err(message) => {
            return Err(VerifyFailure::Internal(json_error(StatusCode::BAD_GATEWAY, &message)));
        }
    };
    let osv_violations = runtime
        .osv_index
        .as_ref()
        .map_or_else(Vec::new, |index| osv_violations_for_lockfile(index, lockfile));
    if violations.is_empty() && osv_violations.is_empty() {
        if let Some(cache) = runtime.verdict_cache.as_ref() {
            cache.record(&hash, &merge_policies(&verifiers, runtime.osv_index.as_ref()));
        }
        return Ok(Some(dist_stats));
    }

    Err(VerifyFailure::Violations(render_violations(&violations, osv_violations)))
}

/// Whole-lockfile verdict cache: an O(1) hit when this exact lockfile
/// already passed under a policy we still trust skips the whole fan-out
/// (the dominant win for a shared pnpr — CI re-runs, a fleet building
/// the same repo).
pub(super) fn past_verdict_trusted(
    runtime: &Resolver,
    hash: &str,
    verifiers: &[Arc<dyn ResolutionVerifier>],
) -> bool {
    runtime.verdict_cache.as_ref().is_some_and(|cache| {
        cache.is_verified(hash, |policy| {
            verifiers.iter().all(|verifier| verifier.can_trust_past_check(policy))
                && runtime.osv_index.as_ref().is_none_or(|index| index.can_trust_policy(policy))
        })
    })
}

pub(super) fn render_violations(
    violations: &[pnpm_resolving_resolver_base::ResolutionPolicyViolation],
    osv_violations: Vec<serde_json::Value>,
) -> Vec<serde_json::Value> {
    let mut rendered: Vec<serde_json::Value> = violations
        .iter()
        .map(|violation| {
            serde_json::json!({
                "name": violation.name.to_string(),
                "version": violation.version,
                "code": violation.code,
                "reason": violation.reason,
            })
        })
        .collect();
    rendered.extend(osv_violations);
    rendered
}

/// Merge every active verifier's policy snapshot into one bag, the key
/// the verdict cache stores alongside the lockfile hash. Later verifiers
/// overwrite earlier ones on a shared key — mirrors the local cache's
/// [`merge_policies`] so a verdict recorded here is comparable to one the
/// client's own cache would write.
pub(super) fn merge_policies(
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
