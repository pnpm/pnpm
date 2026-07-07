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
//!   can start local fetch/materialization immediately and use this
//!   endpoint as the trust verdict. When the verification fan-out fetched
//!   packument metadata it also emits the same sized `package` frames the
//!   resolve frozen fast path does, ahead of the verdict, so the client can
//!   prioritize its largest pending tarball downloads.
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
//! pnpr selects upstream auth from its route policy (see [`crate::route`]),
//! so private dependencies resolve via a pnpr-managed upstream credential or
//! fail closed.

pub(crate) mod osv;
mod protocol;
mod resolve;
mod verdict_cache;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use crate::{
    config::Config as RegistryConfig,
    package_name::PackageName,
    policy::Identity,
    route::{
        Footprint, RouteClass, RouteContext, RouteHook, sanitize_registry_tarball_url,
        strip_url_credentials,
    },
    upstream::tarball_basename,
};

use axum::{
    body::{Body, Bytes},
    http::{StatusCode, header},
    response::Response,
};
use indexmap::IndexMap;
use pacquet_config::Config as PacquetConfig;
use pacquet_lockfile::{
    Lockfile, LockfileResolution, TarballResolution, is_git_hosted_tarball_url,
    pick_registry_for_package,
};
use pacquet_lockfile_verification::{collect_resolution_policy_violations, hash_lockfile};
use pacquet_network::{AuthHeaders, ThrottledClient, UpstreamRouteHook};
use pacquet_package_manager::{
    ResolvedPackageHint, build_resolution_verifiers, tarball_url_and_integrity,
};
use pacquet_resolving_npm_resolver::{
    InMemoryPackageMetaCache, ObservedDistStats, PackageMetaCache, observed_dist_stats_sink,
};
use pacquet_resolving_resolver_base::{PackageVersionGuard, ResolutionVerifier};
use pacquet_store_dir::StoreDir;
use sha2::{Digest, Sha256};

pub(crate) use self::osv::{OsvIndex, format_advisory_ids, load_osv_index};

use self::{protocol::ResolveRequest, verdict_cache::VerdictCache};

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
    /// HMAC secret namespacing a private footprint's cache descriptor.
    /// Part 1 uses it only to label each resolve's cache class in the
    /// operator debug log; Part 2 keys private cache entries by it.
    resolution_cache_secret: Arc<[u8]>,
}

struct CachedResolution {
    lockfile: Lockfile,
    inserted: Instant,
    last_used: Instant,
    footprint: Footprint,
    descriptor_digest: Option<String>,
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

    /// Resolve (or build + intern) the `&'static Config` for a request's
    /// registry configuration. Pacquet's install path resolves against
    /// `config.registry` / `named_registries` / `overrides`, so a request
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

/// Hard cap on how many distinct client configurations the server will
/// intern. Each interned [`PacquetConfig`] is leaked (the install path
/// requires a `&'static Config`), so without a cap an authenticated
/// caller could exhaust memory by varying its registry/policy fields on
/// every request. `1024` is far above the handful of distinct setups a
/// real fleet produces (typically one), matching
/// [`MAX_RESOLUTION_CACHE_ENTRIES`].
const MAX_INTERNED_CONFIGS: usize = 1024;

/// Returned (as a `503`) when [`MAX_INTERNED_CONFIGS`] is reached. The
/// limit resets on restart and a real client reuses one configuration, so
/// a legitimate caller never sees it.
const TOO_MANY_CONFIGS_MESSAGE: &str = "too many distinct registry configurations";

/// Hard cap on the byte size of a single interned config's canonical key,
/// which carries its attacker-controlled `registry` / `namedRegistries` /
/// `overrides` content. [`MAX_INTERNED_CONFIGS`] bounds only the *count* of
/// leaked configs; without this a caller could pad each distinct config with
/// a giant overrides/named-registries map and still amplify the per-request
/// leak (the whole request body is allowed up to the publish-sized limit).
/// `128 KiB` is far above any real registry/overrides configuration.
const MAX_CONFIG_KEY_BYTES: usize = 128 * 1024;

/// Build + leak a `&'static Config` for a request's registry
/// configuration, interned by its canonical JSON so repeat requests reuse
/// it. Returns `None` when the config can't be safely interned:
///
/// * once `max_interned` distinct configurations have been interned — a
///   leaked config can never be reclaimed, so refusing to leak more is the
///   only real bound on the per-request leak (eviction would just let the
///   same key be re-leaked); or
/// * when a single config's canonical key exceeds `max_key_bytes`, which
///   bounds the *size* of each leaked config so a caller can't amplify the
///   leak with a giant `overrides` / `namedRegistries` map.
///
/// Both caps are generous enough that legitimate clients (which reuse one
/// small configuration) never hit them.
fn intern_config(
    configs: &Mutex<HashMap<String, &'static PacquetConfig>>,
    store_dir: &StoreDir,
    cache_dir: &Path,
    request: &ResolveRequest,
    max_interned: usize,
    max_key_bytes: usize,
) -> Option<&'static PacquetConfig> {
    let registry =
        request.registry.clone().unwrap_or_else(|| "https://registry.npmjs.org/".to_string());
    let registry = if registry.ends_with('/') { registry } else { format!("{registry}/") };
    let overrides: Option<IndexMap<String, String>> =
        request.overrides.as_ref().and_then(|value| serde_json::from_value(value.clone()).ok());
    // Key on a sorted view of `overrides`: serde_json preserves insertion order
    // and `IndexMap` is insertion-ordered, so the same overrides sent with a
    // different key order would otherwise hash to distinct cache keys and intern
    // duplicate leaked configs — defeating dedup and burning the cap faster.
    let overrides_key: Option<std::collections::BTreeMap<&str, &str>> = overrides
        .as_ref()
        .map(|overrides| overrides.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect());

    let key = serde_json::json!({
        "registry": registry,
        "namedRegistries": request.named_registries,
        "overrides": overrides_key,
        "minimumReleaseAge": request.minimum_release_age,
        "minimumReleaseAgeExclude": request.minimum_release_age_exclude,
        "minimumReleaseAgeIgnoreMissingTime": request.minimum_release_age_ignore_missing_time,
        "trustPolicy": request.trust_policy,
        "trustPolicyExclude": request.trust_policy_exclude,
        "trustPolicyIgnoreAfter": request.trust_policy_ignore_after,
    })
    .to_string();
    if key.len() > max_key_bytes {
        return None;
    }

