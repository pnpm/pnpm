use super::{
    Arc, AuthHeaders, Bytes, Footprint, Identity, InMemoryPackageMetaCache, Lockfile, Mutex,
    ObservedDistStats, OsvIndex, PackageMetaCache, PacquetConfig, ResolutionVerifier,
    ResolveRequest, Resolver, Response, StatusCode, StreamedResolveInputs, TarballRouter,
    VerificationArtifacts, build_resolution_verifiers_with_artifacts,
    collect_resolution_policy_violations, extract_admitted_body, hash_lockfile, json_error,
    limits::{
        AdmittedResolve, CancelToken, ResolvePermit, reject_oversized_graph,
        request_timeout_response,
    },
    ndjson_single_frame, observed_dist_stats_sink, osv_violations_for_lockfile_until,
    parse_request, reject_inline_url_auth, reject_invalid_registries, reject_off_allowlist_fetches,
    request_tarball_router,
    verdict_cache::{VerdictLookup, VerdictRecord},
    verify_done_frame, violations_frame,
};

/// Admit `POST /-/pnpr/v0/verify-lockfile` before buffering or parsing the
/// body so excess concurrent callers are shed before allocating 32 MiB bodies.
pub(crate) async fn serve_verify_lockfile_request(
    runtime: &Resolver,
    identity: Identity,
    request: axum::extract::Request,
) -> Response {
    let deadline = runtime.limits.deadline_for_request(&request);
    let admitted = match runtime.limits.admit(&identity) {
        Ok(admitted) => admitted,
        Err(response) => return response,
    };
    let body = match extract_admitted_body(request, deadline).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    handle_admitted_verify_lockfile(runtime, identity, body, deadline, admitted).await
}

/// Handle `POST /-/pnpr/v0/verify-lockfile`: verify the client's input
/// lockfile under the client's policy, returning only a terminal NDJSON
/// verdict frame. The client already knows the lockfile is fresh for
/// the current manifests, so this endpoint deliberately does not
/// resolve or echo the lockfile back.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "in-process entry point mirroring handle_resolve")
)]
pub(crate) async fn handle_verify_lockfile(
    runtime: &Resolver,
    identity: Identity,
    body: Bytes,
) -> Response {
    let deadline = runtime.limits.start_deadline();
    let admitted = match runtime.limits.admit(&identity) {
        Ok(admitted) => admitted,
        Err(response) => return response,
    };
    handle_admitted_verify_lockfile(runtime, identity, body, deadline, admitted).await
}

async fn handle_admitted_verify_lockfile(
    runtime: &Resolver,
    identity: Identity,
    body: Bytes,
    deadline: tokio::time::Instant,
    admitted: AdmittedResolve<'_>,
) -> Response {
    let request: ResolveRequest = match parse_request(&body) {
        Ok(request) => request,
        Err(response) => return response,
    };
    drop(body);

    if let Some(response) = reject_oversized_graph(&request, runtime.limits.max_npm_packages)
        .or_else(|| reject_invalid_registries(&request))
        .or_else(|| reject_inline_url_auth(&request))
        .or_else(|| reject_off_allowlist_fetches(&request, &runtime.route_context))
    {
        return response;
    }

    if request.lockfile.is_none() {
        return json_error(StatusCode::BAD_REQUEST, "`lockfile` is required");
    }

    if request.trust_lockfile {
        return verify_trusted_lockfile(runtime, request, deadline, admitted).await;
    }

    let config = match runtime.config_for(&request) {
        Ok(config) => config,
        Err(response) => return response,
    };
    let permit = match admitted.acquire(deadline).await {
        Ok(permit) => permit,
        Err(response) => return response,
    };
    verify_with_permit(runtime, &config, identity, request, permit, deadline).await
}

async fn verify_trusted_lockfile(
    runtime: &Resolver,
    request: ResolveRequest,
    deadline: tokio::time::Instant,
    admitted: AdmittedResolve<'_>,
) -> Response {
    let permit = match admitted.acquire(deadline).await {
        Ok(permit) => Arc::new(permit),
        Err(response) => return response,
    };
    let done = verify_done_frame(&request);
    let Some(input_lockfile) = request.lockfile else {
        return json_error(StatusCode::BAD_REQUEST, "`lockfile` is required");
    };
    match scan_osv_until_deadline(
        runtime.osv_index.clone(),
        Arc::new(input_lockfile),
        Arc::clone(&permit),
        deadline,
    )
    .await
    {
        Ok(violations) if violations.is_empty() => {
            let response = within_verify_deadline(ndjson_single_frame(&done), deadline);
            drop(permit);
            response
        }
        Ok(violations) => {
            let response = within_verify_deadline(
                ndjson_single_frame(&violations_frame(&violations)),
                deadline,
            );
            drop(permit);
            response
        }
        Err(response) => response,
    }
}

fn within_verify_deadline(response: Response, deadline: tokio::time::Instant) -> Response {
    if tokio::time::Instant::now() < deadline { response } else { request_timeout_response() }
}

