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

pub(crate) use dispatch::serve_resolve_request;
pub(crate) use limits::request_timeout_response as resolve_timeout_response;
pub(crate) use verify_lockfile::serve_verify_lockfile_request;

mod dispatch;
use dispatch::extract_admitted_body;

mod request_auth;
mod streaming;
use streaming::{StreamedResolveInputs, stream_resolve_response};

mod verify_lockfile;
use verify_lockfile::verify_request_lockfile;

mod config_cache;
use config_cache::{
    ActiveConfig, CONFIG_KEY_TOO_LARGE_MESSAGE, ConfigCache, MAX_CACHED_CONFIGS,
    MAX_CONFIG_KEY_BYTES, intern_config,
};

mod cache;
mod cargo;
mod limits;
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
use pnpr_route::{Footprint, RouteContext};

use axum::{body::Bytes, http::StatusCode, response::Response};
use indexmap::IndexMap;
use pnpm_config::Config as PacquetConfig;
use pnpm_lockfile::Lockfile;
use pnpm_lockfile_verification::{collect_resolution_policy_violations, hash_lockfile};
use pnpm_network::{AuthHeaders, GuardedDnsResolver, ThrottledClient, native_dns_resolver};
use pnpm_package_manager::build_resolution_verifiers;
use pnpm_resolving_npm_resolver::{
    InMemoryPackageMetaCache, ObservedDistStats, PackageMetaCache, observed_dist_stats_sink,
};
use pnpm_resolving_resolver_base::{PackageVersionGuard, ResolutionVerifier};
use pnpm_store_dir::StoreDir;

use self::{
    cache::{
        CachedResolution, StoreCandidate, cached_resolution, resolution_cache_key,
        store_resolution_candidate,
    },
    limits::ResolverLimits,
    protocol::ResolveRequest,
    request_validation::{
        parse_request, reject_inline_url_auth, reject_invalid_registries,
        reject_off_allowlist_fetches,
    },
    verdict_cache::VerdictCache,
    wire::{
        KEEPALIVE_INTERVAL, StreamObserver, TarballRouter, error_frame_with_code,
        ndjson_single_frame, ndjson_stream_response, osv_violations_for_lockfile_until,
        progress_frame, try_done_frame_until, verify_done_frame, violations_frame,
    },
};

/// Per-server engine backing the pnpr install endpoint: it holds the
/// store, cache, and HTTP client used to resolve a client's project. The
/// store and cache dirs are fixed for the server's lifetime; the
/// registries come from each client request. Configurations are cached
/// by their effective settings and shared with active requests.
///
/// Held lazily in a [`OnceLock`] on the server's routing snapshot so servers
/// that never receive such a request pay nothing, and so each server in
/// a multi-server test process keeps its own store. A new snapshot builds a
/// new engine around the same [`ResolverShared`].
pub(crate) struct Resolver {
    client: Arc<ThrottledClient>,
    osv_index: Option<Arc<OsvIndex>>,
    /// Route-classification inputs (public/private rules, pnpr-managed
    /// upstream credentials, hosted origin, package policy), resolved once
    /// from the registry graph and combined per request with the caller's
    /// identity to drive auth selection and footprint recording.
    route_context: Arc<RouteContext>,
    /// Public URL clients use for pnpr-hosted and `/~<name>/` endpoint
    /// tarball URLs.
    local_registry: crate::server::LocalRegistry,
    cache: Arc<ResolverCache>,
    index: Arc<IndexCache>,
    limits: Arc<ResolverLimits>,
    /// The [`crate::server::RoutingSnapshot::epoch`] this engine resolves
    /// for. Results are cached under it, so a resolution made under one
    /// graph is never served under another.
    graph_epoch: u64,
}

/// The resolver state that outlives a registry graph change: its caches and
/// its admission limits, so a swapped graph neither drops cached work nor
/// hands out a second set of permits.
pub(crate) struct ResolverShared {
    cache: Arc<ResolverCache>,
    index: Arc<IndexCache>,
    limits: Arc<ResolverLimits>,
}