    let mut configs = configs.lock().expect("config cache poisoned");
    if let Some(config) = configs.get(&key) {
        return Some(config);
    }
    if configs.len() >= max_interned {
        return None;
    }

    let mut config = PacquetConfig::new();
    config.store_dir = store_dir.clone();
    config.cache_dir = cache_dir.to_path_buf();
    config.registry = registry;
    config.named_registries = request.named_registries.clone();
    config.overrides = overrides;
    config.modules_dir = PathBuf::from("node_modules");
    config.lockfile = true;
    config.verify_store_integrity = true;
    // The client's verification policy drives both the input-lockfile
    // verifier and the resolver's pick-time `minimumReleaseAge` /
    // `trustPolicy` checks, so newly-resolved entries are held to the
    // same policy as the reused ones.
    config.minimum_release_age = request.minimum_release_age;
    config.minimum_release_age_exclude.clone_from(&request.minimum_release_age_exclude);
    if let Some(ignore_missing_time) = request.minimum_release_age_ignore_missing_time {
        config.minimum_release_age_ignore_missing_time = ignore_missing_time;
    }
    config.trust_policy = request.trust_policy;
    config.trust_policy_exclude.clone_from(&request.trust_policy_exclude);
    config.trust_policy_ignore_after = request.trust_policy_ignore_after;
    let config: &'static PacquetConfig = config.leak();
    configs.insert(key, config);
    Some(config)
}

/// Handle `POST /-/pnpr/v0/resolve`: verify the client's input lockfile under
/// the client's policy, resolve against the client's registries, and
/// stream the result back as NDJSON.
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
pub(crate) async fn handle_resolve(
    runtime: &Resolver,
    identity: Identity,
    body: Bytes,
) -> Response {
    let request: ResolveRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(err) => return json_error(StatusCode::BAD_REQUEST, &err.to_string()),
    };

    if let Some(response) = reject_inline_url_auth(&request) {
        return response;
    }

    if let Some(response) = reject_off_allowlist_fetches(&request, &runtime.route_context) {
        return response;
    }

    // Resolve against the client's registries, not the server's own.
    let Some(config) = runtime.config_for(&request) else {
        return json_error(StatusCode::SERVICE_UNAVAILABLE, TOO_MANY_CONFIGS_MESSAGE);
    };
    let package_version_guard =
        runtime.osv_index.as_ref().map(|index| Arc::clone(index) as Arc<dyn PackageVersionGuard>);

    // Auth is selected by this server's route policy for the caller, not
    // forwarded from the client. Every metadata/tarball fetch the
    // resolve+verify performs records its route into `footprint`, which
    // then decides whether the resolution may populate the shared cache.
    let footprint = Arc::new(Mutex::new(Footprint::default()));
    let request_auth = runtime.hooked_auth(&request, &identity, &footprint);
    let tarball_router = TarballRouter::new(
        Arc::clone(&runtime.route_context),
        identity.clone(),
        runtime.public_url.clone(),
        config.resolved_registries().into_iter().collect(),
    );

    // Verify the *input* lockfile under the client's policy before any
    // package is streamed ([pnpm/pnpm#12139](https://github.com/pnpm/pnpm/issues/12139)).
    // The client skips its own `verifyLockfileResolutions` whenever a
    // pnpr server is configured, so this is the only place the
    // committed/reused entries get checked. A true first install sends
    // no lockfile — nothing to verify. `trustLockfile` is the client's
    // opt-out (mirrors the local path's `--trust-lockfile`). Freshly-
    // resolved entries are held to the same policy by the resolver's
    // pick-time gate (the policy is wired into `config`).
    let mut verified_dist_stats = None;
    if !request.trust_lockfile
        && let Some(input_lockfile) = request.lockfile.as_ref()
    {
        let input_lockfile = tarball_router.verification_lockfile(input_lockfile);
        match verify_input_lockfile(runtime, config, &request_auth, &input_lockfile).await {
            Ok(stats) => verified_dist_stats = stats,
            Err(VerifyFailure::Internal(response)) => return response,
            Err(VerifyFailure::Violations(violations)) => {
                return ndjson_single_frame(&violations_frame(&violations));
            }
        }
    }

    // Short-circuit paths that produce the whole lockfile without an
    // incremental tree walk. A verified frozen lockfile still announces
    // its tarballs as `package` frames when the verification fan-out
    // just fetched their metadata — the sizes let the client start the
    // largest downloads first. On a verdict-cache hit no metadata was
    // fetched, so there's nothing to add and the response is the bare
    // `done` frame.
    if let Some(lockfile) = resolve::fresh_frozen_input_lockfile(config, &request) {
        let lockfile = tarball_router.verification_lockfile(&lockfile);
        let lockfile = tarball_router.route_lockfile(config, &lockfile);
        if let Some(osv_index) = runtime.osv_index.as_ref() {
            let violations = osv_violations_for_lockfile(osv_index, &lockfile);
            if !violations.is_empty() {
                return ndjson_single_frame(&violations_frame(&violations));
            }
        }
        let mut frames = verified_dist_stats
            .map(|sizes| frozen_package_frames(config, &tarball_router, &lockfile, &sizes))
            .unwrap_or_default();
        frames.push(done_frame(&lockfile));
        return ndjson_frames(&frames);
    }
    // The base key is auth-excluded and shared by every candidate for the
    // same resolution inputs. Candidate footprints decide which callers
    // may reuse a stored lockfile.
    let resolution_cache_key = resolution_cache_key(config, &request);
    if let Some(key) = resolution_cache_key.as_ref()
        && let Some(lockfile) = cached_resolution(
            &runtime.resolution_cache,
            runtime.resolution_cache_ttl,
            key,
            &runtime.route_context,
            &identity,
        )
    {
        // The OSV index is immutable for this resolver instance and a lockfile
        // is only stored after passing the OSV check, so a cache hit is already
        // OSV-clean — no per-package re-scan needed on this warm path.
        return ndjson_single_frame(&done_frame(&lockfile));
    }

    // Streaming resolve. Run it in a detached task that pushes one
    // `package` frame per resolved tarball into the channel via the
    // observer, then a terminal `done` / `error` frame. The response
    // body drains the channel as frames arrive.
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let observer: Arc<dyn pacquet_package_manager::ResolutionObserver> = Arc::new(StreamObserver {
        tx: tx.clone(),
        package_version_guard: package_version_guard.clone(),
        tarball_router: tarball_router.clone(),
    });
    let client = Arc::clone(&runtime.client);
    let cache = Arc::clone(&runtime.resolution_cache);
    let cache_ttl = runtime.resolution_cache_ttl;
    let final_osv_index = runtime.osv_index.clone();
    let footprint_for_store = Arc::clone(&footprint);
    let cache_secret = Arc::clone(&runtime.resolution_cache_secret);
    tokio::spawn(async move {
        match resolve::resolve(config, &client, &request, &request_auth, Some(observer)).await {
            Ok(lockfile) => {
                let lockfile = tarball_router.route_lockfile(config, &lockfile);
                if let Some(osv_index) = final_osv_index.as_ref() {
                    let violations = osv_violations_for_lockfile(osv_index, &lockfile);
                    if !violations.is_empty() {
                        let _ = tx.send(violations_frame(&violations));
                        return;
                    }
                }
                if let Some(key) = resolution_cache_key {
                    let footprint = footprint_for_store.lock().expect("footprint poisoned").clone();
                    let descriptor = footprint.digest(&cache_secret);
                    let cached = store_resolution(
                        &cache,
                        cache_ttl,
                        key,
                        footprint.clone(),
                        &cache_secret,
                        &lockfile,
                    );
                    if !footprint.is_public() {
                        tracing::debug!(
                            cached,
                            descriptor = descriptor.as_deref().unwrap_or("none"),
                            "private resolution cache candidate evaluated",
                        );
                    }
                }
                let _ = tx.send(done_frame(&lockfile));
            }
            Err(err) => {
                let _ = tx.send(error_frame(&err.to_string()));
            }
        }
    });
    ndjson_stream_response(rx)
}