fn spawn_osv_violations_scan(
    osv_index: Arc<OsvIndex>,
    lockfile: Arc<Lockfile>,
    permit: Arc<ResolvePermit>,
    cancel: CancelToken,
) -> tokio::task::JoinHandle<Option<Vec<serde_json::Value>>> {
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        osv_violations_for_lockfile_until(&osv_index, &lockfile, &|| cancel.is_cancelled())
    })
}

async fn scan_osv_until_deadline(
    osv_index: Option<Arc<OsvIndex>>,
    lockfile: Arc<Lockfile>,
    permit: Arc<ResolvePermit>,
    deadline: tokio::time::Instant,
) -> Result<Vec<serde_json::Value>, Response> {
    if tokio::time::Instant::now() >= deadline {
        return Err(request_timeout_response());
    }
    let Some(osv_index) = osv_index else {
        return Ok(Vec::new());
    };
    let (cancel, _guard) = CancelToken::new(deadline);
    let mut scan = spawn_osv_violations_scan(osv_index, lockfile, permit, cancel.clone());
    tokio::select! {
        biased;
        () = tokio::time::sleep_until(deadline) => {
            cancel.cancel();
            Err(request_timeout_response())
        }
        joined = &mut scan => match joined {
            Ok(Some(violations)) => Ok(violations),
            Ok(None) => Err(request_timeout_response()),
            Err(err) => Err(json_error(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string())),
        },
    }
}

async fn verify_with_permit(
    runtime: &Resolver,
    config: &PacquetConfig,
    identity: Identity,
    request: ResolveRequest,
    permit: ResolvePermit,
    deadline: tokio::time::Instant,
) -> Response {
    let Some(raw_lockfile) = request.lockfile.as_ref() else {
        return json_error(StatusCode::BAD_REQUEST, "`lockfile` is required");
    };
    let permit = Arc::new(permit);
    let footprint = Arc::new(Mutex::new(Footprint::default()));
    let request_auth = runtime.hooked_auth(&request, &identity, &footprint);
    let tarball_router = request_tarball_router(runtime, &identity, config);
    let outcome = verify_input_lockfile(
        runtime,
        &LockfileVerifyContext {
            config,
            identity: &identity,
            auth_headers: &request_auth,
            tarball_router: &tarball_router,
            footprint: &footprint,
            permit: &permit,
            deadline,
        },
        raw_lockfile,
    )
    .await;
    finish_verified_lockfile(&request, deadline, outcome)
}

struct LockfileVerifyContext<'a> {
    config: &'a PacquetConfig,
    identity: &'a Identity,
    auth_headers: &'a Arc<AuthHeaders>,
    tarball_router: &'a TarballRouter,
    footprint: &'a Mutex<Footprint>,
    permit: &'a Arc<ResolvePermit>,
    deadline: tokio::time::Instant,
}

fn finish_verified_lockfile(
    request: &ResolveRequest,
    deadline: tokio::time::Instant,
    outcome: Result<Option<ObservedDistStats>, VerifyFailure>,
) -> Response {
    match outcome {
        Ok(_) => within_verify_deadline(ndjson_single_frame(&verify_done_frame(request)), deadline),
        Err(VerifyFailure::Internal(response)) => response,
        Err(VerifyFailure::Violations(violations)) => {
            within_verify_deadline(ndjson_single_frame(&violations_frame(&violations)), deadline)
        }
    }
}

/// Verify the *input* lockfile under the client's policy before any package is
/// streamed ([pnpm/pnpm#12139](https://github.com/pnpm/pnpm/issues/12139)).
///
/// The client skips its own `verifyLockfileResolutions` whenever a pnpr server
/// is configured, so this is the only place the committed/reused entries get
/// checked. A true first install sends no lockfile — nothing to verify.
/// `trustLockfile` is the client's opt-out (mirrors the local path's
/// `--trust-lockfile`). Freshly-resolved entries are held to the same policy by
/// the resolver's pick-time gate (the policy is wired into `config`).
pub(super) async fn verify_request_lockfile(
    runtime: &Resolver,
    identity: &Identity,
    inputs: &StreamedResolveInputs,
) -> Result<Option<ObservedDistStats>, Response> {
    if inputs.request.trust_lockfile {
        return Ok(None);
    }
    let Some(input_lockfile) = inputs.request.lockfile.as_ref() else {
        return Ok(None);
    };
    match verify_input_lockfile(
        runtime,
        &LockfileVerifyContext {
            config: &inputs.config,
            identity,
            auth_headers: &inputs.request_auth,
            tarball_router: &inputs.tarball_router,
            footprint: &inputs.footprint,
            permit: &inputs.permit,
            deadline: inputs.deadline,
        },
        input_lockfile,
    )
    .await
    {
        Ok(stats) => Ok(stats),
        Err(VerifyFailure::Internal(response)) => Err(response),
        Err(VerifyFailure::Violations(violations)) => Err(within_verify_deadline(
            ndjson_single_frame(&violations_frame(&violations)),
            inputs.deadline,
        )),
    }
}

