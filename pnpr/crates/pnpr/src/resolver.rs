//! pnpr resolver: server-side dependency resolution exposed as an
//! additive, opt-in protocol alongside pnpr's npm-compatible API. The
//! handshake + endpoint are served under one base URL (the `pnprServer`).
//!
//! Two routes, built on pacquet's resolver:
//!
//! * `GET /-/pnpr` — capability handshake; advertises the supported
//!   protocol versions so a client can negotiate or fail fast.
//! * `POST /-/pnpr/v0/resolve` — resolve a project **against the registries
//!   the client sends** (so the server uses the same source of truth as
//!   the client), verify the client's input lockfile under the client's
//!   policy, and **stream** the result back as NDJSON: one `package`
//!   frame per resolved tarball as the tree walk yields it, then a
//!   terminal `done` frame carrying the full lockfile (or an `error` /
//!   `violations` frame). The client fetches each tarball the moment its
//!   frame arrives, so download overlaps the server's resolution
//!   ([pnpm/pnpm#12234](https://github.com/pnpm/pnpm/issues/12234)),
//!   then fetches the rest in parallel like a normal install
//!   ([pnpm/pnpm#12230](https://github.com/pnpm/pnpm/issues/12230)).
//! * `POST /-/pnpr/v0/verify-lockfile` — verify an already-fresh client
//!   lockfile under the same policy without resolving. A frozen restore
//!   can start local fetch/materialization immediately and only use this
//!   endpoint as the trust verdict.
//!
//! pnpr is a stateless resolver: it stores no tarballs. Public tarballs
//! can still be fetched directly from their upstream registry, while a
//! private proxied route is rewritten to the upstream's `/~<name>/`
//! registry endpoint so upstream URLs and credentials stay server-side.
//!
//! The client's `registry`, `namedRegistries`, `overrides`, and the
//! verification policy (`minimumReleaseAge`, `trustPolicy`, ...) drive
//! resolution and verification. When the client sends its on-disk
//! lockfile, the server verifies it under the client's policy before
//! resolving, then reuses it as the resolution seed (frozen → as-is;
//! non-frozen → reuse-and-update). A multi-project workspace is resolved
//! by reconstructing the workspace on disk (root manifest +
//! `pnpm-workspace.yaml` + member manifests) and letting pacquet's
//! install path discover and resolve every importer. The client
//! authenticates to pnpr (its request `Authorization` identifies the
//! caller) but does not forward its own upstream registry credentials:
//! pnpr selects upstream auth from its route policy (see [`pnpr_route`]),
//! so private dependencies resolve via a pnpr-managed upstream credential or
//! fail closed.

pub(crate) use verify_lockfile::handle_verify_lockfile;

mod streaming;
use streaming::{StreamedResolveInputs, stream_resolve_response};

mod verify_lockfile;
use verify_lockfile::{VerifyFailure, verify_input_lockfile};

mod config_cache;
use config_cache::{
    MAX_CONFIG_KEY_BYTES, MAX_INTERNED_CONFIGS, TOO_MANY_CONFIGS_MESSAGE, intern_config,
};

mod cache;
mod cargo;
mod package_route;
mod protocol;
mod pypi;
mod request_validation;
mod resolve;
mod verdict_cache;
mod wire;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex, OnceLock},
    time::Duration,
};

use pnpr_config::Config as RegistryConfig;
use pnpr_osv::OsvIndex;
use pnpr_policy::Identity;
use pnpr_registry::Ecosystem;
use pnpr_route::{Footprint, RouteContext, RouteHook};