impl ResolverShared {
    /// Drop every cached resolution. Called when the registry graph changes:
    /// a resolution may have read a name the new graph routes elsewhere.
    pub(crate) fn forget_results(&self) {
        self.cache.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    fn new(config: &RegistryConfig) -> Self {
        let cache_dir = config.storage.cache_dir.join("resolver-cache");
        // Best-effort: a real failure here (e.g. a permission problem)
        // resurfaces with a precise error on the first cache write during
        // resolution, so there's nothing actionable to report yet.
        let _ = std::fs::create_dir_all(&cache_dir);
        Self {
            cache: Arc::new(ResolverCache::new(cache_dir, config)),
            index: Arc::new(IndexCache::new(config.http.packument_ttl)),
            limits: Arc::new(ResolverLimits::from_config(config)),
        }
    }
}
struct ResolverCache {
    dir: PathBuf,
    /// The package store resolutions fetch into, shared by every graph.
    store_dir: StoreDir,
    /// Configurations keyed by canonical JSON, with least recently used eviction.
    configs: ConfigCache,
    /// Held behind an [`Arc`] so the detached streaming-resolve task can
    /// own a clone and record its result after the response body has
    /// already started flowing to the client.
    entries: Arc<Mutex<HashMap<String, Vec<CachedResolution>>>>,
    ttl: Duration,
    /// SQLite-backed whole-lockfile verification verdict cache. `None`
    /// only if the database couldn't be opened — verification then runs
    /// every time (uncached) rather than failing the server.
    verdicts: Option<VerdictCache>,
    /// HMAC secret namespacing a private footprint's cache descriptor.
    secret: Arc<[u8]>,
}

struct IndexCache {
    /// How long a cached Cargo sparse-index file stays fresh: the
    /// server's `packument_ttl`, so index metadata ages out on the same
    /// schedule npm packuments do.
    ttl: Duration,
    /// Serializes the fetch of one Cargo sparse-index file, so concurrent
    /// resolves of the same cold graph fetch each entry once.
    cargo_locks: Arc<crate::server::StripedLocks>,
    /// The same, for a Python index's project pages and metadata files.
    python_locks: Arc<crate::server::StripedLocks>,
}

impl ResolverCache {
    fn new(cache_dir: PathBuf, config: &RegistryConfig) -> Self {
        let verdict_cache = VerdictCache::open(&cache_dir.join("lockfile-verdicts.sqlite")).ok();
        let store_dir = config.storage.cache_dir.join("resolver-store");
        // Best-effort, as for the cache dir in [`ResolverShared::new`].
        let _ = std::fs::create_dir_all(&store_dir);
        Self {
            dir: cache_dir,
            store_dir: StoreDir::new(store_dir),
            configs: ConfigCache::default(),
            entries: Arc::new(Mutex::new(HashMap::new())),
            ttl: config.http.packument_ttl,
            verdicts: verdict_cache,
            secret: Arc::clone(&config.resolution_cache_secret),
        }
    }