/// Handle `POST /-/pnpr/v0/verify-lockfile`: verify the client's input
/// lockfile under the client's policy. The client already knows the
/// lockfile is fresh for the current manifests, so this endpoint does not
/// resolve or echo the lockfile back; it returns a terminal NDJSON verdict
/// frame, optionally preceded by sized `package` frames (see
/// [`verify_done_with_frames`]) when the verification fan-out observed
/// dist sizes.
pub(crate) async fn handle_verify_lockfile(
    runtime: &Resolver,
    identity: Identity,
    body: Bytes,
) -> Response {
    let request: ResolveRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(err) => return json_error(StatusCode::BAD_REQUEST, &err.to_string()),
    };

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
        // When the verifier's metadata fan-out observed dist sizes, emit the
        // same sized `package` frames `/-/pnpr/v0/resolve`'s frozen fast path
        // does, ahead of the verdict, so the client can prioritize its largest
        // pending tarball downloads. The client joins each frame to its own
        // lockfile by `integrity` (the announced URL is `route_url`'d and need
        // not match the client's mem-cache key). A verdict-cache hit fetched no
        // metadata, so there are no sizes and the bare verdict is sent.
        Ok(Some(dist_stats)) => verify_done_with_frames(
            runtime.osv_index.as_ref(),
            config,
            &tarball_router,
            &input_lockfile,
            &dist_stats,
        ),
        Ok(None) => verify_done_or_osv_violations(runtime.osv_index.as_ref(), &input_lockfile),
        Err(VerifyFailure::Internal(response)) => response,
        Err(VerifyFailure::Violations(violations)) => {
            ndjson_single_frame(&violations_frame(&violations))
        }
    }
}

const MAX_RESOLUTION_CACHE_ENTRIES: usize = 1024;
const MAX_RESOLUTION_CACHE_CANDIDATES_PER_KEY: usize = 8;

fn cached_resolution(
    cache: &Mutex<HashMap<String, Vec<CachedResolution>>>,
    ttl: Duration,
    key: &str,
    route_context: &RouteContext,
    identity: &Identity,
) -> Option<Lockfile> {
    if ttl.is_zero() {
        return None;
    }
    let mut cache = cache.lock().expect("resolution cache poisoned");
    let candidates = cache.get_mut(key)?;
    candidates.retain(|candidate| candidate.inserted.elapsed() <= ttl);
    let Some((candidate_index, _)) = candidates.iter().enumerate().find(|(_, candidate)| {
        candidate.footprint.is_public() || candidate.footprint.allows(route_context, identity)
    }) else {
        if candidates.is_empty() {
            cache.remove(key);
        }
        return None;
    };
    let candidate = &mut candidates[candidate_index];
    candidate.last_used = Instant::now();
    Some(candidate.lockfile.clone())
}

fn store_resolution(
    cache: &Mutex<HashMap<String, Vec<CachedResolution>>>,
    ttl: Duration,
    key: String,
    footprint: Footprint,
    secret: &[u8],
    lockfile: &Lockfile,
) -> bool {
    if ttl.is_zero() {
        return false;
    }
    let now = Instant::now();
    let descriptor_digest = footprint.digest(secret);
    let candidate = CachedResolution {
        lockfile: lockfile.clone(),
        inserted: now,
        last_used: now,
        footprint,
        descriptor_digest,
    };
    let mut cache = cache.lock().expect("resolution cache poisoned");
    prune_expired_resolution_cache(&mut cache, ttl);
    let candidates = cache.entry(key).or_default();
    if let Some(existing) =
        candidates.iter_mut().find(|entry| entry.descriptor_digest == candidate.descriptor_digest)
    {
        *existing = candidate;
        return true;
    }
    candidates.push(candidate);
    enforce_candidate_limit(candidates);
    while count_resolution_candidates(&cache) > MAX_RESOLUTION_CACHE_ENTRIES {
        if !evict_lru_resolution_candidate(&mut cache, true) {
            break;
        }
    }
    true
}

fn prune_expired_resolution_cache(
    cache: &mut HashMap<String, Vec<CachedResolution>>,
    ttl: Duration,
) {
    cache.retain(|_, candidates| {
        candidates.retain(|candidate| candidate.inserted.elapsed() <= ttl);
        !candidates.is_empty()
    });
}

fn enforce_candidate_limit(candidates: &mut Vec<CachedResolution>) {
    while candidates.len() > MAX_RESOLUTION_CACHE_CANDIDATES_PER_KEY {
        evict_lru_candidate(candidates, true);
    }
}

fn evict_lru_candidate(candidates: &mut Vec<CachedResolution>, private_first: bool) {
    if private_first
        && let Some(index) = candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| !candidate.footprint.is_public())
            .min_by_key(|(_, candidate)| candidate.last_used)
            .map(|(index, _)| index)
    {
        candidates.remove(index);
        return;
    }
    if let Some(index) = candidates
        .iter()
        .enumerate()
        .min_by_key(|(_, candidate)| candidate.last_used)
        .map(|(index, _)| index)
    {
        candidates.remove(index);
    }
}