use axum::{
    body::{Body, Bytes},
    http::{StatusCode, header},
    response::Response,
};
use indexmap::IndexMap;
use pnpm_config::Config as PacquetConfig;
use pnpm_lockfile::Lockfile;
use pnpm_lockfile_verification::{collect_resolution_policy_violations, hash_lockfile};
use pnpm_network::{AuthHeaders, ThrottledClient, UpstreamRouteHook};
use pnpm_package_manager::build_resolution_verifiers;
use pnpm_resolving_npm_resolver::{
    InMemoryPackageMetaCache, ObservedDistStats, PackageMetaCache, observed_dist_stats_sink,
};
use pnpm_resolving_resolver_base::{PackageVersionGuard, ResolutionVerifier};
use pnpm_store_dir::StoreDir;

use self::{
    cache::{CachedResolution, cached_resolution, resolution_cache_key, store_resolution},
    protocol::{EcosystemProbe, ResolveRequest},
    request_validation::{
        reject_inline_url_auth, reject_invalid_patch_hashes, reject_invalid_registries,
        reject_off_allowlist_fetches,
    },
    verdict_cache::VerdictCache,
    wire::{
        StreamObserver, TarballRouter, done_frame, error_frame, frozen_package_frames,
        ndjson_frames, ndjson_single_frame, ndjson_stream_response, osv_violations_for_lockfile,
        verify_done_or_osv_violations, violations_frame,
    },
};

/// Per-server engine backing the pnpr install endpoint: it holds the
/// store, cache, and HTTP client used to resolve a client's project. The
/// store and cache dirs are fixed for the server's lifetime; the
/// *registries* come from each client request (the server resolves
/// against the client's registries, not its own), so the `&'static Config`
/// the install path requires is interned per distinct client registry
/// configuration rather than leaked once or per request.
///
/// Held lazily in a [`OnceLock`] on the server's state so servers that
/// never receive such a request pay nothing, and so each server in
/// a multi-server test process keeps its own store.
pub(crate) struct Resolver {
    store_dir: StoreDir,
    cache_dir: PathBuf,
    client: Arc<ThrottledClient>,
    /// Held behind an [`Arc`] so the detached streaming-resolve task can
    /// own a clone and record its result after the response body has
    /// already started flowing to the client.
    resolution_cache: Arc<Mutex<HashMap<String, Vec<CachedResolution>>>>,
    resolution_cache_ttl: Duration,
    /// One leaked `Config` per distinct client registry configuration,
    /// keyed by its canonical JSON. Capped at [`MAX_INTERNED_CONFIGS`] so a
    /// caller varying its registry/policy fields can't grow the leak
    /// without bound; see [`intern_config`].
    configs: Mutex<HashMap<String, &'static PacquetConfig>>,
    /// SQLite-backed whole-lockfile verification verdict cache. `None`
    /// only if the database couldn't be opened — verification then runs
    /// every time (uncached) rather than failing the server.
    verdict_cache: Option<VerdictCache>,
    osv_index: Option<Arc<OsvIndex>>,
    /// Route-classification inputs (public/private rules, pnpr-managed
    /// upstream credentials, hosted origin, package policy), resolved once
    /// from the server config and combined per request with the caller's
    /// identity to drive auth selection and footprint recording.
    route_context: Arc<RouteContext>,
    /// Public URL clients use for pnpr-hosted and `/~<name>/` endpoint
    /// tarball URLs.
    public_url: String,
    /// How long a cached Cargo sparse-index file stays fresh: the
    /// server's `packument_ttl`, so index metadata ages out on the same
    /// schedule npm packuments do.
    cargo_index_ttl: Duration,
    /// Serializes the fetch of one Cargo sparse-index file, so concurrent
    /// resolves of the same cold graph fetch each entry once.
    cargo_index_locks: Arc<crate::server::StripedLocks>,
    /// The same, for a Python index's project pages and metadata files.
    python_index_locks: Arc<crate::server::StripedLocks>,
    /// HMAC secret namespacing a private footprint's cache descriptor.
    /// Part 1 uses it only to label each resolve's cache class in the
    /// operator debug log; Part 2 keys private cache entries by it.
    resolution_cache_secret: Arc<[u8]>,
}

