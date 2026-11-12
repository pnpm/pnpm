use axum::{body::Bytes, http::StatusCode, response::Response};
use pnpr_policy::Identity;
use pnpr_registry::Ecosystem;
use pnpr_route::{Footprint, RouteContext};
use std::sync::{Arc, Mutex};

use super::{
    ObservedDistStats, PacquetConfig, ResolveRequest, Resolver, StreamedResolveInputs,
    cached_resolution, cargo, json_error,
    limits::{AdmittedResolve, reject_oversized_graph, request_timeout_response},
    protocol::{self, EcosystemProbe},
    pypi, request_tarball_router,
    request_validation::{
        parse_request, reject_incompatible_lockfile_format, reject_inline_url_auth,
        reject_invalid_patch_hashes, reject_invalid_registries, reject_off_allowlist_fetches,
        reject_unsafe_publish_directories,
    },
    resolution_cache_key, resolve, stream_resolve_response, verify_request_lockfile,
    wire::{
        TarballRouter, frozen_package_frames_until, ndjson_frames_until, ndjson_single_frame,
        osv_violations_for_lockfile_until, try_done_frame_until, violations_frame,
    },
};

pub(super) async fn extract_admitted_body(
    request: axum::extract::Request,
    deadline: tokio::time::Instant,
) -> Result<Bytes, Response> {
    let extracted = tokio::time::timeout_at(
        deadline,
        <Bytes as axum::extract::FromRequest<()>>::from_request(request, &()),
    )
    .await
    .map_err(|_| request_timeout_response())?;
    extracted.map_err(axum::response::IntoResponse::into_response)
}

/// Admit `POST /-/pnpr/v0/resolve` before buffering or parsing the body so
/// excess concurrent callers are shed before allocating 32 MiB payloads.
pub(crate) async fn serve_resolve_request(
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
    handle_admitted_resolve(runtime, identity, body, deadline, admitted).await
}

/// Handle `POST /-/pnpr/v0/resolve`. One address serves every ecosystem;
/// the body's `ecosystem` field selects which resolver reads it, and a
/// body without one means npm.
#[cfg_attr(not(test), expect(dead_code, reason = "in-process Bytes entry point for tests"))]
pub(crate) async fn handle_resolve(
    runtime: &Resolver,
    identity: Identity,
    body: Bytes,
) -> Response {
    let deadline = runtime.limits.start_deadline();
    let admitted = match runtime.limits.admit(&identity) {
        Ok(admitted) => admitted,
        Err(response) => return response,
    };
    handle_admitted_resolve(runtime, identity, body, deadline, admitted).await
}

async fn handle_admitted_resolve(
    runtime: &Resolver,
    identity: Identity,
    body: Bytes,
    deadline: tokio::time::Instant,
    admitted: AdmittedResolve<'_>,
) -> Response {
    let probe: EcosystemProbe = match serde_json::from_slice(&body) {
        Ok(probe) => probe,
        Err(err) => return json_error(StatusCode::BAD_REQUEST, &err.to_string()),
    };
    if !super::resolves(probe.ecosystem) {
        return super::refuse_unresolvable(probe.ecosystem);
    }
    match probe.ecosystem {
        Ecosystem::Npm => handle_npm_resolve(runtime, identity, body, deadline, admitted).await,
        Ecosystem::Cargo => {
            cargo::handle_resolve(runtime, identity, body, deadline, admitted).await
        }
        // Listed rather than caught, so an ecosystem added to the shared
        // enum stops here for a decision instead of being refused silently.
        Ecosystem::Pypi => pypi::handle_resolve(runtime, identity, body, deadline, admitted).await,
        // An arm rather than a catch-all so an ecosystem added to the shared
        // enum has to decide here too, even though `resolves` turns this one
        // away before the dispatch runs.
        Ecosystem::Oci => super::refuse_unresolvable(probe.ecosystem),
    }
}

async fn handle_npm_resolve(
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

    resolve_npm_request(runtime, identity, request, deadline, admitted).await
}