fn count_resolution_candidates(cache: &HashMap<String, Vec<CachedResolution>>) -> usize {
    cache.values().map(Vec::len).sum()
}

fn evict_lru_resolution_candidate(
    cache: &mut HashMap<String, Vec<CachedResolution>>,
    private_first: bool,
) -> bool {
    let target = lru_resolution_candidate(cache, private_first)
        .or_else(|| if private_first { lru_resolution_candidate(cache, false) } else { None });
    let Some((key, index, _)) = target else {
        return false;
    };
    if let Some(candidates) = cache.get_mut(&key)
        && index < candidates.len()
    {
        candidates.remove(index);
    }
    if cache.get(&key).is_some_and(Vec::is_empty) {
        cache.remove(&key);
    }
    true
}

fn lru_resolution_candidate(
    cache: &HashMap<String, Vec<CachedResolution>>,
    private_only: bool,
) -> Option<(String, usize, Instant)> {
    cache
        .iter()
        .filter_map(|(key, candidates)| {
            candidates
                .iter()
                .enumerate()
                .filter(|(_, candidate)| !private_only || !candidate.footprint.is_public())
                .min_by_key(|(_, candidate)| candidate.last_used)
                .map(|(index, candidate)| (key.clone(), index, candidate.last_used))
        })
        .min_by_key(|(_, _, last_used)| *last_used)
}

fn resolution_cache_key(config: &PacquetConfig, request: &ResolveRequest) -> Option<String> {
    let projects: Vec<serde_json::Value> = request
        .projects_normalized()
        .into_iter()
        .map(|project| {
            serde_json::json!({
                "dir": project.dir,
                "dependencies": project.dependencies,
                "devDependencies": project.dev_dependencies,
                "optionalDependencies": project.optional_dependencies,
            })
        })
        .collect();
    let input = serde_json::json!({
        "registry": &config.registry,
        "namedRegistries": &request.named_registries,
        "overrides": &request.overrides,
        "projects": projects,
        "inputLockfileHash": request.lockfile.as_ref().map(hash_lockfile),
        "frozenLockfile": request.frozen_lockfile,
        "preferFrozenLockfile": request.prefer_frozen_lockfile,
        "ignoreManifestCheck": request.ignore_manifest_check,
        "trustLockfile": request.trust_lockfile,
        "minimumReleaseAge": request.minimum_release_age,
        "minimumReleaseAgeExclude": &request.minimum_release_age_exclude,
        "minimumReleaseAgeIgnoreMissingTime": request.minimum_release_age_ignore_missing_time,
        "trustPolicy": request.trust_policy,
        "trustPolicyExclude": &request.trust_policy_exclude,
        "trustPolicyIgnoreAfter": request.trust_policy_ignore_after,
    });
    let bytes = serde_json::to_vec(&input).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Some(format!("{:x}", hasher.finalize()))
}

#[derive(Clone)]
struct TarballRouter {
    context: Arc<RouteContext>,
    identity: Identity,
    public_url: String,
    /// Per-scope registry map (`scope -> registry URL`, plus the default) used
    /// to classify a registry-resolved package by its *registry* route rather
    /// than its `dist.tarball` host. See [`Self::route_registry_url`].
    registries: HashMap<String, String>,
}

impl TarballRouter {
    fn new(
        context: Arc<RouteContext>,
        identity: Identity,
        public_url: String,
        registries: HashMap<String, String>,
    ) -> Self {
        Self { context, identity, public_url, registries }
    }

    /// Route a registry-resolved package's tarball by the **registry** it came
    /// from, not its `dist.tarball` URL. A split-domain registry serves the
    /// tarball from a different host than the packument, so classifying by the
    /// tarball URL would misread a private package as public and leak its raw
    /// upstream URL. Classifying by the registry origin keeps a private
    /// package on its `/~<name>/` endpoint; a public one still emits its real
    /// (anonymously fetchable) tarball URL for a direct CDN download.
    fn route_registry_url(&self, package: &str, version: &str, tarball_url: &str) -> String {
        let registry = pick_registry_for_package(&self.registries, package, None);
        match self.context.classify(&self.identity, &registry, Some(package)) {
            // The `dist.tarball` is untrusted upstream metadata, so sanitize it
            // before emitting/caching: drop inline `user:pass@host` userinfo and
            // any query/fragment a registry could use to carry a signed-URL
            // token. A genuinely public tarball is anonymously fetchable, so the
            // sanitized URL still works.
            RouteClass::Public => sanitize_registry_tarball_url(tarball_url),
            RouteClass::Hosted { .. } => pnpr_tarball_url(
                &self.public_url,
                package,
                &tarball_filename(package, version, tarball_url),
            ),
            RouteClass::Proxied { alias, .. } => upstream_endpoint_tarball_url(
                &self.public_url,
                &alias,
                package,
                &tarball_filename(package, version, tarball_url),
            ),
        }
    }

    fn route_lockfile(&self, config: &PacquetConfig, lockfile: &Lockfile) -> Lockfile {
        let mut routed = lockfile.clone();
        let Some(packages) = routed.packages.as_mut() else {
            return routed;
        };
        for (package_key, metadata) in packages {
            if !matches!(
                metadata.resolution,
                LockfileResolution::Registry(_) | LockfileResolution::Tarball(_),
            ) {
                continue;
            }
            let Ok((tarball_url, integrity)) =
                tarball_url_and_integrity(&metadata.resolution, package_key, config)
            else {
                continue;
            };
            if !is_http_tarball_url(&tarball_url) || is_git_hosted_tarball_url(&tarball_url) {
                continue;
            }
            let name = package_key.name.to_string();
            let version = package_key.suffix.version().to_string();
            let routed_url = self.route_url(&name, &version, &tarball_url);
            if routed_url == tarball_url.as_ref() {
                continue;
            }
            metadata.resolution = LockfileResolution::Tarball(TarballResolution {
                tarball: routed_url,
                integrity: Some(integrity.clone()),
                git_hosted: None,
                path: None,
            });
        }
        routed
    }

    fn verification_lockfile(&self, lockfile: &Lockfile) -> Lockfile {
        let mut upstream = lockfile.clone();
        let Some(packages) = upstream.packages.as_mut() else {
            return upstream;
        };
        for metadata in packages.values_mut() {
            let LockfileResolution::Tarball(resolution) = &mut metadata.resolution else {
                continue;
            };
            if let Some(tarball_url) = self.upstream_endpoint_tarball_url(&resolution.tarball) {
                resolution.tarball = tarball_url;
            }
        }
        upstream
    }