/// Why [`verify_input_lockfile`] failed: either the lockfile violated
/// the client's policy (carry the rendered violations so the caller can
/// shape them for the client's protocol) or verification aborted with a
/// ready-made error or timeout response.
enum VerifyFailure {
    Violations(Vec<serde_json::Value>),
    Internal(Response),
}

async fn collect_policy_violations_until(
    lockfile: &Lockfile,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    deadline: tokio::time::Instant,
) -> Result<Vec<pnpm_resolving_resolver_base::ResolutionPolicyViolation>, VerifyFailure> {
    let Ok(result) = tokio::time::timeout_at(
        deadline,
        collect_resolution_policy_violations(lockfile, verifiers, None),
    )
    .await
    else {
        return Err(VerifyFailure::Internal(request_timeout_response()));
    };
    // A transport failure verifying an entry (the upstream registry couldn't be
    // reached/authorized) is a gateway error, not a policy violation — surface
    // the registry's own (credential-redacted) message to the client.
    result.map_err(|message| VerifyFailure::Internal(json_error(StatusCode::BAD_GATEWAY, &message)))
}

/// Verify the client's input lockfile under the client's policy.
async fn verify_input_lockfile(
    runtime: &Resolver,
    context: &LockfileVerifyContext<'_>,
    raw_lockfile: &Lockfile,
) -> Result<Option<ObservedDistStats>, VerifyFailure> {
    let epoch = runtime.local_registry.verdict_epoch();
    let (lockfile, routed_entries) =
        context.tarball_router.verification_lockfile_with_routed_entries(raw_lockfile);
    let lockfile = Arc::new(lockfile);
    let routed_entries = Arc::new(routed_entries);
    let hash = verification_verdict_hash(&lockfile, &routed_entries);
    let dist_stats = observed_dist_stats_sink();
    let verifiers = build_input_verifiers(
        runtime,
        context.config,
        context.auth_headers,
        context.tarball_router,
        &dist_stats,
        Arc::clone(&routed_entries),
    )?;

    if let Some(cached) =
        past_verdict_footprint(runtime, context.identity, &hash, epoch.as_deref(), &verifiers)
    {
        context.footprint
            .lock()
            .expect("footprint poisoned")
            .extend(&cached);
        return Ok(None);
    }

    let violations =
        collect_policy_violations_until(&lockfile, &verifiers, context.deadline).await?;
    let osv_violations = scan_osv_until_deadline(
        runtime.osv_index.clone(),
        lockfile,
        Arc::clone(context.permit),
        context.deadline,
    )
    .await
    .map_err(VerifyFailure::Internal)?;
    if violations.is_empty() && osv_violations.is_empty() {
        record_verdict(runtime, &hash, epoch.as_deref(), context.footprint, &verifiers);
        return Ok(Some(dist_stats));
    }

    Err(VerifyFailure::Violations(render_violations(&violations, osv_violations)))
}

fn build_input_verifiers(
    runtime: &Resolver,
    config: &PacquetConfig,
    auth_headers: &Arc<AuthHeaders>,
    tarball_router: &TarballRouter,
    dist_stats: &ObservedDistStats,
    routed_entries: Arc<super::wire::RoutedVerificationEntries>,
) -> Result<Vec<Arc<dyn ResolutionVerifier>>, VerifyFailure> {
    let meta_cache = Arc::new(InMemoryPackageMetaCache::default());
    let artifacts = VerificationArtifacts {
        observed_stats: Some(Arc::clone(dist_stats)),
        canonical_fetches: None,
        lookups: None,
        published_tarball_mapper: Some(tarball_router.published_tarball_mapper(routed_entries)),
    };
    build_resolution_verifiers_with_artifacts(
        config,
        Arc::clone(&runtime.client),
        Some(meta_cache as Arc<dyn PackageMetaCache>),
        Some(Arc::clone(auth_headers)),
        artifacts,
    )
    .map_err(|err| {
        VerifyFailure::Internal(json_error(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()))
    })
}

fn verification_verdict_hash(
    lockfile: &Lockfile,
    routed_entries: &super::wire::RoutedVerificationEntries,
) -> String {
    let base = hash_lockfile(lockfile);
    if routed_entries.is_empty() {
        return base;
    }
    use sha2::{Digest as _, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(base.as_bytes());
    for (name, version, registry_name) in routed_entries {
        hasher.update([0]);
        hasher.update(name.as_bytes());
        hasher.update([0]);
        hasher.update(version.as_bytes());
        hasher.update([0]);
        if let Some(registry_name) = registry_name {
            hasher.update(registry_name.as_bytes());
        }
    }
    format!("{:x}", hasher.finalize())
}

fn record_verdict(
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
fn past_verdict_footprint(
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