    fn config_for(
        &self,
        store_dir: &StoreDir,
        request: &ResolveRequest,
    ) -> Result<ActiveConfig, Response> {
        let permit = Arc::clone(&self.configs.admission)
            .try_acquire_owned()
            .map_err(|_| {
                json_error(StatusCode::SERVICE_UNAVAILABLE, "too many active resolver requests")
            })?;
        let config = intern_config(
            &self.configs.entries,
            store_dir,
            &self.dir,
            request,
            MAX_CACHED_CONFIGS,
            MAX_CONFIG_KEY_BYTES,
        )
        .ok_or_else(|| json_error(StatusCode::BAD_REQUEST, CONFIG_KEY_TOO_LARGE_MESSAGE))?;
        Ok(ActiveConfig { config, _permit: permit })
    }
}

impl IndexCache {
    fn new(cargo_index_ttl: Duration) -> Self {
        Self {
            ttl: cargo_index_ttl,
            cargo_locks: Arc::new(crate::server::StripedLocks::new()),
            python_locks: Arc::new(crate::server::StripedLocks::new()),
        }
    }
}

impl Resolver {
    pub(crate) fn get_or_init<'a>(
        routing: &'a crate::server::RoutingSnapshot,
        shared: &OnceLock<ResolverShared>,
        osv_index: Option<Arc<OsvIndex>>,
        state: &crate::server::AppState,
    ) -> &'a Resolver {
        routing.resolver.get_or_init(|| {
            let config = &state.inner.config;
            let shared = shared.get_or_init(|| ResolverShared::new(config));
            let route_context = Arc::clone(&routing.route_context);
            let mut resolver = Resolver::build_for(config, route_context, shared, osv_index);
            resolver.local_registry.attach(state, routing);
            resolver.graph_epoch = routing.epoch;
            resolver
        })
    }

    #[cfg(test)]
    fn build(config: &RegistryConfig, osv_index: Option<Arc<OsvIndex>>) -> Resolver {
        let route_context = Arc::new(RouteContext::from_config(config));
        Self::build_for(config, route_context, &ResolverShared::new(config), osv_index)
    }

    /// The engine's limits, for a test that tunes them on an engine it built
    /// alone.
    #[cfg(test)]
    fn limits_mut(&mut self) -> &mut ResolverLimits {
        Arc::get_mut(&mut self.limits).expect("a test engine shares no limits")
    }

    /// The engine's cache, for a test that tunes it on an engine it built
    /// alone.
    #[cfg(test)]
    fn cache_mut(&mut self) -> &mut ResolverCache {
        Arc::get_mut(&mut self.cache).expect("a test engine shares no cache")
    }

    fn build_for(
        config: &RegistryConfig,
        route_context: Arc<RouteContext>,
        shared: &ResolverShared,
        osv_index: Option<Arc<OsvIndex>>,
    ) -> Resolver {
        // Re-validate every redirect hop against the same fetch allowlist the
        // request boundary uses, so an allowlisted registry that redirects to
        // an off-allowlist host cannot slip a server-side fetch past it (SSRF),
        // and check every address a hostname resolves to before connecting.
        let redirect_context = Arc::clone(&route_context);
        let connect_context = Arc::clone(&route_context);
        let client = Arc::new(ThrottledClient::new_for_installs_with_guards(
            move |url| redirect_context.allows_fetch(url.as_str()),
            Arc::new(GuardedDnsResolver::new(
                native_dns_resolver(),
                Arc::new(move |host, address| connect_context.allows_address(host, address)),
            )),
        ));
        Resolver {
            client,
            osv_index,
            route_context,
            local_registry: crate::server::LocalRegistry::new(config.http.public_url.clone()),
            cache: Arc::clone(&shared.cache),
            index: Arc::clone(&shared.index),
            limits: Arc::clone(&shared.limits),
            graph_epoch: 0,
        }
    }

    /// `key`, a resolution cache key, scoped to this engine's graph.
    fn graph_cache_key(&self, key: &str) -> String {
        format!("{}:{key}", self.graph_epoch)
    }

    /// Where `registry`'s Cargo sparse-index files are cached. The origin
    /// is hashed into the path so two registries serving the same crate
    /// name never share an entry; the caller's route scope adds the last
    /// namespace segment at fetch time.
    fn cargo_index_cache_dir(&self, registry: &str) -> PathBuf {
        self.cache.dir
            .join("cargo-index")
            .join(pnpm_crypto_hash::create_hex_hash(registry))
    }

    /// Where `index`'s Python documents are cached. As with Cargo, the
    /// origin is hashed into the path so two indexes serving the same
    /// project never share an entry.
    fn python_index_cache_dir(&self, index: &str) -> PathBuf {
        self.cache.dir
            .join("python-index")
            .join(pnpm_crypto_hash::create_hex_hash(index))
    }

    /// Reuse the request's configuration while bounding active work and cached entries.
    fn config_for(&self, request: &ResolveRequest) -> Result<ActiveConfig, Response> {
        self.cache.config_for(&self.cache.store_dir, request)
    }
}

/// Whether `/-/pnpr/v0/resolve` resolves this ecosystem.
///
/// The handshake advertises what this admits and [`serve_resolve_request`]
/// dispatches on it, so the two cannot drift: a list kept beside the dispatch
/// could advertise an ecosystem the dispatch turns away, and both are
/// exhaustive matches, so a new ecosystem stops here for a decision.
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

/// A diagnostic's message and its causes on one line. A resolve failure
/// rides an NDJSON frame, where miette's rendered report would arrive as
/// an unreadable block of escaped newlines.
fn report_message(report: &miette::Report) -> String {
    report
        .chain()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(": ")
}

fn json_error(status: StatusCode, message: &str) -> Response {
    json_error_with_code(status, message, None)
}

fn json_error_with_code(status: StatusCode, message: &str, code: Option<&str>) -> Response {
    pnpr_error::json_error(status, message, code)
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
        runtime.local_registry.public_url.clone(),
        config
            .resolved_registries()
            .into_iter()
            .collect(),
    )
}