    fn route_url(&self, package: &str, version: &str, tarball_url: &str) -> String {
        match self.context.classify(&self.identity, tarball_url, Some(package)) {
            // A public route keeps its upstream URL: it was fetched
            // anonymously, so its tarball is anonymously fetchable and pnpr
            // never mints a per-tarball gateway URL. Any inline userinfo a
            // malicious/compromised registry embedded in `dist.tarball` is
            // stripped first, so pnpr never streams or caches it.
            RouteClass::Public => strip_url_credentials(tarball_url),
            RouteClass::Hosted { .. } => pnpr_tarball_url(
                &self.public_url,
                package,
                &tarball_filename(package, version, tarball_url),
            ),
            RouteClass::Proxied { alias, .. } => upstream_endpoint_tarball_url(
                &self.public_url,
                &alias,
                package,
                &tarball_filename(package, version, tarball_url),
            ),
        }
    }

    /// Reverse a `/~<name>/<pkg>/-/<file>` endpoint tarball URL back to its
    /// upstream URL so an input lockfile carrying endpoint URLs can be verified
    /// against the real registry. Returns `None` for any other URL, and for an
    /// endpoint the caller is not authorized for (so verification cannot be
    /// used as an oracle for an upstream the caller cannot reach).
    fn upstream_endpoint_tarball_url(&self, tarball_url: &str) -> Option<String> {
        let prefix = format!("{}/~", self.public_url.trim_end_matches('/'));
        let route = tarball_url.strip_prefix(&prefix)?;
        let (upstream, rest) = route.split_once('/')?;
        let registry = self.context.upstream_registry(&self.identity, upstream)?;
        Some(format!("{}/{rest}", registry.trim_end_matches('/')))
    }
}

fn tarball_filename(package: &str, version: &str, tarball_url: &str) -> String {
    tarball_basename(tarball_url).map_or_else(
        || {
            PackageName::parse(package).map_or_else(
                |_| format!("{package}-{version}.tgz"),
                |name| name.tarball_name_for_version(version),
            )
        },
        str::to_string,
    )
}

fn pnpr_tarball_url(public_url: &str, package: &str, filename: &str) -> String {
    format!("{}/{package}/-/{filename}", public_url.trim_end_matches('/'))
}

/// The `/~<name>/<package>/-/<filename>` registry-endpoint URL a proxied
/// route's tarball is served through. Canonical for a client whose scope is
/// configured at `https://<pnpr>/~<name>/`, so the lockfile entry collapses
/// to integrity-only; the upstream URL and credential stay server-side.
fn upstream_endpoint_tarball_url(
    public_url: &str,
    upstream: &str,
    package: &str,
    filename: &str,
) -> String {
    format!("{}/~{upstream}/{package}/-/{filename}", public_url.trim_end_matches('/'))
}

/// NDJSON content type for the `/-/pnpr/v0/resolve` response. One JSON object
/// per line; the client parses frames as they arrive. Excluded from the
/// server's gzip [`CompressionLayer`](crate::server) so frames flush to
/// the client incrementally rather than being buffered by the encoder.
const NDJSON_CONTENT_TYPE: &str = "application/x-ndjson";

/// [`ResolutionObserver`](pacquet_package_manager::ResolutionObserver)
/// that turns each resolved tarball into a `package` NDJSON frame and
/// pushes it down the response channel. `on_resolved` is best-effort: a
/// closed channel (client hung up) or a serialization failure drops the
/// frame silently — the resolve still runs to completion server-side.
struct StreamObserver {
    tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    package_version_guard: Option<Arc<dyn PackageVersionGuard>>,
    tarball_router: TarballRouter,
}

impl pacquet_package_manager::ResolutionObserver for StreamObserver {
    fn on_resolved(&self, hint: pacquet_package_manager::ResolvedPackageHint<'_>) {
        if let Ok(line) = ndjson_line(&package_frame(&self.tarball_router, &hint)) {
            let _ = self.tx.send(line);
        }
    }

    fn package_version_guard(&self) -> Option<Arc<dyn PackageVersionGuard>> {
        self.package_version_guard.clone()
    }
}

/// One `package` NDJSON frame. `unpackedSize` is omitted (not null)
/// when the registry never published a `dist.unpackedSize`, so older
/// clients parse the frame unchanged.
fn package_frame(router: &TarballRouter, hint: &ResolvedPackageHint<'_>) -> serde_json::Value {
    // A registry-resolved package's `tarball_url` is the packument's
    // `dist.tarball`, which a split-domain registry hosts on a different origin
    // — route it by the registry, not the tarball host, so a private package
    // never leaks its raw upstream URL. Direct tarball deps keep their own URL.
    let tarball_url = if hint.from_registry {
        router.route_registry_url(hint.name, hint.version, hint.tarball_url)
    } else {
        router.route_url(hint.name, hint.version, hint.tarball_url)
    };
    let mut frame = serde_json::json!({
        "type": "package",
        "id": hint.id,
        "name": hint.name,
        "version": hint.version,
        "integrity": hint.integrity,
        "tarball": tarball_url,
    });
    if let Some(size) = hint.unpacked_size {
        frame["unpackedSize"] = serde_json::Value::from(size);
    }
    if let Some(count) = hint.file_count {
        frame["fileCount"] = serde_json::Value::from(count);
    }
    frame
}