/// Resolve an npm project: verify the client's input lockfile under the
/// client's policy, resolve against the client's registries, and stream
/// the result back as NDJSON.
async fn resolve_npm_request(
    runtime: &Resolver,
    identity: Identity,
    request: ResolveRequest,
    deadline: tokio::time::Instant,
    admitted: AdmittedResolve<'_>,
) -> Response {
    let mut inputs =
        match prepare_npm_resolve(runtime, &identity, request, deadline, admitted).await {
            Ok(inputs) => inputs,
            Err(response) => return response,
        };
    let verified_dist_stats = match verify_request_lockfile(runtime, &inputs).await {
        Ok(stats) => stats,
        Err(response) => return response,
    };
    if let Some(response) =
        short_circuit_npm_resolve(runtime, &identity, &mut inputs, verified_dist_stats)
    {
        return response;
    }
    if tokio::time::Instant::now() >= inputs.deadline {
        return request_timeout_response();
    }
    stream_resolve_response(runtime, inputs)
}

fn short_circuit_npm_resolve(
    runtime: &Resolver,
    identity: &Identity,
    inputs: &mut StreamedResolveInputs,
    verified_dist_stats: Option<ObservedDistStats>,
) -> Option<Response> {
    if let Some(response) = frozen_lockfile_response(
        runtime,
        &inputs.config,
        &inputs.request,
        &inputs.tarball_router,
        verified_dist_stats,
        inputs.deadline,
    ) {
        return Some(within_deadline(response, inputs.deadline));
    }
    inputs.cache_key = resolution_cache_key(&inputs.request);
    cached_resolution_response(
        runtime,
        identity,
        inputs.cache_key.as_ref(),
        &inputs.request,
        inputs.deadline,
    )
    .map(|response| within_deadline(response, inputs.deadline))
}

fn within_deadline(response: Response, deadline: tokio::time::Instant) -> Response {
    if tokio::time::Instant::now() < deadline { response } else { request_timeout_response() }
}

async fn prepare_npm_resolve(
    runtime: &Resolver,
    identity: &Identity,
    request: ResolveRequest,
    deadline: tokio::time::Instant,
    admitted: AdmittedResolve<'_>,
) -> Result<StreamedResolveInputs, Response> {
    if let Some(response) =
        reject_unusable_resolve(&request, &runtime.route_context, runtime.limits.max_npm_packages)
    {
        return Err(response);
    }
    // Validate and intern config before waiting for a running slot while the
    // caller's admission permit prevents shed load from evicting warm entries.
    let config = runtime.config_for(&request)?;
    let permit = admitted.acquire(deadline).await?;
    // Auth is selected by this server's route policy for the caller, not
    // forwarded from the client. Every metadata/tarball fetch the
    // resolve+verify performs records its route into `footprint`, which
    // then decides whether the resolution may populate the shared cache.
    let footprint = Arc::new(Mutex::new(Footprint::default()));
    let request_auth = runtime.hooked_auth(&request, identity, &footprint);
    let tarball_router = request_tarball_router(runtime, identity, &config);
    Ok(StreamedResolveInputs {
        config,
        request,
        request_auth,
        tarball_router,
        footprint,
        cache_key: None,
        permit: Arc::new(permit),
        deadline,
    })
}

/// The request-level refusals a resolve is held to before any fetch.
pub(super) fn reject_unusable_resolve(
    request: &ResolveRequest,
    context: &RouteContext,
    max_packages: usize,
) -> Option<Response> {
    reject_oversized_graph(request, max_packages)
        .or_else(|| reject_invalid_registries(request))
        .or_else(|| reject_invalid_patch_hashes(request))
        .or_else(|| reject_inline_url_auth(request))
        .or_else(|| reject_unsafe_publish_directories(request))
        .or_else(|| reject_off_allowlist_fetches(request, context))
        .or_else(|| reject_incompatible_lockfile_format(request))
}