impl Resolver {
    pub(crate) fn get_or_init<'a>(
        cell: &'a OnceLock<Resolver>,
        config: &RegistryConfig,
        osv_index: Option<Arc<OsvIndex>>,
    ) -> &'a Resolver {
        cell.get_or_init(|| Resolver::build(config, osv_index))
    }

    fn build(config: &RegistryConfig, osv_index: Option<Arc<OsvIndex>>) -> Resolver {
        let store_dir = config.cache_storage.join("pnpr-store");
        let cache_dir = config.cache_storage.join("pnpr-cache");
        // Best-effort: a real failure here (e.g. a permission problem)
        // resurfaces with a precise error on the first store/cache write
        // during resolution, so there's nothing actionable to report yet.
        let _ = std::fs::create_dir_all(&store_dir);
        let _ = std::fs::create_dir_all(&cache_dir);
        let verdict_cache = VerdictCache::open(&cache_dir.join("lockfile-verdicts.sqlite")).ok();
        let route_context = Arc::new(RouteContext::from_config(config));
        // Re-validate every redirect hop against the same fetch allowlist the
        // request boundary uses, so an allowlisted registry that redirects to
        // an off-allowlist host cannot slip a server-side fetch past it (SSRF).
        let redirect_context = Arc::clone(&route_context);
        let client = Arc::new(ThrottledClient::new_for_installs_with_redirect_guard(move |url| {
            redirect_context.allows_registry(url.as_str())
        }));
        Resolver {
            store_dir: StoreDir::new(store_dir),
            cache_dir,
            client,
            resolution_cache: Arc::new(Mutex::new(HashMap::new())),
            resolution_cache_ttl: config.packument_ttl,
            configs: Mutex::new(HashMap::new()),
            verdict_cache,
            osv_index,
            route_context,
            public_url: config.public_url.clone(),
            cargo_index_ttl: config.packument_ttl,
            cargo_index_locks: Arc::new(crate::server::StripedLocks::new()),
            python_index_locks: Arc::new(crate::server::StripedLocks::new()),
            resolution_cache_secret: Arc::clone(&config.resolution_cache_secret),
        }
    }

    /// Build the request's [`AuthHeaders`] with the route hook installed:
    /// every metadata/tarball fetch is classified against this server's
    /// route policy for `identity`, the pnpr-managed credential (never the
    /// client's) is selected, and the touched private routes accumulate in
    /// `footprint`. The client's forwarded `auth_headers` are kept on the
    /// value (so `to_by_scope` still reflects them) but no longer consulted.
    fn hooked_auth(
        &self,
        request: &ResolveRequest,
        identity: &Identity,
        footprint: &Arc<Mutex<Footprint>>,
    ) -> Arc<AuthHeaders> {
        let hook: Arc<dyn UpstreamRouteHook> = Arc::new(RouteHook::new(
            Arc::clone(&self.route_context),
            identity.clone(),
            Arc::clone(footprint),
            Arc::clone(&self.resolution_cache_secret),
        ));
        Arc::new(AuthHeaders::from_by_scope(request.auth_headers.clone()).with_route_hook(hook))
    }

    /// Where `registry`'s Cargo sparse-index files are cached. The origin
    /// is hashed into the path so two registries serving the same crate
    /// name never share an entry; the caller's route scope adds the last
    /// namespace segment at fetch time.
    fn cargo_index_cache_dir(&self, registry: &str) -> PathBuf {
        self.cache_dir.join("cargo-index").join(pnpm_crypto_hash::create_hex_hash(registry))
    }

    /// Where `index`'s Python documents are cached. As with Cargo, the
    /// origin is hashed into the path so two indexes serving the same
    /// project never share an entry.
    fn python_index_cache_dir(&self, index: &str) -> PathBuf {
        self.cache_dir.join("python-index").join(pnpm_crypto_hash::create_hex_hash(index))
    }

    /// Resolve (or build + intern) the `&'static Config` for a request's
    /// registry configuration. Pacquet's install path resolves against
    /// `config.registry` / `registries_by_prefix` / `overrides`, so a request
    /// from a client with a different registry setup gets its own Config.
    ///
    /// `None` once [`MAX_INTERNED_CONFIGS`] distinct configurations have
    /// been interned — see [`intern_config`].
    fn config_for(&self, request: &ResolveRequest) -> Option<&'static PacquetConfig> {
        intern_config(
            &self.configs,
            &self.store_dir,
            &self.cache_dir,
            request,
            MAX_INTERNED_CONFIGS,
            MAX_CONFIG_KEY_BYTES,
        )
    }
}