/// `package` frames for every tarball-fetchable entry of a verified
/// frozen lockfile, deduplicated by tarball URL. Mirrors what the
/// streaming resolve's [`StreamObserver`] would have announced had the
/// tree walk run: the client prefetches each tarball on arrival, with
/// `unpackedSize` (from the verification fan-out's metadata, when the
/// registry published one) prioritizing the largest downloads.
///
/// Tarball URLs are derived with the same
/// [`tarball_url_and_integrity`] the client's frozen materialization
/// uses, so the announced URLs match the client's mem-cache keys
/// byte-for-byte. Non-tarball resolutions (git, directory, binary,
/// variations) are skipped — the client fetches those through their
/// own protocol paths.
fn frozen_package_frames(
    config: &PacquetConfig,
    router: &TarballRouter,
    lockfile: &Lockfile,
    dist_stats: &ObservedDistStats,
) -> Vec<Vec<u8>> {
    let Some(packages) = lockfile.packages.as_ref() else {
        return Vec::new();
    };
    let mut seen_urls = std::collections::HashSet::new();
    let mut frames = Vec::new();
    for (package_key, snapshot) in packages {
        if !matches!(
            snapshot.resolution,
            LockfileResolution::Registry(_) | LockfileResolution::Tarball(_),
        ) {
            continue;
        }
        let Ok((tarball_url, integrity)) =
            tarball_url_and_integrity(&snapshot.resolution, package_key, config)
        else {
            continue;
        };
        let name = package_key.name.to_string();
        let version = package_key.suffix.version().to_string();
        let tarball_url = router.route_url(&name, &version, &tarball_url);
        if !seen_urls.insert(tarball_url.clone()) {
            continue;
        }
        let id = format!("{name}@{version}");
        let integrity = integrity.to_string();
        let stats = dist_stats.get(&(name.clone(), version.clone())).map(|entry| *entry.value());
        let frame = package_frame(
            router,
            &ResolvedPackageHint {
                id: &id,
                name: &name,
                version: &version,
                integrity: &integrity,
                tarball_url: &tarball_url,
                unpacked_size: stats.and_then(|stats| stats.unpacked_size),
                file_count: stats.and_then(|stats| stats.file_count),
                // The URL is already routed (canonical → endpoint above), so
                // re-routing by registry would be redundant; route_url is a
                // no-op on an already-routed URL.
                from_registry: false,
            },
        );
        if let Ok(line) = ndjson_line(&frame) {
            frames.push(line);
        }
    }
    frames
}

/// Terminal `done` frame: the full resolved lockfile + stats. The client
/// writes the lockfile and fetches every tarball itself.
fn done_frame(lockfile: &Lockfile) -> Vec<u8> {
    let total_packages = lockfile.packages.as_ref().map_or(0, std::collections::HashMap::len);
    let frame = serde_json::json!({
        "type": "done",
        "lockfile": serde_json::to_value(lockfile).unwrap_or(serde_json::Value::Null),
        "stats": { "totalPackages": total_packages },
    });
    ndjson_line(&frame).unwrap_or_else(|_| {
        br#"{"type":"error","message":"failed to serialize lockfile"}"#.to_vec()
    })
}

/// Terminal `error` frame for a resolution that aborted mid-stream,
/// after one or more `package` frames may already have been sent (so the
/// HTTP status is locked at 200 — the failure has to ride in the body).
fn error_frame(message: &str) -> Vec<u8> {
    let frame = serde_json::json!({ "type": "error", "message": message });
    ndjson_line(&frame)
        .unwrap_or_else(|_| br#"{"type":"error","message":"resolution failed"}"#.to_vec())
}

/// Terminal `violations` frame: the input lockfile failed the client's
/// policy. Each entry mirrors the local runner's rendered violation so
/// the client rebuilds the identical `VerifyError` and aborts the same
/// way the local gate would.
fn violations_frame(violations: &[serde_json::Value]) -> Vec<u8> {
    let frame = serde_json::json!({ "type": "violations", "violations": violations });
    ndjson_line(&frame)
        .unwrap_or_else(|_| br#"{"type":"error","message":"verification failed"}"#.to_vec())
}

fn verify_done_frame() -> Vec<u8> {
    ndjson_line(&serde_json::json!({ "type": "done" }))
        .unwrap_or_else(|_| br#"{"type":"error","message":"verification failed"}"#.to_vec())
}

const OSV_VULNERABILITY_CODE: &str = "ERR_PNPM_OSV_VULNERABILITY";

fn verify_done_or_osv_violations(
    osv_index: Option<&Arc<OsvIndex>>,
    lockfile: &Lockfile,
) -> Response {
    let Some(osv_index) = osv_index else {
        return ndjson_single_frame(&verify_done_frame());
    };
    let violations = osv_violations_for_lockfile(osv_index, lockfile);
    if violations.is_empty() {
        ndjson_single_frame(&verify_done_frame())
    } else {
        ndjson_single_frame(&violations_frame(&violations))
    }
}

/// Terminal verdict for `/-/pnpr/v0/verify-lockfile`, preceded by the sized
/// `package` frames `/-/pnpr/v0/resolve`'s frozen fast path emits. The
/// verifier's metadata fan-out already fetched each packument to check the
/// client's policy, so the observed `dist_stats` come for free — surfacing
/// them lets the client prioritize its largest pending tarball downloads.
/// OSV violations suppress the frames: a vulnerable lockfile must not seed
/// any download. The verdict frame is always last.
fn verify_done_with_frames(
    osv_index: Option<&Arc<OsvIndex>>,
    config: &PacquetConfig,
    tarball_router: &TarballRouter,
    lockfile: &Lockfile,
    dist_stats: &ObservedDistStats,
) -> Response {
    if let Some(osv_index) = osv_index {
        let violations = osv_violations_for_lockfile(osv_index, lockfile);
        if !violations.is_empty() {
            return ndjson_single_frame(&violations_frame(&violations));
        }
    }
    let mut frames = frozen_package_frames(config, tarball_router, lockfile, dist_stats);
    frames.push(verify_done_frame());
    ndjson_frames(&frames)
}

fn osv_violations_for_lockfile(index: &OsvIndex, lockfile: &Lockfile) -> Vec<serde_json::Value> {
    let Some(packages) = lockfile.packages.as_ref() else {
        return Vec::new();
    };

    let mut seen = std::collections::HashSet::new();
    let mut violations = Vec::new();
    for (package_key, snapshot) in packages {
        if !is_osv_checkable_resolution(&snapshot.resolution) {
            continue;
        }
        let name = package_key.name.to_string();
        let version = package_key.suffix.version().to_string();
        let mut ids = index.vulnerability_ids(&name, &version);
        // For a tarball resolution the fetched artifact's identity is its
        // URL, not the lockfile key. Under `trustLockfile` a tampered
        // lockfile could key a safe `name@version` while pointing the
        // tarball at a vulnerable artifact, so also screen the version in
        // the tarball filename. This is additive — a mismatch alone is
        // never a violation (custom registries may name tarballs
        // differently), only an actually-vulnerable version is.
        if let LockfileResolution::Tarball(tarball) = &snapshot.resolution
            && let Some(url_version) = tarball_url_version(&tarball.tarball, &name)
            && url_version != version
        {
            ids.extend(index.vulnerability_ids(&name, url_version));
            ids.sort_unstable();
            ids.dedup();
        }
        if ids.is_empty() {
            continue;
        }
        // Dedup only the rare vulnerable hits — several lockfile keys can
        // share one name@version via peer suffixes — so the common
        // (non-vulnerable) entry never pays for the set.
        if !seen.insert((name.clone(), version.clone())) {
            continue;
        }
        violations.push(serde_json::json!({
            "name": name,
            "version": version,
            "code": OSV_VULNERABILITY_CODE,
            "reason": format!(
                "is listed in the local OSV database as vulnerable ({})",
                format_advisory_ids(&ids),
            ),
        }));
    }
    violations
}

/// Best-effort extraction of the version from a registry tarball URL of
/// the conventional `<unscoped-name>-<version>.tgz` shape. Returns `None`
/// for non-standard naming so a legitimate custom registry isn't
/// misjudged. Never parses the URL strictly — the lockfile is untrusted.
fn tarball_url_version<'a>(url: &'a str, name: &str) -> Option<&'a str> {
    let last = url.rsplit('/').next()?;
    let last = last.split(['?', '#']).next().unwrap_or(last);
    let stem = strip_tarball_suffix(last)?;
    let unscoped = name.rsplit('/').next().unwrap_or(name);
    let version = stem.strip_prefix(unscoped)?.strip_prefix('-')?;
    (!version.is_empty()).then_some(version)
}

/// Strip a `.tgz` / `.tar.gz` tarball suffix case-insensitively, so a
/// tampered lockfile can't dodge the URL-version cross-check with a
/// `.TGZ` or `.tar.gz` variant. Returns `None` for any other suffix.
fn strip_tarball_suffix(name: &str) -> Option<&str> {
    [".tar.gz", ".tgz"].into_iter().find_map(|suffix| {
        let head_len = name.len().checked_sub(suffix.len())?;
        let (head, tail) = (name.get(..head_len)?, name.get(head_len..)?);
        tail.eq_ignore_ascii_case(suffix).then_some(head)
    })
}

fn is_osv_checkable_resolution(resolution: &LockfileResolution) -> bool {
    match resolution {
        LockfileResolution::Registry(_) => true,
        // A frozen lockfile is attacker-controlled, so gate on the tarball
        // URL rather than the tamper-prone `git_hosted` flag or strict URL
        // parsing — otherwise `gitHosted: true` or a barely-malformed URL
        // would let a vulnerable package opt out of the OSV scan. Mirrors
        // the npm verifier's URL-based gate.
        LockfileResolution::Tarball(tarball) => {
            is_http_tarball_url(&tarball.tarball) && !is_git_hosted_tarball_url(&tarball.tarball)
        }
        LockfileResolution::Directory(_)
        | LockfileResolution::Git(_)
        | LockfileResolution::Binary(_)
        | LockfileResolution::Variations(_) => false,
    }
}

/// Whether a tarball URL uses an http(s) scheme — the only schemes a
/// registry artifact is served over. Case-insensitive (so a tampered
/// uppercase scheme can't slip past) without allocating a lowercased copy.
fn is_http_tarball_url(url: &str) -> bool {
    let bytes = url.as_bytes();
    bytes.get(..8).is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"https://"))
        || bytes.get(..7).is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"http://"))
}