/// A verified frozen lockfile is the whole answer: no incremental tree walk
/// runs. Its tarballs are still announced as `package` frames when the
/// verification fan-out just fetched their metadata — the sizes let the client
/// start the largest downloads first. On a verdict-cache hit no metadata was
/// fetched, so there is nothing to add and the response is the bare `done`
/// frame.
fn frozen_lockfile_response(
    runtime: &Resolver,
    config: &PacquetConfig,
    request: &ResolveRequest,
    tarball_router: &TarballRouter,
    verified_dist_stats: Option<ObservedDistStats>,
    deadline: tokio::time::Instant,
) -> Option<Response> {
    let expired = || tokio::time::Instant::now() >= deadline;
    let lockfile = match prepare_frozen_lockfile(config, request, tarball_router, &expired)? {
        Ok(lockfile) => lockfile,
        Err(timeout) => return Some(timeout),
    };
    if let Some(osv_index) = runtime.osv_index.as_ref() {
        let Some(violations) = osv_violations_for_lockfile_until(osv_index, &lockfile, &expired)
        else {
            return Some(request_timeout_response());
        };
        if !violations.is_empty() {
            return Some(ndjson_single_frame(&violations_frame(&violations)));
        }
    }
    Some(serialize_frozen_frames(
        config,
        request,
        tarball_router,
        &lockfile,
        verified_dist_stats,
        &expired,
    ))
}

fn prepare_frozen_lockfile(
    config: &PacquetConfig,
    request: &ResolveRequest,
    tarball_router: &TarballRouter,
    should_cancel: &dyn Fn() -> bool,
) -> Option<Result<pnpm_lockfile::Lockfile, Response>> {
    let lockfile = resolve::fresh_frozen_input_lockfile(config, request)?;
    let verified = tarball_router.verification_lockfile(&lockfile);
    Some(
        tarball_router
            .route_lockfile_until(config, &verified, should_cancel)
            .ok_or_else(request_timeout_response),
    )
}

fn serialize_frozen_frames(
    config: &PacquetConfig,
    request: &ResolveRequest,
    tarball_router: &TarballRouter,
    lockfile: &pnpm_lockfile::Lockfile,
    verified_dist_stats: Option<ObservedDistStats>,
    should_cancel: &dyn Fn() -> bool,
) -> Response {
    let Some(done_res) = try_done_frame_until(
        lockfile,
        &protocol::AppliedPolicies::from_request(request),
        should_cancel,
    ) else {
        return request_timeout_response();
    };
    let done = match done_res {
        Ok(done) => done,
        Err(error_frame) => return ndjson_single_frame(&error_frame),
    };
    let mut frames = match verified_dist_stats {
        Some(sizes) => {
            let Some(frames) = frozen_package_frames_until(
                config,
                tarball_router,
                lockfile,
                &sizes,
                should_cancel,
            ) else {
                return request_timeout_response();
            };
            frames
        }
        None => Vec::new(),
    };
    frames.push(done);
    ndjson_frames_until(frames, should_cancel).unwrap_or_else(request_timeout_response)
}

/// A stored lockfile this caller may reuse.
///
/// The OSV index is immutable for this resolver instance and a lockfile is only
/// stored after passing the OSV check, so a cache hit is already OSV-clean — no
/// per-package re-scan is needed on this warm path.
fn cached_resolution_response(
    runtime: &Resolver,
    identity: &Identity,
    key: Option<&String>,
    request: &ResolveRequest,
    deadline: tokio::time::Instant,
) -> Option<Response> {
    let lockfile = cached_resolution(
        &runtime.cache.entries,
        runtime.cache.ttl,
        key?,
        &runtime.route_context,
        identity,
    )?;
    let expired = || tokio::time::Instant::now() >= deadline;
    let Some(done_res) = try_done_frame_until(
        &lockfile,
        &protocol::AppliedPolicies::from_request(request),
        &expired,
    ) else {
        return Some(request_timeout_response());
    };
    Some(ndjson_single_frame(&done_res.unwrap_or_else(|err| err)))
}