/// Whether `/-/pnpr/v0/resolve` resolves this ecosystem.
///
/// The handshake advertises what this admits and [`handle_resolve`] dispatches
/// on it, so the two cannot drift: a list kept beside the dispatch could
/// advertise an ecosystem the dispatch turns away, and both are exhaustive
/// matches, so a new ecosystem stops here for a decision.
pub(crate) const fn resolves(ecosystem: Ecosystem) -> bool {
    match ecosystem {
        Ecosystem::Npm | Ecosystem::Cargo | Ecosystem::Pypi => true,
        // An image has no dependency graph to resolve.
        Ecosystem::Oci => false,
    }
}

fn refuse_unresolvable(ecosystem: Ecosystem) -> Response {
    json_error(
        StatusCode::BAD_REQUEST,
        &format!("{ecosystem} projects have no dependency graph for this endpoint to resolve"),
    )
}

/// The ecosystems the handshake advertises, in the enum's own order.
pub(crate) fn resolved_ecosystems() -> impl Iterator<Item = Ecosystem> {
    Ecosystem::all().filter(|ecosystem| resolves(*ecosystem))
}

/// Handle `POST /-/pnpr/v0/resolve`. One address serves every ecosystem;
/// the body's `ecosystem` field selects which resolver reads it, and a
/// body without one means npm.
pub(crate) async fn handle_resolve(
    runtime: &Resolver,
    identity: Identity,
    body: Bytes,
) -> Response {
    let probe: EcosystemProbe = match serde_json::from_slice(&body) {
        Ok(probe) => probe,
        Err(err) => return json_error(StatusCode::BAD_REQUEST, &err.to_string()),
    };
    if !resolves(probe.ecosystem) {
        return refuse_unresolvable(probe.ecosystem);
    }
    match probe.ecosystem {
        Ecosystem::Npm => handle_npm_resolve(runtime, identity, &body).await,
        Ecosystem::Cargo => cargo::handle_resolve(runtime, identity, &body).await,
        // Listed rather than caught, so an ecosystem added to the shared
        // enum stops here for a decision instead of being refused silently.
        Ecosystem::Pypi => pypi::handle_resolve(runtime, identity, &body).await,
        // An arm rather than a catch-all so an ecosystem added to the shared
        // enum has to decide here too, even though `resolves` turns this one
        // away before the dispatch runs.
        Ecosystem::Oci => refuse_unresolvable(probe.ecosystem),
    }
}

async fn handle_npm_resolve(runtime: &Resolver, identity: Identity, body: &[u8]) -> Response {
    let request: ResolveRequest = match serde_json::from_slice(body) {
        Ok(request) => request,
        Err(err) => return json_error(StatusCode::BAD_REQUEST, &err.to_string()),
    };

    resolve_npm_request(runtime, identity, request).await
}