/// Serialize one frame to a newline-terminated NDJSON line.
fn ndjson_line(value: &serde_json::Value) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// A 200 NDJSON response carrying a single, already-serialized terminal
/// frame (the short-circuit and violation paths, which never stream
/// `package` frames).
fn ndjson_single_frame(frame: &[u8]) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, NDJSON_CONTENT_TYPE)
        .body(Body::from(frame.to_vec()))
        .expect("binary response is always valid")
}

/// A 200 NDJSON response carrying several already-serialized frames in
/// one fixed body. Used by the frozen fast path, where every frame is
/// known up front — no channel to stream from.
fn ndjson_frames(frames: &[Vec<u8>]) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, NDJSON_CONTENT_TYPE)
        .body(Body::from(frames.concat()))
        .expect("binary response is always valid")
}

/// A 200 NDJSON response whose body drains the frame channel as the
/// detached resolve task produces frames. Closing the channel (the task
/// dropped its sender) ends the body.
fn ndjson_stream_response(rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>) -> Response {
    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|line| (Ok::<_, std::io::Error>(axum::body::Bytes::from(line)), rx))
    });
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, NDJSON_CONTENT_TYPE)
        .body(Body::from_stream(stream))
        .expect("streaming response is always valid")
}

/// Why [`verify_input_lockfile`] failed: either the lockfile violated
/// the client's policy (carry the rendered violations so the caller can
/// shape them for the client's protocol) or the verifiers couldn't be
/// built at all (a ready-made error response).
enum VerifyFailure {
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
async fn verify_input_lockfile(
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
    )
    .map_err(|err| {
        VerifyFailure::Internal(json_error(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()))
    })?;

