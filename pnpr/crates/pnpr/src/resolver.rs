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

mod cache;
mod protocol;
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
    protocol::ResolveRequest,
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

/// Hard cap on how many distinct client configurations the server will
/// intern. Each interned [`PacquetConfig`] is leaked (the install path
/// requires a `&'static Config`), so without a cap an authenticated
/// caller could exhaust memory by varying its registry/policy fields on
/// every request. `1024` is far above the handful of distinct setups a
/// real fleet produces (typically one), matching
/// [`cache::MAX_RESOLUTION_CACHE_ENTRIES`].
const MAX_INTERNED_CONFIGS: usize = 1024;

/// Returned (as a `503`) when [`MAX_INTERNED_CONFIGS`] is reached. The
/// limit resets on restart and a real client reuses one configuration, so
/// a legitimate caller never sees it.
const TOO_MANY_CONFIGS_MESSAGE: &str = "too many distinct registry configurations";

/// Hard cap on the byte size of a single interned config's canonical key,
/// which carries its attacker-controlled `registry` / `namedRegistries` /
/// `overrides` content. [`MAX_INTERNED_CONFIGS`] bounds only the *count* of
/// leaked configs; without this a caller could pad each distinct config with
/// a giant overrides/package-extensions/registry map and still amplify the per-request
/// leak (the whole request body is allowed up to the publish-sized limit).
/// `128 KiB` is far above any real resolver configuration.
const MAX_CONFIG_KEY_BYTES: usize = 128 * 1024;

/// The settings a request resolves under, and the only part of an input
/// lockfile's `settings` block the interning key carries. Keying on the whole
/// block would let a caller mint an unbounded number of distinct configs out
/// of the fields the config never reads (`peersSuffixMaxLength` alone is a
/// `u64`) and exhaust [`MAX_INTERNED_CONFIGS`], after which no caller gets a
/// config at all.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct EffectiveResolverSettings {
    auto_install_peers: bool,
    dedupe_peers: bool,
    exclude_links_from_lockfile: bool,
}

impl EffectiveResolverSettings {
    /// The client's own values whenever it sends them. A client that sends
    /// none (one older than
    /// [pnpm/pnpm#13389](https://github.com/pnpm/pnpm/issues/13389)) falls
    /// back to the input lockfile on a frozen request — nothing is
    /// re-resolved there, the freshness gate compares these three against the
    /// config, and the server's defaults would call a lockfile that is valid
    /// for its owner stale. On an update-capable request it falls back to the
    /// server's defaults instead: the lockfile records what the *last* install
    /// used, which is stale exactly when the client has just changed one of
    /// these.
    fn for_request(request: &ResolveRequest) -> Self {
        static DEFAULTS: LazyLock<PacquetConfig> = LazyLock::new(PacquetConfig::new);

        let lockfile_settings =
            request.frozen_lockfile.then(|| request.lockfile.as_ref()?.settings.as_ref()).flatten();

        EffectiveResolverSettings {
            auto_install_peers: request
                .auto_install_peers
                .or_else(|| lockfile_settings.map(|settings| settings.auto_install_peers))
                .unwrap_or(DEFAULTS.auto_install_peers),
            dedupe_peers: request
                .dedupe_peers
                .or_else(|| lockfile_settings.and_then(|settings| settings.dedupe_peers))
                .unwrap_or(DEFAULTS.dedupe_peers),
            exclude_links_from_lockfile: request
                .exclude_links_from_lockfile
                .or_else(|| lockfile_settings.map(|settings| settings.exclude_links_from_lockfile))
                .unwrap_or(DEFAULTS.exclude_links_from_lockfile),
        }
    }
}

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
///   leak with a giant `overrides`, `packageExtensions`, or registry map.
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

    let resolver_settings = EffectiveResolverSettings::for_request(request);

    let key = serde_json::json!({
        "registry": registry,
        "resolverSettings": resolver_settings,
        "registries": &request.registries,
        "overrides": overrides_key,
        "patchedDependencies": &request.patched_dependencies,
        "packageExtensions": &request.package_extensions,
        "allowUnusedPatches": request.allow_unused_patches,
        "resolutionMode": request.resolution_mode,
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
    // The client's declarations go through the same inversion the config
    // reader runs on the `registries` setting, so the server routes scopes
    // and prefixes exactly as the client would.
    let lookups = pnpm_config::registries::declarations_into_lookups(request.registries.clone());
    if request.registry.is_none()
        && let Some(default_registry) = lookups.default_registry
    {
        config.registry = default_registry;
    }
    config.registries_by_scope = lookups.registries_by_scope;
    config.registries_by_prefix = lookups.registries_by_prefix;
    config.registry_options_by_url = lookups.registry_options_by_url;
    config.overrides = overrides;
    config.patched_dependency_hashes_override.clone_from(&request.patched_dependencies);
    config.package_extensions.clone_from(&request.package_extensions);
    config.allow_unused_patches = request.allow_unused_patches;
    config.modules_dir = PathBuf::from("node_modules");
    config.lockfile = true;
    config.verify_store_integrity = true;
    // The client's resolution and verification policies drive both the
    // input-lockfile verifier and the resolver's pick-time
    // `minimumReleaseAge` / `trustPolicy` checks, so a newly-resolved
    // entry is picked the way the client would have picked it and held
    // to the same policy as the reused ones.
    config.resolution_mode = request.resolution_mode;
    config.minimum_release_age = request.minimum_release_age;
    config.minimum_release_age_exclude.clone_from(&request.minimum_release_age_exclude);
    if let Some(ignore_missing_time) = request.minimum_release_age_ignore_missing_time {
        config.minimum_release_age_ignore_missing_time = ignore_missing_time;
    }
    config.trust_policy = request.trust_policy;
    config.trust_policy_exclude.clone_from(&request.trust_policy_exclude);
    config.trust_policy_ignore_after = request.trust_policy_ignore_after;
    config.auto_install_peers = resolver_settings.auto_install_peers;
    config.dedupe_peers = resolver_settings.dedupe_peers;
    config.exclude_links_from_lockfile = resolver_settings.exclude_links_from_lockfile;
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

    if let Some(response) = reject_invalid_registries(&request) {
        return response;
    }
    if let Some(response) = reject_invalid_patch_hashes(&request) {
        return response;
    }
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
    let observer: Arc<dyn pnpm_package_manager::ResolutionObserver> = Arc::new(StreamObserver {
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
        match Box::pin(resolve::resolve(config, &client, &request, &request_auth, Some(observer)))
            .await
        {
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
        None,
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