async fn resolve_npm_request(
    runtime: &Resolver,
    identity: Identity,
    request: ResolveRequest,
) -> Response {
    if let Some(response) = reject_unusable_resolve(&request, &runtime.route_context) {
        return response;
    }

    // Resolve against the client's registries, not the server's own.
    let Some(config) = runtime.config_for(&request) else {
        return json_error(StatusCode::SERVICE_UNAVAILABLE, TOO_MANY_CONFIGS_MESSAGE);
    };
    // Auth is selected by this server's route policy for the caller, not
    // forwarded from the client. Every metadata/tarball fetch the
    // resolve+verify performs records its route into `footprint`, which
    // then decides whether the resolution may populate the shared cache.
    let footprint = Arc::new(Mutex::new(Footprint::default()));
    let request_auth = runtime.hooked_auth(&request, &identity, &footprint);
    let tarball_router = request_tarball_router(runtime, &identity, config);

    // Verify the *input* lockfile under the client's policy before any
    // package is streamed ([pnpm/pnpm#12139](https://github.com/pnpm/pnpm/issues/12139)).
    // The client skips its own `verifyLockfileResolutions` whenever a
    // pnpr server is configured, so this is the only place the
    // committed/reused entries get checked. A true first install sends
    // no lockfile — nothing to verify. `trustLockfile` is the client's
    // opt-out (mirrors the local path's `--trust-lockfile`). Freshly-
    // resolved entries are held to the same policy by the resolver's
    // pick-time gate (the policy is wired into `config`).
    let verified_dist_stats =
        match verify_request_lockfile(runtime, config, &request, &request_auth, &tarball_router)
            .await
        {
            Ok(stats) => stats,
            Err(response) => return response,
        };

    // Short-circuit paths that produce the whole lockfile without an
    // incremental tree walk. A verified frozen lockfile still announces
    // its tarballs as `package` frames when the verification fan-out
    // just fetched their metadata — the sizes let the client start the
    // largest downloads first. On a verdict-cache hit no metadata was
    // fetched, so there's nothing to add and the response is the bare
    // `done` frame.
    if let Some(response) =
        frozen_lockfile_response(runtime, config, &request, &tarball_router, verified_dist_stats)
    {
        return response;
    }
    // The base key is auth-excluded and shared by every candidate for the
    // same resolution inputs. Candidate footprints decide which callers
    // may reuse a stored lockfile.
    let resolution_cache_key = resolution_cache_key(config, &request);
    if let Some(response) =
        cached_resolution_response(runtime, &identity, resolution_cache_key.as_ref())
    {
        return response;
    }

    stream_resolve_response(
        runtime,
        StreamedResolveInputs {
            config,
            request,
            request_auth,
            tarball_router,
            footprint,
            cache_key: resolution_cache_key,
        },
    )
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
async fn verify_request_lockfile(
    runtime: &Resolver,
    config: &'static PacquetConfig,
    request: &ResolveRequest,
    request_auth: &Arc<AuthHeaders>,
    tarball_router: &TarballRouter,
) -> Result<Option<ObservedDistStats>, Response> {
    if request.trust_lockfile {
        return Ok(None);
    }
    let Some(input_lockfile) = request.lockfile.as_ref() else {
        return Ok(None);
    };
    let input_lockfile = tarball_router.verification_lockfile(input_lockfile);
    match verify_input_lockfile(runtime, config, request_auth, &input_lockfile).await {
        Ok(stats) => Ok(stats),
        Err(VerifyFailure::Internal(response)) => Err(response),
        Err(VerifyFailure::Violations(violations)) => {
            Err(ndjson_single_frame(&violations_frame(&violations)))
        }
    }
}

/// Resolve an npm project: verify the client's input lockfile under the
/// client's policy, resolve against the client's registries, and stream
/// the result back as NDJSON.
///
/// The response is `application/x-ndjson`: one `package` frame per
/// resolved tarball as the server's tree walk yields it (so the client
/// fetches tarballs while the server is still resolving —
/// [pnpm/pnpm#12234](https://github.com/pnpm/pnpm/issues/12234)),
/// followed by exactly one terminal frame: `done` carrying the full
/// lockfile + stats, `error` if resolution aborts mid-stream, or
/// `violations` if the input lockfile failed the client's policy. The
/// short-circuit paths (frozen reuse, cache hit) emit only the terminal
/// `done` frame. A private proxied tarball is announced through its
/// upstream's `/~<name>/` registry endpoint rather than its upstream URL.
/// The request-level refusals a resolve is held to before any fetch.
fn reject_unusable_resolve(request: &ResolveRequest, context: &RouteContext) -> Option<Response> {
    reject_invalid_registries(request)
        .or_else(|| reject_invalid_patch_hashes(request))
        .or_else(|| reject_inline_url_auth(request))
        .or_else(|| reject_off_allowlist_fetches(request, context))
}

/// A verified frozen lockfile is the whole answer: no incremental tree walk
/// runs. Its tarballs are still announced as `package` frames when the
/// verification fan-out just fetched their metadata — the sizes let the client
/// start the largest downloads first. On a verdict-cache hit no metadata was
/// fetched, so there is nothing to add and the response is the bare `done`
/// frame.
fn frozen_lockfile_response(
    runtime: &Resolver,
    config: &'static PacquetConfig,
    request: &ResolveRequest,
    tarball_router: &TarballRouter,
    verified_dist_stats: Option<ObservedDistStats>,
) -> Option<Response> {
    let lockfile = resolve::fresh_frozen_input_lockfile(config, request)?;
    let lockfile = tarball_router.verification_lockfile(&lockfile);
    let lockfile = tarball_router.route_lockfile(config, &lockfile);
    if let Some(osv_index) = runtime.osv_index.as_ref() {
        let violations = osv_violations_for_lockfile(osv_index, &lockfile);
        if !violations.is_empty() {
            return Some(ndjson_single_frame(&violations_frame(&violations)));
        }
    }
    let mut frames = verified_dist_stats
        .map(|sizes| frozen_package_frames(config, tarball_router, &lockfile, &sizes))
        .unwrap_or_default();
    frames.push(done_frame(&lockfile));
    Some(ndjson_frames(&frames))
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
) -> Option<Response> {
    let lockfile = cached_resolution(
        &runtime.resolution_cache,
        runtime.resolution_cache_ttl,
        key?,
        &runtime.route_context,
        identity,
    )?;
    Some(ndjson_single_frame(&done_frame(&lockfile)))
}

/// What a finished resolution offers the resolution cache.
struct StoreCandidate<'a> {
    cache: &'a Mutex<HashMap<String, Vec<CachedResolution>>>,
    cache_ttl: Duration,
    key: String,
    footprint: &'a Mutex<Footprint>,
    cache_secret: &'a [u8],
    lockfile: &'a Lockfile,
}