    // Whole-lockfile verdict cache: an O(1) hit when this exact lockfile
    // already passed under a policy we still trust skips the whole fan-out
    // (the dominant win for a shared pnpr — CI re-runs, a fleet building
    // the same repo).
    let hash = hash_lockfile(lockfile);
    if let Some(cache) = runtime.verdict_cache.as_ref()
        && cache.is_verified(&hash, |policy| {
            verifiers.iter().all(|verifier| verifier.can_trust_past_check(policy))
                && runtime.osv_index.as_ref().is_none_or(|index| index.can_trust_policy(policy))
        })
    {
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
    Err(VerifyFailure::Violations(rendered))
}

/// Merge every active verifier's policy snapshot into one bag, the key
/// the verdict cache stores alongside the lockfile hash. Later verifiers
/// overwrite earlier ones on a shared key — mirrors the local cache's
/// [`merge_policies`] so a verdict recorded here is comparable to one the
/// client's own cache would write.
fn merge_policies(
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

/// Reject a request that would have pnpr fetch from an origin that is not on
/// the route allowlist (see [`RouteContext::allows_registry`]) — the
/// resolver's SSRF boundary, run before any server-side fetch. pnpr fetches
/// only from operator-configured registries, so a caller cannot point it at
/// cloud instance metadata, an internal service, or any other off-allowlist
/// host. Beyond the default/named registries this also covers every fetch a
/// *direct-URL dependency* would trigger: an `http(s)`/`git` dependency spec,
/// an override URL leaf, or an input lockfile's tarball URL. A semver range or
/// `npm:`/`workspace:`/`file:` alias never hits the network, so it is ignored.
fn reject_off_allowlist_fetches(
    request: &ResolveRequest,
    context: &RouteContext,
) -> Option<Response> {
    // Registries are fetch targets whatever their scheme.
    let mut registries: Vec<&str> = Vec::new();
    if let Some(registry) = request.registry.as_deref() {
        registries.push(registry);
    }
    registries.extend(request.named_registries.values().map(String::as_str));
    if let Some(off) = registries.into_iter().find(|registry| !context.allows_registry(registry)) {
        return Some(forbidden_off_allowlist(off));
    }

    // Direct-URL dependency specs and input-lockfile tarball URLs reach the
    // network only when they carry an http(s)/git URL.
    let mut url_specs: Vec<&str> = Vec::new();
    let projects = request.projects_normalized();
    for project in &projects {
        for map in
            [&project.dependencies, &project.dev_dependencies, &project.optional_dependencies]
        {
            url_specs.extend(map.values().map(String::as_str));
        }
    }
    if let Some(packages) =
        request.lockfile.as_ref().and_then(|lockfile| lockfile.packages.as_ref())
    {
        for package in packages.values() {
            if let LockfileResolution::Tarball(resolution) = &package.resolution {
                url_specs.push(resolution.tarball.as_str());
            }
        }
    }
    if let Some(off) = url_specs.into_iter().find(|spec| fetch_is_off_allowlist(spec, context)) {
        return Some(forbidden_off_allowlist(off));
    }

    // Override leaves can themselves be direct-URL specs.
    if let Some(off) = request
        .overrides
        .as_ref()
        .and_then(|overrides| first_off_allowlist_override(overrides, context))
    {
        return Some(forbidden_off_allowlist(&off));
    }

    None
}

/// Whether `spec` would trigger a server-side fetch to an origin that is not on
/// the allowlist. Covers any `scheme://host` URL (an `http(s)` tarball and
/// every git transport — `git`/`ssh`/`rsync`/`ftp`/`file`/... — with a `git+`
/// prefix stripped) and scp-style git remotes (`[user@]host:path`), which
/// pacquet routes to the ssh git resolver. Specs that never reach the network —
/// semver ranges, `npm:`/`workspace:`/`file:`/`link:` aliases (no `://`),
/// scoped names — return `false`.
fn fetch_is_off_allowlist(spec: &str, context: &RouteContext) -> bool {
    let url = spec.strip_prefix("git+").unwrap_or(spec);
    if url.contains("://") {
        // Gate by origin regardless of scheme: any transport that reaches a
        // host can be an SSRF vector (every git transport — git/ssh/rsync/ftp/
        // file/...), and a scheme with no allowlistable host (e.g. `file://`,
        // which would read a server-local path) nerf-darts to nothing and is
        // rejected.
        return !context.allows_registry(url);
    }
    // A scp-style git remote carries no scheme, so normalize its host to an
    // `ssh://host/` origin the allowlist can match (nerf-darting is
    // scheme-agnostic, so an operator allowlisting `https://host/` covers it).
    match scp_git_host(url) {
        Some(host) => !context.allows_registry(&format!("ssh://{host}/")),
        None => false,
    }
}

/// The host of a scp-style git remote (`[user@]host:path`), or `None`. The
/// distinguishing shape is a `user@host` authority before the first `:` with a
/// path after it — generalizing the `git@...` form pacquet's git resolver treats
/// as ssh. A protocol spec (`npm:...`, `file:...`) has no `@` in its authority, and
/// a `scheme://...` URL is handled before this is reached.
fn scp_git_host(spec: &str) -> Option<&str> {
    let (authority, path) = spec.split_once(':')?;
    if path.is_empty() || authority.contains('/') {
        return None;
    }
    let (_, host) = authority.rsplit_once('@')?;
    (!host.is_empty()).then_some(host)
}

/// The first override URL leaf whose origin is off the fetch allowlist, if any.
fn first_off_allowlist_override(
    value: &serde_json::Value,
    context: &RouteContext,
) -> Option<String> {
    match value {
        serde_json::Value::String(spec) => {
            fetch_is_off_allowlist(spec, context).then(|| spec.clone())
        }
        serde_json::Value::Array(items) => {
            items.iter().find_map(|item| first_off_allowlist_override(item, context))
        }
        serde_json::Value::Object(map) => {
            map.values().find_map(|item| first_off_allowlist_override(item, context))
        }
        _ => None,
    }
}

fn forbidden_off_allowlist(target: &str) -> Response {
    json_error(
        StatusCode::FORBIDDEN,
        &format!(
            "{target:?} is not allowed by this pnpr server; the operator must declare its \
             registry as a public route or an upstream",
        ),
    )
}

/// Reject a request whose client-supplied URLs carry inline
/// `user:pass@host` credentials, before any fetch or cache write. Covers
/// the default and named registries, every dependency spec, override
/// values, and the tarball URLs of an input lockfile — every surface a
/// tarball/registry URL can reach the resolver (or be echoed back) through.
/// Returns a `400` response when one is found.
fn reject_inline_url_auth(request: &ResolveRequest) -> Option<Response> {
    let mut specs: Vec<&str> = Vec::new();
    if let Some(registry) = request.registry.as_deref() {
        specs.push(registry);
    }
    specs.extend(request.named_registries.values().map(String::as_str));
    let projects = request.projects_normalized();
    for project in &projects {
        for map in
            [&project.dependencies, &project.dev_dependencies, &project.optional_dependencies]
        {
            specs.extend(map.values().map(String::as_str));
        }
    }
    // A supplied lockfile can carry `resolution.tarball` URLs that reach the
    // verify/frozen paths and would otherwise be routed or echoed back.
    if let Some(packages) =
        request.lockfile.as_ref().and_then(|lockfile| lockfile.packages.as_ref())
    {
        for package in packages.values() {
            if let LockfileResolution::Tarball(resolution) = &package.resolution {
                specs.push(resolution.tarball.as_str());
            }
        }
    }
    let inline = specs.iter().any(|spec| crate::route::url_has_inline_credentials(spec))
        || request.overrides.as_ref().is_some_and(overrides_have_inline_url_auth);
    inline.then(|| {
        json_error(
            StatusCode::BAD_REQUEST,
            "inline URL credentials (user:pass@host) are not allowed; \
             configure an upstream credential alias instead",
        )
    })
}

/// Recursively scan an `overrides` JSON value for any string leaf that is
/// a URL carrying inline credentials.
fn overrides_have_inline_url_auth(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(spec) => crate::route::url_has_inline_credentials(spec),
        serde_json::Value::Array(items) => items.iter().any(overrides_have_inline_url_auth),
        serde_json::Value::Object(map) => map.values().any(overrides_have_inline_url_auth),
        _ => false,
    }
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