/// Offer a finished resolution to the cache, logging what a private one was
/// judged on.
fn store_resolution_candidate(candidate: StoreCandidate<'_>) {
    let footprint = candidate.footprint.lock().expect("footprint poisoned").clone();
    let descriptor = footprint.digest(candidate.cache_secret);
    let cached = store_resolution(
        candidate.cache,
        candidate.cache_ttl,
        candidate.key,
        footprint.clone(),
        candidate.cache_secret,
        candidate.lockfile,
    );
    if footprint.is_public() {
        return;
    }
    tracing::debug!(
        cached,
        descriptor = descriptor.as_deref().unwrap_or("none"),
        "private resolution cache candidate evaluated",
    );
}

/// A diagnostic's message and its causes on one line. A resolve failure
/// rides an NDJSON frame, where miette's rendered report would arrive as
/// an unreadable block of escaped newlines.
fn report_message(report: &miette::Report) -> String {
    report.chain().map(ToString::to_string).collect::<Vec<_>>().join(": ")
}

fn json_error(status: StatusCode, message: &str) -> Response {
    let body = serde_json::json!({ "error": message }).to_string();
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .expect("static json error response is always valid")
}

#[cfg(test)]
mod tests;

fn request_tarball_router(
    runtime: &Resolver,
    identity: &Identity,
    config: &PacquetConfig,
) -> TarballRouter {
    TarballRouter::new(
        Arc::clone(&runtime.route_context),
        identity.clone(),
        runtime.public_url.clone(),
        config.resolved_registries().into_iter().collect(),
    )
}
