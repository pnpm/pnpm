use crate::{
    auth::{AuthState, TokenRecord, UpsertOutcome, identify},
    config::{Config, HostedConfig},
    error::RegistryError,
    journal::JournaledPublish,
    package_name::PackageName,
    policy::{Identity, PackageRules},
    publish::{
        PendingAttachment, extract_attachments, iso_from_unix_millis, merge_manifest, now_iso,
        stream_decode_verify_and_write,
    },
    registry::{ConcreteKind, Registry, Resolved},
    storage::{HostedPackumentVersion, PackumentWrite, Storage},
    streaming,
    upstream::{
        CacheValidators, FetchOutcome, PackumentFetch, Upstream, abbreviate_packument,
        extract_version_manifest, rewrite_tarball_urls, tarball_basename,
    },
};
use axum::{
    Router,
    body::Body,
    extract::{
        ConnectInfo, DefaultBodyLimit, FromRequestParts, OriginalUri, Path, Request, State,
        connect_info::Connected,
    },
    http::{HeaderMap, Method, StatusCode, header, request::Parts},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, delete, get, post, put},
    serve::IncomingStream,
};
use chrono::Utc;
use indexmap::IndexMap;
use serde_json::{Value, json};
use ssri::Integrity;
use std::{
    collections::HashSet,
    net::{IpAddr, SocketAddr},
    sync::{Arc, LazyLock},
    time::Duration,
};
use tower_http::{
    compression::{
        CompressionLayer,
        predicate::{DefaultPredicate, NotForContentType, Predicate as _},
    },
    trace::TraceLayer,
};
use tracing::Span;

/// MIME the npm registry uses for the abbreviated install-v1 form.
/// Matches what pacquet (and pnpm/npm/yarn) send in `Accept` when
/// resolving for an install — see pacquet's
/// `resolving-npm-resolver::ACCEPT_ABBREVIATED_DOC`. Returning the
/// full document instead bloats the wire by 2–10× on packuments with
/// long version histories.
const ABBREVIATED_CONTENT_TYPE: &str = "application/vnd.npm.install-v1+json";

/// Cap tarballs at 100 MiB while pnpr has to spool them to disk for SRI
/// verification. This bounds per-request temporary disk usage for
/// chunked or malicious upstream bodies.
const MAX_TARBALL_BYTES: u64 = 100 * 1024 * 1024;

/// Cap publish bodies at 100 MiB. The default axum body limit is
/// 2 MiB, far too small for a real package — npm itself caps publish
/// at 100 MiB and verdaccio inherits that limit. We apply it via
/// [`DefaultBodyLimit::max`] on the router rather than on each
/// route, so future write endpoints inherit the same ceiling.
const MAX_PUBLISH_BODY_BYTES: usize = MAX_TARBALL_BYTES as usize;
const PACKUMENT_WRITE_RETRIES: usize = 8;

/// Cap adduser/login bodies far below the publish ceiling. The body is a
/// small couchdb-user JSON document, and login is the one body-accepting
/// endpoint reachable anonymously on every tier — letting it inherit the
/// 100 MiB publish limit would hand unauthenticated callers a cheap
/// buffer-and-parse amplifier.
const MAX_LOGIN_BODY_BYTES: usize = 64 * 1024;

#[derive(Clone)]
struct AppState {
    inner: Arc<AppInner>,
}

struct AppInner {
    storage: Storage,
    /// One [`Upstream`] per declared upstream, keyed by the same name
    /// used in [`Config::upstreams`]. Built once at router construction
    /// time so each request avoids re-allocating a `ThrottledClient`.
    upstreams: IndexMap<String, Upstream>,
    /// The disposable cache namespace of each upstream, keyed like
    /// [`Self::upstreams`]. A pure function of the config (see
    /// [`compute_upstream_cache_namespace`]), precomputed here so the
    /// per-request path doesn't re-sort and re-hash the upstream's headers on
    /// every packument and tarball served through an upstream registry.
    upstream_cache_namespaces: IndexMap<String, String>,
    config: Config,
    auth: AuthState,
    /// Serializes the read-modify-write packument flows per package so
    /// two concurrent writers to the same package on this instance can't
    /// lose each other's changes. See [`PackageLocks`].
    package_locks: PackageLocks,
    /// Lazily-built engine backing the `/-/pnpr/v0/resolve` endpoint. Built on
    /// first such request so servers that never receive one pay nothing.
    resolver: std::sync::OnceLock<crate::resolver::Resolver>,
    /// Local OSV index, loaded before the server accepts requests when
    /// `osv.enabled` is set and a mounted surface consults it.
    osv_index: Option<Arc<crate::resolver::OsvIndex>>,
}

/// Per-package serialization for the read-modify-write packument flows
/// (publish, dist-tag changes, partial-unpublish). Without it, two
/// concurrent publishes of the same package both read the old
/// packument, merge their own version in, and write back — last writer
/// wins and the other version is silently lost.
///
/// A fixed stripe set of mutexes keyed by a hash of the package name
/// serializes writers to the same package while letting different
/// packages proceed in parallel. The fixed count bounds memory (unlike
/// a per-name map that grows with every package ever published); two
/// packages that hash to the same stripe just serialize against each
/// other, which is harmless.
///
/// This guards concurrency **within one instance**. Across replicas
/// sharing one hosted store, the same race needs a conditional write
/// (S3 `If-Match` / `ETag`); that is the cross-replica half tracked in
/// [pnpm/pnpm#12199](https://github.com/pnpm/pnpm/issues/12199).
struct PackageLocks {
    stripes: Box<[tokio::sync::Mutex<()>]>,
}

impl PackageLocks {
    /// Number of stripes. 64 keeps false sharing between distinct
    /// packages rare while staying tiny in memory.
    const STRIPES: usize = 64;

    fn new() -> Self {
        let stripes = (0..Self::STRIPES).map(|_| tokio::sync::Mutex::new(())).collect();
        Self { stripes }
    }

    /// Lock the stripe owning `name`, held until the returned guard is
    /// dropped. Callers hold it across the whole read-modify-write so the
    /// read and the write are atomic with respect to other same-package
    /// writers.
    async fn lock(&self, name: &str) -> tokio::sync::MutexGuard<'_, ()> {
        self.stripes[self.stripe_index(name)].lock().await
    }

    /// Lock the stripes owning every name in `names`, held until the
    /// returned guards are dropped. Stripes are locked in ascending
    /// index order (duplicates collapsed), so two overlapping
    /// batch publishes — or a batch publish racing a single-package
    /// publish — can't deadlock on lock order.
    async fn lock_many(&self, names: &[&str]) -> Vec<tokio::sync::MutexGuard<'_, ()>> {
        let mut indices: Vec<usize> = names.iter().map(|name| self.stripe_index(name)).collect();
        indices.sort_unstable();
        indices.dedup();
        let mut guards = Vec::with_capacity(indices.len());
        for index in indices {
            guards.push(self.stripes[index].lock().await);
        }
        guards
    }

    fn stripe_index(&self, name: &str) -> usize {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(name, &mut hasher);
        std::hash::Hasher::finish(&hasher) as usize % self.stripes.len()
    }
}

/// Build the axum [`Router`] with in-memory auth state. Convenient
/// for tests and for callers that don't want disk-backed users —
/// [`serve`] is the production entry point and goes through
/// [`router_with_auth`] with an [`AuthState::load`]-ed bundle so a
/// corrupted htpasswd file surfaces as a startup error.
///
/// The 2- and 3-segment routes do dispatch inside the handler rather
/// than registering overlapping parametric routes — matchit can't
/// disambiguate `/{scope}/{name}` from `/{name}/{version}` at the
/// router level, so we take both via one handler that branches on
/// the `@` prefix and the literal-`-` segment.
pub fn router(config: Config) -> Router {
    let max_users = config.auth.htpasswd.max_users;
    router_with_auth(config, AuthState::in_memory_with_max_users(max_users))
}

/// Fallible counterpart to [`router`]: surfaces a missing/invalid OSV
/// database (when `osv.enabled`) as an error instead of panicking, for
/// embedders that build the router directly rather than via [`serve`].
pub fn try_router(config: Config) -> crate::error::Result<Router> {
    let max_users = config.auth.htpasswd.max_users;
    try_router_with_auth(config, AuthState::in_memory_with_max_users(max_users))
}

/// Like [`router`] but with a caller-supplied [`AuthState`]. Used
/// by [`serve`] to wire the persistent file-backed stores, and by
/// tests that want to override the bcrypt cost or pre-seed users.
///
/// Panics if `osv.enabled` is set but the database can't load; call
/// [`try_router_with_auth`] to handle that as a recoverable error.
pub fn router_with_auth(config: Config, auth: AuthState) -> Router {
    try_router_with_auth(config, auth)
        .expect("pnpr config must be valid and any enabled OSV database must load before building the router")
}

/// Fallible counterpart to [`router_with_auth`].
pub fn try_router_with_auth(mut config: Config, auth: AuthState) -> crate::error::Result<Router> {
    // Enforce the "at least one surface enabled" invariant for embedders
    // that build and serve the router themselves rather than going through
    // `serve`/`serve_listener`.
    config.ensure_a_feature_is_enabled()?;
    config.ensure_valid_registry_graph()?;
    let osv_index = load_active_osv_index(&config)?;
    Ok(router_with_auth_and_osv(config, auth, osv_index))
}

/// Load the OSV index only for surfaces that actually consult it. With
/// both mounted surfaces disabled rejected earlier, that means any
/// enabled `osv` config now applies to the resolver, the registry, or
/// both.
fn load_active_osv_index(
    config: &Config,
) -> crate::error::Result<Option<Arc<crate::resolver::OsvIndex>>> {
    if config.resolver.enabled || config.registry.enabled {
        crate::resolver::load_osv_index(config)
    } else {
        Ok(None)
    }
}

/// Run startup side effects and load the auth backends. The registry
/// needs publish-journal recovery; auth loads on every tier because the
/// account endpoints (which mint and manage tokens) are always served,
/// and both mounted surfaces consult caller identity.
async fn load_startup_auth(config: &Config) -> crate::error::Result<AuthState> {
    if config.registry.enabled {
        crate::journal::recover_publish_journal(config).await?;
    }
    AuthState::load(&config.auth, &config.backend).await
}

fn router_with_auth_and_osv(
    config: Config,
    auth: AuthState,
    osv_index: Option<Arc<crate::resolver::OsvIndex>>,
) -> Router {
    let storage =
        Storage::new(&config.hosted_store, config.storage.clone(), config.cache_storage.clone());
    let registry_enabled = config.registry.enabled;
    let resolver_enabled = config.resolver.enabled;
    // Only the registry routes consult the upstreams, so a resolver-only
    // server builds none — skipping a `ThrottledClient` allocation per
    // configured upstream.
    let upstreams: IndexMap<String, Upstream> = if registry_enabled {
        config
            .upstreams
            .iter()
            .map(|(name, upstream)| (name.clone(), Upstream::new(name, upstream)))
            .collect()
    } else {
        IndexMap::new()
    };
    let upstream_cache_namespaces = config
        .upstreams
        .keys()
        .map(|name| (name.clone(), compute_upstream_cache_namespace(&config, name)))
        .collect();
    let state = AppState {
        inner: Arc::new(AppInner {
            storage,
            upstreams,
            upstream_cache_namespaces,
            config,
            auth,
            package_locks: PackageLocks::new(),
            resolver: std::sync::OnceLock::new(),
            osv_index,
        }),
    };
    // `/-/ping` is a health check and is always served. The two
    // configurable surfaces — the resolver (install accelerator) and the
    // npm registry — are each mounted only when their feature is enabled,
    // so an operator can run resolver-only, registry-only, or both. The
    // config guarantees at least one is enabled.
    let mut router = Router::new().route("/-/ping", get(serve_ping));
    // The account endpoints — adduser/login, whoami, profile, token
    // listing/revocation, logout — are pnpr account management, not
    // npm-registry functionality: they mint and manage the tokens every
    // authenticated surface demands, so they ride every tier alongside
    // `/-/ping`. A resolver-only tier can then issue its own credentials
    // (`pnpm login --registry https://<resolver-host>/`) instead of
    // depending on a registry-serving replica that shares the auth backend.
    //
    // Each endpoint also answers under any `/~<prefix>/`, so a client whose
    // registry URL is a registry endpoint can log in against it. The identity
    // endpoints are global and consult no registry state; a registry-table lookup
    // would gate nothing while turning the 401-vs-404 split into an
    // existence oracle for private registry names that the content handlers
    // carefully mask.
    router = router
        .route("/-/whoami", get(get_whoami))
        .route("/{prefix}/-/whoami", get(get_whoami_prefixed))
        .route(
            "/-/user/{user}",
            put(put_login).route_layer(DefaultBodyLimit::max(MAX_LOGIN_BODY_BYTES)),
        )
        .route(
            "/{prefix}/-/user/{user}",
            put(put_login_prefixed).route_layer(DefaultBodyLimit::max(MAX_LOGIN_BODY_BYTES)),
        )
        .route("/-/user/token/{token}", delete(delete_session_token))
        .route("/{prefix}/-/user/token/{token}", delete(delete_session_token_prefixed))
        .route("/-/npm/v1/user", get(get_profile))
        .route("/{prefix}/-/npm/v1/user", get(get_profile_prefixed))
        .route("/-/npm/v1/tokens", get(get_token_list))
        .route("/{prefix}/-/npm/v1/tokens", get(get_token_list_prefixed))
        .route("/-/npm/v1/tokens/token/{key}", delete(delete_token_by_key))
        .route("/{prefix}/-/npm/v1/tokens/token/{key}", delete(delete_token_by_key_prefixed));
    // The install-accelerator (resolver) surface, all under the reserved
    // `/-/pnpr` namespace. `/-/pnpr` is the capability handshake (404 on a
    // plain registry); `/-/pnpr/v0/resolve` and `/-/pnpr/v0/verify-lockfile`
    // are the resolver endpoints. These resolve against the registries the
    // *client* sends, so the accelerator works whether or not this process
    // also fronts a registry.
    //
    // When the resolver is disabled, only `/-/pnpr` gets a 404 stub: it is
    // the capability-probe path and overlaps the registry catch-all
    // (`/-/pnpr` matches `/{first}/{second}`), so without the stub a probe
    // would be proxied upstream, giving a confusing 502 where a client
    // expects the "no resolver here" 404. The `/-/pnpr/v0/*` endpoints carry
    // no capability probe, so they are left unmounted rather than stubbed: a
    // client learns the resolver is absent from the handshake 404 and never
    // calls them.
    if resolver_enabled {
        router = router
            .route("/-/pnpr", get(serve_pnpr_handshake))
            .route(
                "/-/pnpr/v0/resolve",
                post(serve_resolve).route_layer(middleware::from_fn_with_state(
                    state.clone(),
                    require_resolver_caller,
                )),
            )
            .route(
                "/-/pnpr/v0/verify-lockfile",
                post(serve_verify_lockfile).route_layer(middleware::from_fn_with_state(
                    state.clone(),
                    require_resolver_caller,
                )),
            );
    } else {
        router = router.route("/-/pnpr", any(resolver_disabled));
    }
    // The npm-registry surface: every packument/tarball read, publish,
    // unpublish, dist-tag, and search. When the surface is off (no registries
    // declared, or `--disable-registry`), none of these routes are mounted
    // — not merely hidden — so a resolver-only tier exposes no registry
    // surface at all.
    if registry_enabled {
        router = router
            // Batch publish: one request carrying many packages' publish
            // documents. Not part of the standard npm registry API —
            // `pnpm publish --batch` opts into it explicitly.
            .route("/-/pnpm/v1/publish", put(serve_batch_publish))
            // Staged (two-phase) publishing — the `pnpm stage` surface.
            // Static `-`/`stage` segments take priority over the generic
            // segment-count routes below, so these never shadow package
            // reads. Each route has a `/~<name>/`-prefixed twin so a client
            // whose registry URL is a registry endpoint can stage through it.
            .route("/-/stage", get(staged::list_staged))
            .route("/-/stage/package/{name}", post(staged::post_staged_publish))
            .route("/-/stage/{id}", get(staged::get_staged).delete(staged::reject_staged))
            .route("/-/stage/{id}/approve", post(staged::approve_staged))
            .route("/-/stage/{id}/tarball", get(staged::get_staged_tarball))
            .route("/{prefix}/-/stage", get(staged::list_staged_prefixed))
            .route("/{prefix}/-/stage/package/{name}", post(staged::post_staged_publish_prefixed))
            .route(
                "/{prefix}/-/stage/{id}",
                get(staged::get_staged_prefixed).delete(staged::reject_staged_prefixed),
            )
            .route("/{prefix}/-/stage/{id}/approve", post(staged::approve_staged_prefixed))
            .route("/{prefix}/-/stage/{id}/tarball", get(staged::get_staged_tarball_prefixed))
            .route("/{name}", get(get_packument_unscoped).put(put_one_segment))
            .route("/{first}/{second}", get(get_two_segments).put(put_two_segments))
            .route(
                "/{first}/{second}/{third}",
                get(get_three_segments).put(put_three_segments).delete(delete_three_segments),
            )
            .route("/{scope}/{name}/-/{filename}", get(get_tarball_scoped))
            .route(
                "/{a}/{b}/{c}/{d}",
                get(get_four_segments).put(put_four_segments).delete(delete_four_segments),
            )
            .route(
                "/{a}/{b}/{c}/{d}/{e}",
                get(get_five_segments).put(put_five_segments).delete(delete_five_segments),
            )
            // Scoped tarball delete: `DELETE /@scope/name/-/<basename-version>.tgz/-rev/<rev>`,
            // plus the registry-addressed dist-tag write and unscoped tarball delete.
            .route(
                "/{a}/{b}/{c}/{d}/{e}/{f}",
                get(get_six_segments).put(put_six_segments).delete(delete_six_segments),
            )
            // Registry-addressed scoped tarball delete:
            // `DELETE /~<name>/@scope/name/-/<file>/-rev/<rev>`
            .route("/{a}/{b}/{c}/{d}/{e}/{f}/{g}", delete(delete_seven_segments));
    }
    router
        .layer(DefaultBodyLimit::max(MAX_PUBLISH_BODY_BYTES))
        // Authenticate once, ahead of every handler: resolve the caller,
        // enforce bearer-token read-only / CIDR restrictions (so a
        // restricted token is rejected before a write handler buffers its
        // up-to-100-MiB body), and stash the identity for handlers to read.
        // Inside the trace layer below, so a rejection is still one record.
        .layer(axum::middleware::from_fn_with_state(state.clone(), authenticate))
        // gzip metadata responses for clients that send `Accept-Encoding:
        // gzip`, matching how a real (CDN-fronted) registry serves
        // packuments — pnpr is commonly hit directly with no proxy in
        // front, so the application is the only layer that can compress.
        // Scoped to JSON: tarballs (`application/octet-stream`, already
        // `.tgz`) are excluded so we never re-gzip an already-compressed
        // payload. The pnpr resolver NDJSON streams
        // (`application/x-ndjson`) is excluded too: gzip-buffering it
        // would defeat the point of streaming — frames must flush to the
        // client as each package resolves, not wait for the encoder.
        .layer(
            CompressionLayer::new().compress_when(
                DefaultPredicate::new()
                    .and(NotForContentType::const_new("application/octet-stream"))
                    .and(NotForContentType::const_new("application/x-ndjson")),
            ),
        )
        // One structured access record per HTTP request: a span
        // carrying method + URI plus a single `finished processing
        // request` event on the response with status and latency.
        // Both the span and the event use the `pnpr::access`
        // target so `LogLevel::Http`'s filter directive can scope to
        // them. `on_request(())` / `on_failure(())` suppress
        // tower-http's default emissions so each request produces
        // exactly one record. The format and level are picked up from
        // the subscriber installed in `main.rs` (driven by the YAML
        // `log:` block — pretty or NDJSON).
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &Request<Body>| {
                    tracing::info_span!(
                        target: "pnpr::access",
                        "request",
                        method = %request.method(),
                        uri = %loggable_uri(request.uri()),
                        // Filled in by `record_cache_status` for packument
                        // reads (e.g. `cache=hit`); stays absent otherwise.
                        cache = tracing::field::Empty,
                    )
                })
                .on_request(())
                .on_response(|response: &Response<Body>, latency: Duration, _span: &Span| {
                    tracing::info!(
                        target: "pnpr::access",
                        status = response.status().as_u16(),
                        latency_ms = latency.as_millis() as u64,
                        "finished processing request",
                    );
                })
                .on_failure(()),
        )
        .with_state(state)
}

/// The request URI as recorded in the access log. npm's logout protocol
/// (`DELETE .../-/user/token/{token}`, path-less or under a `/~<prefix>/`)
/// puts the raw bearer token in the URL path, and a reusable credential
/// must never reach a log line, so everything after that marker is
/// redacted. Every other URI is logged verbatim; a false positive (a
/// registry path that merely embeds the marker) is redacted too, which
/// only costs log detail on a request no route serves.
fn loggable_uri(uri: &axum::http::Uri) -> String {
    const TOKEN_MARKER: &str = "/-/user/token/";
    match uri.path().find(TOKEN_MARKER) {
        Some(index) => {
            format!("{}<redacted>", &uri.path()[..index + TOKEN_MARKER.len()])
        }
        None => uri.to_string(),
    }
}

/// Bind to `config.listen` and serve forever. Loads auth state before
/// binding so a startup-time auth error surfaces before we accept any
/// client connections. Registry startup additionally recovers the publish
/// journal.
pub async fn serve(mut config: Config) -> crate::error::Result<()> {
    // Enforce the "at least one surface" invariant here too, not only at
    // YAML load / CLI: embedders build `Config` programmatically and call
    // straight into `serve`, so a both-disabled config must fail loudly
    // rather than start a server that only answers `/-/ping`.
    config.ensure_a_feature_is_enabled()?;
    config.ensure_valid_registry_graph()?;
    log_enabled_surfaces(&config);
    let osv_index = load_active_osv_index(&config)?;
    let auth = load_startup_auth(&config).await?;
    let listen = config.listen;
    let app = router_with_auth_and_osv(config, auth, osv_index);
    let listener = NodelayTcpListener(tokio::net::TcpListener::bind(listen).await?);
    tracing::info!(%listen, "pnpr listening");
    axum::serve(listener, app.into_make_service_with_connect_info::<PeerAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Log which surfaces are mounted at startup. A misconfiguration — a
/// `registries:` block that didn't parse the way the operator meant, or a
/// typo'd `resolver:` block name, which the intentionally
/// verdaccio-lenient config parser silently ignores and so leaves the
/// surface at its default-enabled state — is then immediately visible to
/// the operator rather than only discoverable by probing.
fn log_enabled_surfaces(config: &Config) {
    tracing::info!(
        registry = config.registry.enabled,
        resolver = config.resolver.enabled,
        "pnpr surfaces",
    );
}

/// Serve on an already-bound listener.
///
/// Test harnesses can bind to `127.0.0.1:0`, read the OS-assigned
/// address, and then hand that listener here without a bind/drop/rebind
/// race.
pub async fn serve_listener(
    mut config: Config,
    listener: tokio::net::TcpListener,
) -> crate::error::Result<()> {
    let listen = listener.local_addr()?;
    config.ensure_a_feature_is_enabled()?;
    config.ensure_valid_registry_graph()?;
    log_enabled_surfaces(&config);
    let osv_index = load_active_osv_index(&config)?;
    // Load the configured auth backends here too — going through `router`
    // would silently fall back to in-memory auth and ignore a persisted
    // htpasswd / SQLite store or a configured `backend:`.
    let auth = load_startup_auth(&config).await?;
    let app = router_with_auth_and_osv(config, auth, osv_index);
    tracing::info!(%listen, "pnpr listening");
    axum::serve(
        NodelayTcpListener(listener),
        app.into_make_service_with_connect_info::<PeerAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

/// Wraps [`tokio::net::TcpListener`] to disable Nagle's algorithm on
/// every accepted socket.
///
/// Node's http server sets `TCP_NODELAY` by default; hyper 1.x
/// doesn't. With Nagle on, the kernel coalesces small writes and
/// (on Linux epoll) introduces ~tens-of-µs of per-response delay
/// while waiting for follow-up bytes that never come — invisible
/// on macOS's kqueue scheduling, but stacks up across the
/// thousand-request fan-out of an install benchmark.
///
/// Set on a per-socket basis after accept because the option lives
/// on the *connection*, not the listening socket.
struct NodelayTcpListener(tokio::net::TcpListener);

impl axum::serve::Listener for NodelayTcpListener {
    type Io = tokio::net::TcpStream;
    type Addr = std::net::SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            match self.0.accept().await {
                Ok((socket, addr)) => {
                    // Ignore set_nodelay errors — failure means the
                    // peer already closed; serving the connection
                    // will surface that as a normal HTTP error.
                    let _ = socket.set_nodelay(true);
                    return (socket, addr);
                }
                Err(err) => {
                    tracing::warn!(?err, "tcp accept error; retrying");
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            }
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.0.local_addr()
    }
}

/// Client socket address captured from the accepted TCP connection, for
/// the CIDR-restriction gate. A local newtype (rather than [`SocketAddr`]
/// directly) so we can implement axum's [`Connected`] for
/// [`NodelayTcpListener`] — the blanket impl axum ships covers only the
/// bare [`tokio::net::TcpListener`], not our wrapper. This is the real
/// peer address from the socket, never a client-supplied forwarding
/// header, so it can't be spoofed.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PeerAddr(pub(crate) SocketAddr);

impl Connected<IncomingStream<'_, NodelayTcpListener>> for PeerAddr {
    fn connect_info(stream: IncomingStream<'_, NodelayTcpListener>) -> Self {
        PeerAddr(*stream.remote_addr())
    }
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown signal received");
}

// --------------------------------------------------------------------
// GET handlers — packument, version manifest, tarball.
// Same overall shape as before, with an access-policy check added
// up front so protected packages return 401 to anonymous callers.
// --------------------------------------------------------------------

async fn get_packument_unscoped(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    serve_packument(&state, &identity, &headers, &name).await
}

async fn get_two_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    headers: HeaderMap,
    Path((first, second)): Path<(String, String)>,
) -> Response {
    // `/~<name>/<pkg>` — unscoped packument through a registry endpoint. The
    // tarball base is the client's `/~<name>/` URL so the rewritten URLs stay
    // canonical for the registry the client actually addressed.
    if let Some(registry) = first.strip_prefix('~').filter(|registry| !registry.is_empty()) {
        let base = upstream_tarball_base(&state.inner.config.public_url, registry);
        return private_no_cache(
            serve_registry_packument(&state, &identity, &headers, registry, &second, &base).await,
        );
    }
    if first.starts_with('@') {
        if first.contains('/') {
            serve_version_manifest(&state, &identity, &first, &second).await
        } else {
            let full = format!("{first}/{second}");
            serve_packument(&state, &identity, &headers, &full).await
        }
    } else {
        serve_version_manifest(&state, &identity, &first, &second).await
    }
}

async fn get_three_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    Path((first, second, third)): Path<(String, String, String)>,
) -> Response {
    if first == "-" && second == "v1" && third == "search" {
        let query = uri.query().unwrap_or("");
        // Search results are filtered per caller (registry access + per-package
        // ACL), so they must never land in a shared HTTP cache.
        return private_no_cache(serve_search(&state, &identity, None, query).await);
    }
    if let Some(registry) = first.strip_prefix('~').filter(|registry| !registry.is_empty()) {
        // The account endpoints (whoami, adduser, logout, profile, tokens)
        // live on dedicated always-mounted routes; a `/~<name>/-/...` path
        // that still reaches this handler names no registry content.
        if second == "-" {
            return not_found();
        }
        let base = upstream_tarball_base(&state.inner.config.public_url, registry);
        if second.starts_with('@') {
            // `/~<name>/@scope%2Fname/<version>` — version manifest for an
            // encoded scoped package through a registry endpoint.
            if second.contains('/') {
                return private_no_cache(
                    serve_registry_version_manifest(
                        &state, &identity, registry, &second, &third, &base,
                    )
                    .await,
                );
            }
            // `/~<name>/@scope/<pkg>` — scoped packument through a registry.
            let full = format!("{second}/{third}");
            return private_no_cache(
                serve_registry_packument(&state, &identity, &headers, registry, &full, &base).await,
            );
        }
        // `/~<name>/<pkg>/<version-or-tag>` — unscoped version manifest
        // through a registry endpoint. (The unscoped tarball shape
        // `/~<name>/<pkg>/-/<file>` is a distinct literal-`-` route.)
        return private_no_cache(
            serve_registry_version_manifest(&state, &identity, registry, &second, &third, &base)
                .await,
        );
    }
    if second == "-" {
        serve_tarball(&state, &identity, &first, &third).await
    } else if first.starts_with('@') {
        let full = format!("{first}/{second}");
        serve_version_manifest(&state, &identity, &full, &third).await
    } else {
        not_found()
    }
}

async fn get_tarball_scoped(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((scope, name, filename)): Path<(String, String, String)>,
) -> Response {
    // `/~<name>/<pkg>/-/<file>` — unscoped tarball through a registry endpoint.
    if let Some(registry) = scope.strip_prefix('~').filter(|registry| !registry.is_empty()) {
        return private_no_cache(
            serve_registry_tarball(&state, &identity, registry, &name, &filename).await,
        );
    }
    if !scope.starts_with('@') {
        return not_found();
    }
    let full = format!("{scope}/{name}");
    serve_tarball(&state, &identity, &full, &filename).await
}

/// 4-segment GET:
/// * `/-/package/{pkg}/dist-tags` — packument's `dist-tags` object.
/// * `/-/org/{scope}/team` — the teams of the registry claiming `@scope`.
/// * `/~<name>/-/v1/search` — search through a registry endpoint.
/// * `/~<name>/@scope/{pkg}/{version}` — scoped version manifest through a
///   registry endpoint.
async fn get_four_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    OriginalUri(uri): OriginalUri,
    Path((a, b, c, d)): Path<(String, String, String, String)>,
) -> Response {
    if a == "-" && b == "package" && d == "dist-tags" {
        let response = get_dist_tags(&state, &identity, None, &c).await;
        return private_if_caller_gated(&state, &c, response);
    }
    if a == "-" && b == "org" && d == "team" {
        return private_no_cache(get_org_teams(&state, &identity, None, &c));
    }
    if let Some(registry) = a.strip_prefix('~').filter(|registry| !registry.is_empty()) {
        if b == "-" && c == "v1" && d == "search" {
            let query = uri.query().unwrap_or("");
            return private_no_cache(serve_search(&state, &identity, Some(registry), query).await);
        }
        if b.starts_with('@') && !b.contains('/') {
            let full = format!("{b}/{c}");
            let base = upstream_tarball_base(&state.inner.config.public_url, registry);
            return private_no_cache(
                serve_registry_version_manifest(&state, &identity, registry, &full, &d, &base)
                    .await,
            );
        }
    }
    not_found()
}

/// 5-segment GET:
/// * `/-/team/{scope}/{team}/user` — a team's members.
/// * `/~<name>/@scope/<pkg>/-/<file>` — scoped tarball through a registry
///   endpoint.
/// * `/~<name>/-/package/<pkg>/dist-tags` — dist-tags through a registry
///   endpoint.
/// * `/~<name>/-/org/{scope}/team` — org teams through a registry endpoint.
///
/// Every other 5-segment GET is a not-found catchall (the route exists so
/// DELETE/PUT can sit on the same path).
async fn get_five_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((a, b, c, d, e)): Path<(String, String, String, String, String)>,
) -> Response {
    if a == "-" && b == "team" && e == "user" {
        return private_no_cache(get_team_members(&state, &identity, None, &c, &d));
    }
    if let Some(registry) = a.strip_prefix('~').filter(|registry| !registry.is_empty()) {
        if b.starts_with('@') && d == "-" {
            let full = format!("{b}/{c}");
            return private_no_cache(
                serve_registry_tarball(&state, &identity, registry, &full, &e).await,
            );
        }
        if b == "-" && c == "package" && e == "dist-tags" {
            return private_no_cache(get_dist_tags(&state, &identity, Some(registry), &d).await);
        }
        if b == "-" && c == "org" && e == "team" {
            return private_no_cache(get_org_teams(&state, &identity, Some(registry), &d));
        }
    }
    not_found()
}

/// 6-segment GET:
/// * `/~<name>/-/team/{scope}/{team}/user` — a team's members through a
///   registry endpoint.
async fn get_six_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((a, b, c, d, e, f)): Path<(String, String, String, String, String, String)>,
) -> Response {
    if let Some(registry) = a.strip_prefix('~').filter(|registry| !registry.is_empty())
        && b == "-"
        && c == "team"
        && f == "user"
    {
        return private_no_cache(get_team_members(&state, &identity, Some(registry), &d, &e));
    }
    not_found()
}

// --------------------------------------------------------------------
// PUT handlers — adduser, publish, dist-tag write.
// --------------------------------------------------------------------

/// `PUT /{name}` — publish an unscoped package.
async fn put_one_segment(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(name): Path<String>,
    body: axum::body::Bytes,
) -> Response {
    publish_package(&state, &identity, None, &name, body).await
}

/// `PUT /{first}/{second}` — publish a scoped package
/// (`/@scope/name`), or an unscoped package through a registry endpoint
/// (`/~<name>/<pkg>`). The `/-/package/{pkg}` shape never lands here
/// because that's at least 4 segments.
async fn put_two_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((first, second)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Response {
    // `PUT /~<name>/<pkg>` — publish an unscoped package through a registry.
    if let Some(registry) = first.strip_prefix('~').filter(|registry| !registry.is_empty()) {
        return publish_package(&state, &identity, Some(registry), &second, body).await;
    }
    if first.starts_with('@') {
        let full = format!("{first}/{second}");
        return publish_package(&state, &identity, None, &full, body).await;
    }
    not_found()
}

/// `PUT /{pkg}/-rev/{rev}` — packument update (partial unpublish).
async fn put_three_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((first, second, third)): Path<(String, String, String)>,
    body: axum::body::Bytes,
) -> Response {
    // `PUT /~<name>/@scope/<pkg>` — publish a scoped package through a registry.
    if let Some(registry) = first.strip_prefix('~').filter(|registry| !registry.is_empty())
        && second.starts_with('@')
    {
        let full = format!("{second}/{third}");
        return publish_package(&state, &identity, Some(registry), &full, body).await;
    }
    if second == "-rev" {
        // `third` is the opaque revision token the client sent back.
        // We don't track revisions, so it's only used for routing —
        // the body is the full mutated packument.
        let _ = third;
        return update_packument(&state, &identity, None, &first, &body).await;
    }
    not_found()
}

/// 4-segment PUT:
/// * `/~<name>/{pkg}/-rev/{rev}` — packument update (partial unpublish)
///   through a registry endpoint. Scoped packages arrive percent-encoded as a
///   single `@scope%2Fname` segment, like the path-less 3-segment form.
async fn put_four_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((a, b, c, d)): Path<(String, String, String, String)>,
    body: axum::body::Bytes,
) -> Response {
    // `PUT /-/org/{scope}/team` — team create; config-managed, rejected.
    if a == "-" && b == "org" && d == "team" {
        return reject_team_mutation(&state, &identity, None, &c, "create a team");
    }
    if let Some(registry) = a.strip_prefix('~').filter(|registry| !registry.is_empty())
        && c == "-rev"
    {
        let _ = d; // revision token is unused
        return update_packument(&state, &identity, Some(registry), &b, &body).await;
    }
    not_found()
}

/// `DELETE /{pkg}/-rev/{rev}` — remove the entire package
/// (`pnpm unpublish --force`). For scoped packages the URL is
/// `/@scope%2Fname/-rev/{rev}` and arrives as a single segment after
/// axum's percent-decoding.
async fn delete_three_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((first, second, third)): Path<(String, String, String)>,
) -> Response {
    if second == "-rev" {
        let _ = third;
        return delete_package(&state, &identity, None, &first).await;
    }
    not_found()
}

/// 5-segment PUT:
/// * `/-/package/{pkg}/dist-tags/{tag}` — add/update a dist-tag.
/// * `/-/team/{scope}/{team}/user` — team member add; config-managed,
///   rejected.
/// * `/~<name>/-/org/{scope}/team` — team create through a registry
///   endpoint; config-managed, rejected.
async fn put_five_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((a, b, c, d, e)): Path<(String, String, String, String, String)>,
    body: axum::body::Bytes,
) -> Response {
    if a == "-" && b == "package" && d == "dist-tags" {
        return set_dist_tag(&state, &identity, None, &c, &e, &body).await;
    }
    if a == "-" && b == "team" && e == "user" {
        return reject_team_mutation(&state, &identity, None, &c, "add a team member");
    }
    if let Some(registry) = a.strip_prefix('~').filter(|registry| !registry.is_empty())
        && b == "-"
        && c == "org"
        && e == "team"
    {
        return reject_team_mutation(&state, &identity, Some(registry), &d, "create a team");
    }
    not_found()
}

/// 6-segment PUT:
/// * `/~<name>/-/package/{pkg}/dist-tags/{tag}` — add/update a dist-tag
///   through a registry endpoint.
/// * `/~<name>/-/team/{scope}/{team}/user` — team member add through a
///   registry endpoint; config-managed, rejected.
async fn put_six_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((a, b, c, d, e, f)): Path<(String, String, String, String, String, String)>,
    body: axum::body::Bytes,
) -> Response {
    if let Some(registry) = a.strip_prefix('~').filter(|registry| !registry.is_empty())
        && b == "-"
    {
        if c == "package" && e == "dist-tags" {
            return set_dist_tag(&state, &identity, Some(registry), &d, &f, &body).await;
        }
        if c == "team" && f == "user" {
            return reject_team_mutation(
                &state,
                &identity,
                Some(registry),
                &d,
                "add a team member",
            );
        }
    }
    not_found()
}

/// 4-segment DELETE:
/// * `/-/team/{scope}/{team}` — team destroy; config-managed, rejected.
/// * `/~<name>/{pkg}/-rev/{rev}` — remove the entire package through a
///   registry endpoint (scoped packages arrive percent-encoded as one
///   `@scope%2Fname` segment).
async fn delete_four_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((a, b, c, d)): Path<(String, String, String, String)>,
) -> Response {
    if a == "-" && b == "team" {
        let _ = d; // team name — the mutation is rejected regardless
        return reject_team_mutation(&state, &identity, None, &c, "destroy a team");
    }
    if let Some(registry) = a.strip_prefix('~').filter(|registry| !registry.is_empty())
        && c == "-rev"
    {
        let _ = d; // revision token is unused
        return delete_package(&state, &identity, Some(registry), &b).await;
    }
    not_found()
}

/// 5-segment DELETE:
/// * `/-/package/{pkg}/dist-tags/{tag}` — remove a dist-tag.
/// * `/-/team/{scope}/{team}/user` — team member remove; config-managed,
///   rejected.
/// * `/~<name>/-/team/{scope}/{team}` — team destroy through a registry
///   endpoint; config-managed, rejected.
/// * `/{pkg}/-/{filename}/-rev/{rev}` — remove an unscoped tarball
///   (one step of `pnpm unpublish <pkg>@<version>`).
async fn delete_five_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((a, b, c, d, e)): Path<(String, String, String, String, String)>,
) -> Response {
    if a == "-" && b == "package" && d == "dist-tags" {
        return remove_dist_tag(&state, &identity, None, &c, &e).await;
    }
    if a == "-" && b == "team" && e == "user" {
        return reject_team_mutation(&state, &identity, None, &c, "remove a team member");
    }
    if let Some(registry) = a.strip_prefix('~').filter(|registry| !registry.is_empty())
        && b == "-"
        && c == "team"
    {
        let _ = e; // team name — the mutation is rejected regardless
        return reject_team_mutation(&state, &identity, Some(registry), &d, "destroy a team");
    }
    if b == "-" && d == "-rev" {
        let _ = e; // revision token is unused
        return delete_tarball(&state, &identity, None, &a, &c).await;
    }
    not_found()
}

/// 6-segment DELETE:
/// * `/{scope}/{name}/-/{filename}/-rev/{rev}` — remove a scoped
///   tarball. The pnpm unpublish flow gets here when the tarball URL
///   it reconstructs from the packument is the literal-slash scoped
///   form (`http://host/@scope/name/-/name-1.0.0.tgz`), so the
///   request lands here unencoded rather than as a 5-seg
///   `@scope%2Fname` URL.
/// * `/~<name>/-/package/{pkg}/dist-tags/{tag}` — remove a dist-tag
///   through a registry endpoint.
/// * `/~<name>/-/team/{scope}/{team}/user` — team member remove through a
///   registry endpoint; config-managed, rejected.
/// * `/~<name>/{pkg}/-/{filename}/-rev/{rev}` — remove an unscoped
///   tarball through a registry endpoint.
async fn delete_six_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((a, b, c, d, e, f)): Path<(String, String, String, String, String, String)>,
) -> Response {
    if a.starts_with('@') && c == "-" && e == "-rev" {
        let _ = f; // revision token is unused
        let full = format!("{a}/{b}");
        return delete_tarball(&state, &identity, None, &full, &d).await;
    }
    if let Some(registry) = a.strip_prefix('~').filter(|registry| !registry.is_empty()) {
        if b == "-" && c == "package" && e == "dist-tags" {
            return remove_dist_tag(&state, &identity, Some(registry), &d, &f).await;
        }
        if b == "-" && c == "team" && f == "user" {
            return reject_team_mutation(
                &state,
                &identity,
                Some(registry),
                &d,
                "remove a team member",
            );
        }
        if c == "-" && e == "-rev" {
            let _ = f; // revision token is unused
            return delete_tarball(&state, &identity, Some(registry), &b, &d).await;
        }
    }
    not_found()
}

/// 7-segment DELETE:
/// * `/~<name>/{scope}/{name}/-/{filename}/-rev/{rev}` — remove a scoped
///   tarball through a registry endpoint (the unencoded literal-slash form the
///   pnpm unpublish flow reconstructs from the packument's tarball URL).
async fn delete_seven_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((a, b, c, d, e, f, g)): Path<(String, String, String, String, String, String, String)>,
) -> Response {
    if let Some(registry) = a.strip_prefix('~').filter(|registry| !registry.is_empty())
        && b.starts_with('@')
        && d == "-"
        && f == "-rev"
    {
        let _ = g; // revision token is unused
        let full = format!("{b}/{c}");
        return delete_tarball(&state, &identity, Some(registry), &full, &e).await;
    }
    not_found()
}

// --------------------------------------------------------------------
// Account routes — adduser/login, whoami, profile, token list and
// revocation, logout. Mounted on every tier (see the router construction
// in `router_with_auth_and_osv`). Each has a `/~<prefix>/`-addressed twin
// whose `/{prefix}/...` route pattern also matches a non-`~` first
// segment; that shape is not an account URL, so the handler 404s it —
// though route-level layers still run first (an oversized body to a
// non-`~` login path is the body cap's 413, not a 404).
// --------------------------------------------------------------------

/// Whether `prefix` is a `/~<prefix>/`-style first segment — the only
/// shape the prefixed account routes serve.
fn is_tilde_prefix(prefix: &str) -> bool {
    prefix.strip_prefix('~').is_some_and(|rest| !rest.is_empty())
}

async fn get_whoami(AuthedCaller(identity): AuthedCaller) -> Response {
    private_no_cache(serve_whoami(&identity))
}

async fn get_whoami_prefixed(
    AuthedCaller(identity): AuthedCaller,
    Path(prefix): Path<String>,
) -> Response {
    if !is_tilde_prefix(&prefix) {
        return not_found();
    }
    private_no_cache(serve_whoami(&identity))
}

/// `PUT /-/user/org.couchdb.user:{name}` — adduser / login. Authenticates
/// from the request body, not the caller's existing identity.
async fn put_login(
    State(state): State<AppState>,
    Path(user): Path<String>,
    body: axum::body::Bytes,
) -> Response {
    match user.strip_prefix("org.couchdb.user:") {
        Some(name) => add_user(&state, name, &body).await,
        None => not_found(),
    }
}

async fn put_login_prefixed(
    State(state): State<AppState>,
    Path((prefix, user)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Response {
    if !is_tilde_prefix(&prefix) {
        return not_found();
    }
    put_login(State(state), Path(user), body).await
}

async fn delete_session_token(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(token): Path<String>,
) -> Response {
    private_no_cache(logout(&state, &identity, &token).await)
}

async fn delete_session_token_prefixed(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((prefix, token)): Path<(String, String)>,
) -> Response {
    if !is_tilde_prefix(&prefix) {
        return not_found();
    }
    private_no_cache(logout(&state, &identity, &token).await)
}

async fn get_profile(AuthedCaller(identity): AuthedCaller) -> Response {
    private_no_cache(serve_profile(&identity))
}

async fn get_profile_prefixed(
    AuthedCaller(identity): AuthedCaller,
    Path(prefix): Path<String>,
) -> Response {
    if !is_tilde_prefix(&prefix) {
        return not_found();
    }
    private_no_cache(serve_profile(&identity))
}

async fn get_token_list(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
) -> Response {
    private_no_cache(list_tokens(&state, &identity).await)
}

async fn get_token_list_prefixed(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(prefix): Path<String>,
) -> Response {
    if !is_tilde_prefix(&prefix) {
        return not_found();
    }
    private_no_cache(list_tokens(&state, &identity).await)
}

async fn delete_token_by_key(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(key): Path<String>,
) -> Response {
    private_no_cache(revoke_token_by_key(&state, &identity, &key).await)
}

async fn delete_token_by_key_prefixed(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((prefix, key)): Path<(String, String)>,
) -> Response {
    if !is_tilde_prefix(&prefix) {
        return not_found();
    }
    private_no_cache(revoke_token_by_key(&state, &identity, &key).await)
}

// --------------------------------------------------------------------
// Handler bodies.
// --------------------------------------------------------------------

async fn serve_packument(
    state: &AppState,
    identity: &Identity,
    headers: &HeaderMap,
    raw_name: &str,
) -> Response {
    // The path-less base is an alias for the default-target registry: every
    // request routes through the registry graph (authoritatively, no
    // fall-through). With no default target the bare host has no registry.
    match default_registry_target(state) {
        Some(target) => {
            // The path-less base: tarball URLs stay canonical for the bare host.
            let base = state.inner.config.public_url.clone();
            let response =
                serve_registry_packument(state, identity, headers, &target, raw_name, &base).await;
            private_if_caller_gated(state, raw_name, response)
        }
        None => not_found(),
    }
}

async fn serve_version_manifest(
    state: &AppState,
    identity: &Identity,
    raw_name: &str,
    version_or_tag: &str,
) -> Response {
    match default_registry_target(state) {
        Some(target) => {
            let base = state.inner.config.public_url.clone();
            let response = serve_registry_version_manifest(
                state,
                identity,
                &target,
                raw_name,
                version_or_tag,
                &base,
            )
            .await;
            private_if_caller_gated(state, raw_name, response)
        }
        None => not_found(),
    }
}

/// Serve a single version's manifest (`GET <base>/<pkg>/<version-or-tag>`)
/// through the registry graph. Resolves the package to its one concrete origin,
/// loads that origin's packument, and extracts the requested version with its
/// `dist.tarball` rewritten onto the same origin's base.
async fn serve_registry_version_manifest(
    state: &AppState,
    identity: &Identity,
    registry: &str,
    raw_name: &str,
    version_or_tag: &str,
    tarball_base: &str,
) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };
    let resolved_source = resolve_registry_source(state, registry, name.as_str());
    let bytes = match &resolved_source {
        RegistrySource::Upstream(source) => {
            // The upstream registry's per-package rules gate the read — see
            // `serve_registry_packument`.
            if let Err(err) =
                authorize(state, identity, &resolved_source, name.as_str(), Action::Access)
            {
                return error_response(&err);
            }
            match load_upstream_packument_for(state, identity, source, &name).await {
                Ok(Some(bytes)) => bytes,
                Ok(None) => return not_found(),
                Err(response) => return *response,
            }
        }
        RegistrySource::Hosted(source) => {
            // The hosted gate answers a denial itself — a not-found mask or
            // an explicit-rule 401/403 — see `serve_registry_packument`.
            let org = match hosted_read_namespace(state, identity, source, name.as_str()) {
                Ok(org) => org,
                Err(response) => return *response,
            };
            match state.inner.storage.for_hosted(&org).read_hosted_packument(&name).await {
                Ok(Some(bytes)) => bytes,
                Ok(None) => return not_found(),
                Err(err) => return error_response(&err),
            }
        }
        RegistrySource::Unclaimed | RegistrySource::NotFound => return not_found(),
    };
    let packument: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(err) => return error_response(&RegistryError::Json(err)),
    };
    if let Some(osv_index) = state.inner.osv_index.as_ref() {
        let resolved = resolve_version_or_tag(&packument, version_or_tag);
        if is_osv_vulnerable_packument_version(&packument, name.as_str(), resolved, osv_index) {
            return not_found();
        }
    }
    let Some(manifest) = extract_version_manifest(&packument, &name, version_or_tag, tarball_base)
    else {
        return not_found();
    };
    match serde_json::to_vec(&manifest) {
        Ok(body) => packument_bytes_response(body, "application/json"),
        Err(err) => error_response(&RegistryError::Json(err)),
    }
}

/// The `dist.tarball` rewrite base for an upstream's `/~<name>/` registry
/// endpoint, so a served packument points tarball requests back at the same
/// endpoint (where this server re-checks access and proxies the bytes).
fn upstream_tarball_base(public_url: &str, upstream: &str) -> String {
    format!("{}/~{upstream}", public_url.trim_end_matches('/'))
}

/// Resolve the upstream behind an authorized `/~<name>/` endpoint request.
///
/// Fails closed: an upstream that does not exist or carries no `access:` policy
/// is a `404` (it is not a private-route endpoint), and a caller the policy
/// does not admit is a `403`. Returns the [`Upstream`] to fetch *through* —
/// `/~<name>/` requests never read or write the shared proxy mirror, so a
/// private upstream's packuments and tarballs can never leak across the public
/// path or another upstream.
fn authorized_upstream<'a>(
    state: &'a AppState,
    identity: &Identity,
    upstream: &str,
) -> Result<&'a Upstream, Box<Response>> {
    let Some(config) = state.inner.config.upstreams.get(upstream) else {
        return Err(Box::new(not_found()));
    };
    // A private upstream registry gates by its `access:` list; a public registry
    // (no access) is reachable by anyone at its `/~<name>/` URL, its upstream
    // credential (if any) staying server-side either way.
    if let Some(access) = config.access.as_ref()
        && !access.allows(identity)
    {
        let user = require_caller(identity, "upstream access")
            .unwrap_or_else(|_| "<anonymous>".to_string());
        return Err(Box::new(error_response(&RegistryError::Forbidden {
            user,
            action: "access",
            resource: format!("upstream {upstream:?}"),
        })));
    }
    state.inner.upstreams.get(upstream).ok_or_else(|| Box::new(not_found()))
}

/// The disposable cache namespace for an upstream registry's `/~<name>/` route —
/// the entry precomputed in [`AppInner::upstream_cache_namespaces`], falling back
/// to a fresh computation only for a name outside [`Config::upstreams`] (which
/// the registry dispatch never produces).
fn upstream_cache_namespace(state: &AppState, upstream: &str) -> String {
    state
        .inner
        .upstream_cache_namespaces
        .get(upstream)
        .cloned()
        .unwrap_or_else(|| compute_upstream_cache_namespace(&state.inner.config, upstream))
}

/// Compute an upstream registry's disposable cache namespace, so its packuments
/// and tarballs never collide with another registry's.
///
/// Both shapes fold in the registry's upstream **URL**: the cache is a mirror of
/// one declared origin, so repointing a registry's `url:` moves to a fresh
/// namespace and bytes fetched from the previous origin can never answer for
/// the new one. The cache-first warm tarball path depends on this — it serves
/// a cached entry without re-binding it against the current packument.
///
/// A **private** registry — any that declares `access:` (so it is not `public`; the
/// config loader forbids a public registry from carrying any credential) — is
/// namespaced by an HMAC over `(registry, url, credential)` keyed with
/// the server secret: the on-disk path leaks neither the registry name nor the
/// credential, and a credential rotation moves to a fresh namespace. Keying on
/// the declared visibility rather than on the presence of an `Authorization`
/// header keeps a registry whose credential rides a *custom* header (or which
/// gates access without an upstream credential) out of the guessable public
/// namespace. A **public** registry has nothing private to protect and its content
/// is integrity-verified, so it uses a *stable* namespace
/// (`~public/<digest-of-registry-name-and-url>`) that is shared across process
/// restarts.
fn compute_upstream_cache_namespace(config: &Config, upstream: &str) -> String {
    let url =
        config.upstreams.get(upstream).map_or("", |upstream_config| upstream_config.url.as_str());
    if let Some(upstream_config) = config.upstreams.get(upstream)
        && upstream_config.access.is_some()
    {
        // The credential epoch covers the origin URL and every header the
        // upstream attaches upstream, not just `Authorization`, so repointing
        // the URL or rotating a credential carried in a custom header moves
        // the private cache to a fresh namespace. The NUL separator keeps
        // `(url, headers)` pairs unambiguous — a URL cannot contain NUL.
        let epoch = crate::route::credential_digest(&format!(
            "{url}\0{}",
            crate::route::headers_credential_digest(&upstream_config.headers),
        ));
        let digest =
            crate::route::upstream_cache_digest(upstream, epoch, &config.resolution_cache_secret);
        return format!("~upstreams/{digest}");
    }
    // Public registry: a stable, secret-free namespace keyed by the registry name
    // and its origin URL (hashed so a path-unsafe value can't escape the
    // cache root).
    format!("~public/{}", crate::route::credential_digest(&format!("{upstream}\0{url}")))
}

/// Await `fut`, emitting its duration as a `pnpr::serve_timing` debug event
/// (`phase`, `package`, `elapsed_us`).
///
/// Enabling that target — `RUST_LOG=pnpr::serve_timing=debug`, or a pnpr `log:`
/// level of `debug`/`trace` — turns the upstream serve paths into a per-request
/// profile of where time goes: the upstream packument/tarball fetch vs the
/// on-disk cache read. Meant both for ad-hoc perf diagnosis (e.g. cold-store
/// regressions) and as a server-side datapoint the integrated benchmark can
/// scrape from the mock's log as a new testbed measurement, alongside its
/// client-side phase events. Near zero-cost when the target is disabled: the
/// only always-on work is one `Instant::now()`; the field values (including
/// `elapsed`) are computed only when the event is enabled.
async fn timed<Fut: Future>(phase: &'static str, package: &str, fut: Fut) -> Fut::Output {
    let start = std::time::Instant::now();
    let out = fut.await;
    tracing::debug!(
        target: "pnpr::serve_timing",
        phase,
        package,
        elapsed_us = start.elapsed().as_micros() as u64,
    );
    out
}

/// Load an upstream route's packument: a fresh per-registry cache entry when one
/// exists, otherwise a fetch through the registry (with its server-side credential)
/// written back to the same namespace. A registry with `cache: false` neither reads
/// nor writes the cache — it streams everything through, refetching each time.
async fn load_upstream_packument(
    state: &AppState,
    namespace: &str,
    upstream: &Upstream,
    name: &PackageName,
    ttl: Duration,
) -> Result<Option<Vec<u8>>, RegistryError> {
    if upstream.caches()
        && let Some(bytes) = timed(
            "packument:cache_read",
            name.as_str(),
            state.inner.storage.read_upstream_packument(namespace, name, ttl),
        )
        .await?
    {
        return Ok(Some(bytes));
    }
    let fetched = match timed(
        "packument:upstream_fetch",
        name.as_str(),
        upstream.fetch_packument(name, &CacheValidators::default()),
    )
    .await
    {
        Ok(fetched) => fetched,
        // Stale-if-error: a stale entry is refetched, but if the upstream is
        // unreachable, serve the last cached body for this same origin rather
        // than failing. This preserves availability during a transient outage
        // and is not a cross-origin fall-through — the bytes came from this very
        // registry. A clean `NotFound` is an `Ok` variant below, so it never lands
        // here and stays an authoritative 404.
        Err(err) => {
            // Only mask a *transient* availability failure (transport, 5xx, open
            // circuit). A 4xx is authoritative about this request — auth revoked,
            // `410 Gone`, throttled — and must surface rather than be answered
            // from old bytes.
            if err.is_transient_upstream_error()
                && upstream.caches()
                && let Some(bytes) =
                    state.inner.storage.read_upstream_packument_any(namespace, name).await?
            {
                // `log_message()` (not `?err`): an upstream error embeds the
                // request URL, which can carry credentials (basic-auth userinfo,
                // a token query param). Log the credential-redacted rendering.
                tracing::warn!(
                    error = %err.log_message(),
                    package = %name.as_str(),
                    "upstream packument refetch failed; serving stale cache",
                );
                return Ok(Some(bytes));
            }
            return Err(err);
        }
    };
    match fetched {
        PackumentFetch::Modified(fetched) => {
            if upstream.caches()
                && let Err(err) = state
                    .inner
                    .storage
                    .write_upstream_packument(namespace, name, &fetched.bytes)
                    .await
            {
                tracing::warn!(?err, package = %name.as_str(), "upstream packument cache write failed");
            }
            Ok(Some(fetched.bytes))
        }
        PackumentFetch::NotFound => {
            // The 404 is authoritative: the package is gone from this origin,
            // so drop its cached entry too. Otherwise the stale copy would
            // outlive every TTL and a later transient outage could resurrect
            // the unpublished package through the stale-if-error fallback.
            if upstream.caches()
                && let Err(err) = state.inner.storage.remove_upstream_package(namespace, name).await
            {
                tracing::warn!(
                    ?err,
                    package = %name.as_str(),
                    "failed to purge cached entry after an upstream 404",
                );
            }
            Ok(None)
        }
        // `load_upstream_packument` sends no conditional validators (the upstream
        // cache refetches stale entries rather than revalidating — see
        // `Store::read_upstream_packument`), so a well-behaved upstream never
        // answers 304 here. If one does anyway, "not modified" means the cached
        // body is current, so serve it (fresh or stale) rather than a spurious
        // 404 that a client could cache as "package gone".
        PackumentFetch::NotModified => {
            state.inner.storage.read_upstream_packument_any(namespace, name).await
        }
    }
}

/// Authorize and load an upstream registry's packument bytes (from its per-registry
/// private cache, else a fresh fetch through the registry), or a [`Response`]
/// error the caller should return. Shared by the packument and version-manifest
/// serving paths.
async fn load_upstream_packument_for(
    state: &AppState,
    identity: &Identity,
    upstream: &str,
    name: &PackageName,
) -> Result<Option<Vec<u8>>, Box<Response>> {
    let namespace = upstream_cache_namespace(state, upstream);
    let upstream = authorized_upstream(state, identity, upstream)?;
    let ttl = upstream.maxage().unwrap_or(state.inner.config.packument_ttl);
    load_upstream_packument(state, &namespace, upstream, name, ttl)
        .await
        .map_err(|err| Box::new(error_response(&err)))
}

/// Load a package's packument bytes through the addressed `/~<name>/` (or,
/// path-less, the default-target registry) — resolving to one concrete origin and
/// reading there, with no fall-through. `Ok(None)` is a definitive not-found
/// (unknown package, no route, no default target, or an unauthorized private
/// hosted org). Used by the readers that aren't
/// packument/tarball/version-manifest (e.g. `dist-tags`).
async fn load_packument_for_read(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    name: &PackageName,
) -> Result<Option<Vec<u8>>, Box<Response>> {
    let target = match registry {
        Some(registry) => registry.to_string(),
        None => match default_registry_target(state) {
            Some(target) => target,
            None => return Ok(None),
        },
    };
    // The resolved registry's per-package rules apply to every served read,
    // upstream or hosted — otherwise a restricted package would leak (e.g.
    // its dist-tags) through these path-less readers. A hosted denial is a
    // not-found mask rather than a 401/403 that reveals existence (see
    // `serve_registry_packument`).
    let resolved_source = resolve_registry_source(state, &target, name.as_str());
    match &resolved_source {
        RegistrySource::Upstream(source) => {
            if let Err(err) =
                authorize(state, identity, &resolved_source, name.as_str(), Action::Access)
            {
                return Err(Box::new(error_response(&err)));
            }
            load_upstream_packument_for(state, identity, source, name).await
        }
        RegistrySource::Hosted(source) => {
            let org = match hosted_gate(state, identity, source, name.as_str()) {
                HostedGate::Allowed(org) => org,
                HostedGate::MaskNotFound => return Ok(None),
                HostedGate::Denied(err) => return Err(Box::new(error_response(&err))),
            };
            state
                .inner
                .storage
                .for_hosted(&org)
                .read_hosted_packument(name)
                .await
                .map_err(|err| Box::new(error_response(&err)))
        }
        RegistrySource::Unclaimed | RegistrySource::NotFound => Ok(None),
    }
}

async fn serve_packument_via_upstream(
    state: &AppState,
    identity: &Identity,
    headers: &HeaderMap,
    upstream: &str,
    name: &PackageName,
    tarball_base: &str,
) -> Response {
    let bytes = match load_upstream_packument_for(state, identity, upstream, name).await {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return not_found(),
        Err(response) => return *response,
    };
    match packument_response(
        name,
        &bytes,
        tarball_base,
        state.inner.osv_index.as_ref(),
        wants_abbreviated(headers),
    ) {
        Ok(response) => response,
        Err(err) => error_response(&err),
    }
}

/// Serve a tarball through an upstream's `/~<name>/` endpoint. The version's
/// `dist.integrity` is read from the upstream's own packument (served from the
/// private cache when fresh), and the bytes are verified against it. Both the
/// packument and the verified tarball are cached under the upstream's private
/// namespace, so a private upstream's content never lands in the shared proxy
/// mirror yet is not re-fetched on every request.
async fn serve_tarball_via_upstream(
    state: &AppState,
    identity: &Identity,
    upstream: &str,
    raw_name: &str,
    filename: &str,
) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };
    // A canonical `<basename>-<version>.tgz` (or the scoped wire form) is
    // normalized as usual. A non-canonical basename preserved verbatim from
    // the upstream's `dist.tarball` (see `rewrite_tarball_urls`) is accepted
    // opaquely so long as it is safe as a cache path segment — the packument
    // match below is what authorizes it, binding it to a declared version
    // and integrity. Rejecting it here would make such a version
    // un-fetchable through the very URL this server advertised.
    let (filename, parsed_version) = match name.parse_tarball_name(filename) {
        Ok((canonical, version)) => (canonical, Some(version)),
        Err(err) => {
            if !crate::package_name::is_safe_path_segment(filename) {
                return error_response(&err);
            }
            (filename.to_string(), None)
        }
    };
    let namespace = upstream_cache_namespace(state, upstream);
    let upstream = match authorized_upstream(state, identity, upstream) {
        Ok(upstream) => upstream,
        Err(response) => return *response,
    };
    // Pre-check OSV on the filename-derived version (when the name is
    // canonical) to fail fast; the authoritative check against the
    // packument-resolved version runs below either way.
    if let Some(version) = &parsed_version
        && let Err(err) = ensure_osv_allowed(state, &name, version)
    {
        return error_response(&err);
    }
    let ttl = upstream.maxage().unwrap_or(state.inner.config.packument_ttl);
    // Serve a cached hit before touching the packument: a cached entry was
    // bound to a declared version and verified against `dist.integrity` when
    // it was written, and the client re-verifies what it receives, so no
    // re-bind or re-hash is needed. The packument load — and the full-document
    // JSON parse in `expected_tarball_dist` — costs milliseconds per request
    // for a large package and would dominate warm tarball serving.
    //
    // Deliberately, a hit is NOT re-bound against the packument as it stands
    // *now*: a version unpublished since the write stays downloadable from
    // this disposable mirror until the entry is wiped (registry-CDN
    // semantics; resolution already stops offering it once the refreshed
    // packument drops it), and a hostile packument rewrite — say, duplicate
    // `dist.tarball` basenames — cannot retroactively poison bytes that were
    // verified on the way in. The fail-closed bind below protects the *fetch*
    // of new bytes; end-to-end SRI (the client's lockfile) is the authority
    // on what it accepts. Only OSV screening needs the packument-resolved
    // version first, so with OSV enabled the cache read waits for the bind
    // below. A `cache: false` upstream skips the cache and streams through.
    if upstream.caches()
        && state.inner.osv_index.is_none()
        && let Some(response) = cached_upstream_tarball(state, &namespace, &name, &filename).await
    {
        return response;
    }
    let packument = match timed(
        "tarball:packument_load",
        name.as_str(),
        load_upstream_packument(state, &namespace, upstream, &name, ttl),
    )
    .await
    {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return not_found(),
        Err(err) => return error_response(&err),
    };
    let TarballDist { version, integrity } =
        match expected_tarball_dist(&packument, &name, &filename) {
            Ok(Some(dist)) => dist,
            Ok(None) => return not_found(),
            Err(err) => return error_response(&err),
        };
    if parsed_version.as_deref() != Some(version.as_str())
        && let Err(err) = ensure_osv_allowed(state, &name, &version)
    {
        return error_response(&err);
    }
    if upstream.caches()
        && state.inner.osv_index.is_some()
        && let Some(response) = cached_upstream_tarball(state, &namespace, &name, &filename).await
    {
        return response;
    }

    let response = match timed(
        "tarball:upstream_fetch",
        name.as_str(),
        upstream.fetch_tarball_response(&name, &filename),
    )
    .await
    {
        Ok(FetchOutcome::Ok(response)) => response,
        Ok(FetchOutcome::NotFound) => return not_found(),
        Err(err) => return error_response(&err),
    };
    let write =
        match state.inner.storage.open_upstream_tarball_tmp(&namespace, &name, &filename).await {
            Ok(write) => write,
            Err(err) => return error_response(&err),
        };
    if !upstream.caches() {
        // Fetch-through: verify and stream from the temp file, then remove it,
        // so a `cache: false` upstream's tarball is never persisted.
        return match streaming::download_verified_to_temp(
            response,
            write,
            &integrity,
            MAX_TARBALL_BYTES,
        )
        .await
        {
            Ok((file, len, tmp_path)) => {
                tarball_response(streaming::stream_file_and_remove(file, tmp_path), Some(len))
            }
            Err(err) => error_response(&tarball_stream_error(err, &name, &filename)),
        };
    }
    // Stream the download to the client while teeing it into the namespaced
    // cache; the entry is promoted only on an SRI match (see
    // `stream_verified_to_cache`). No `Content-Length` is set: the upstream's
    // is attacker-controlled and unverifiable before streaming, so the body is
    // chunked and the client reads to EOF (then re-verifies the integrity).
    match streaming::stream_verified_to_cache(response, write, &integrity, MAX_TARBALL_BYTES) {
        Ok(body) => tarball_response(body, None),
        Err(err) => error_response(&tarball_stream_error(err, &name, &filename)),
    }
}

/// The response for a cached upstream tarball, or `None` on a cache miss. A
/// cache-open fault is logged and treated as a miss so the caller falls back
/// to the upstream fetch rather than failing the request.
async fn cached_upstream_tarball(
    state: &AppState,
    namespace: &str,
    name: &PackageName,
    filename: &str,
) -> Option<Response> {
    match timed(
        "tarball:cache_read",
        name.as_str(),
        state.inner.storage.open_upstream_tarball(namespace, name, filename),
    )
    .await
    {
        Ok(Some((file, len))) => Some(tarball_response(streaming::stream_file(file), Some(len))),
        Ok(None) => None,
        Err(err) => {
            tracing::warn!(?err, package = %name.as_str(), %filename, "upstream tarball cache open failed");
            None
        }
    }
}

// --------------------------------------------------------------------
// Registry dispatch. A `/~<name>/` request resolves the package to
// exactly one concrete origin through the validated registry graph
// ([`crate::registry`]) and serves it there — authoritatively. Every concrete
// registry's declared `patterns:` are enforced here, before storage or any
// upstream is consulted, on the direct address and through a router alike; a
// router selects the first source whose patterns claim the name. An unclaimed
// name is a definitive 404 (never a fall-through to another origin), and a
// selected-but-unavailable upstream surfaces an *error* rather than a 404
// (the via-upstream path returns `UpstreamUnavailable`), so a down private
// source can never be reported as "not found" and pushed onto a public origin
// one layer out.
// --------------------------------------------------------------------

/// The concrete origin a `/~<name>/` request resolved to, owned so it can be
/// held across an `await` without borrowing the config.
enum RegistrySource {
    /// An upstream registry (public or private), served via its `/~<source>/`
    /// upstream machinery. The id is a key in [`Config::upstreams`].
    Upstream(String),
    /// A hosted registry, served from the hosted store.
    Hosted(String),
    /// No declared namespace claims the package — the addressed registry's
    /// patterns don't cover it, or none of a router's sources claim it. A
    /// definitive 404 on reads; writes reject it with a reason instead, so a
    /// typo'd scope fails loudly rather than 404-ing later.
    Unclaimed,
    /// The registry id is unknown — a definitive not-found with no fall-through.
    NotFound,
}

/// The registry the path-less base (`https://<pnpr>/`) aliases, owned so it can be
/// held across an `await`. `None` disables the path-less base entirely — the
/// bare host has no registry and every request is a not-found, so clients must
/// address a `/~<name>/`. There is no legacy hosted-then-proxy path: a
/// path-less request resolves through the registry graph or it does not resolve.
fn default_registry_target(state: &AppState) -> Option<String> {
    state.inner.config.registries.default_registry().map(str::to_string)
}

fn resolve_registry_source(state: &AppState, registry: &str, package: &str) -> RegistrySource {
    match state.inner.config.registries.resolve(registry, package) {
        Resolved::Concrete { registry, kind: ConcreteKind::Upstream } => {
            RegistrySource::Upstream(registry.to_string())
        }
        Resolved::Concrete { registry, kind: ConcreteKind::Hosted } => {
            RegistrySource::Hosted(registry.to_string())
        }
        // An unclaimed name is definitive — never a fall-through to another
        // origin, and never a storage or upstream consultation.
        Resolved::Unclaimed => RegistrySource::Unclaimed,
        // The graph is the only dispatch table: server construction folds
        // every configured upstream into it (`ensure_valid_registry_graph`), so
        // a name it doesn't know is a definitive not-found — there is no
        // upstream-table side door that would skip namespace enforcement.
        Resolved::UnknownRegistry => RegistrySource::NotFound,
    }
}

/// Whether the concrete origin `package` resolves to through `registry` serves
/// caller-gated content: a hosted registry whose access list denies anonymous
/// callers, or an upstream registry that declares `access:`. Responses from such
/// an origin vary by `Authorization` and must never land in a shared HTTP
/// cache, whichever URL surface (path-less or `/~<name>/`) served them.
fn resolves_to_private_source(state: &AppState, registry: &str, package: &str) -> bool {
    match resolve_registry_source(state, registry, package) {
        RegistrySource::Hosted(source) => {
            state.inner.config.hosted.get(&source).is_some_and(|hosted| {
                !hosted.rules.for_package(package).access.allows(&Identity::Anonymous)
            })
        }
        // A private upstream (registry-level `access:`) is caller-gated for
        // *every* name — unlike a hosted registry, its registry-level gate is
        // enforced independently at serving (`authorized_upstream` runs
        // before per-package rules on every upstream read), so a per-package
        // `access: $all` entry cannot open a name on it and `access.is_some()`
        // alone already means the response varies by caller. A public
        // upstream can still gate individual names through a per-package
        // `access` rule.
        RegistrySource::Upstream(source) => {
            state.inner.config.upstreams.get(&source).is_some_and(|upstream| {
                upstream.access.is_some()
                    || !upstream.rules.for_package(package).access.allows(&Identity::Anonymous)
            })
        }
        RegistrySource::Unclaimed | RegistrySource::NotFound => false,
    }
}

/// Apply the private-cache headers to a path-less response whenever it can
/// vary by caller — the default-target resolution for `package` lands on a
/// source whose effective per-package access denies anonymous callers (so the
/// same URL answers differently depending on `Authorization`, even through a
/// public registry). These are the same headers the `/~<name>/` surface applies
/// unconditionally, so the two URL surfaces for the same content get the same
/// defense against a shared HTTP cache replaying an authenticated response to
/// an anonymous caller. A publicly-readable resolution stays cacheable: the
/// path-less base is the hot install path.
fn private_if_caller_gated(state: &AppState, package: &str, response: Response) -> Response {
    match default_registry_target(state) {
        Some(target) if resolves_to_private_source(state, &target, package) => {
            private_no_cache(response)
        }
        _ => response,
    }
}

/// Serve a packument addressed to `/~<name>/<pkg>` through the registry graph.
async fn serve_registry_packument(
    state: &AppState,
    identity: &Identity,
    headers: &HeaderMap,
    registry: &str,
    raw_name: &str,
    tarball_base: &str,
) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };
    // `tarball_base` is the URL the *client* addressed (the path-less host or a
    // `/~<name>/`), not the resolved source's `/~<source>/`. The served
    // packument's `dist.tarball` URLs must stay canonical for that base so a
    // client's lockfile drops them — persisting the resolved source path would
    // bake the registry name in and break lockfile portability.
    let resolved_source = resolve_registry_source(state, registry, name.as_str());
    match &resolved_source {
        RegistrySource::Upstream(source) => {
            // The upstream registry's per-package rules gate every served
            // read, so an access-gated name can't be read even through a
            // public upstream. Checked before serving so the decision
            // precedes any existence-revealing signal like an OSV 403.
            if let Err(err) =
                authorize(state, identity, &resolved_source, name.as_str(), Action::Access)
            {
                return error_response(&err);
            }
            serve_packument_via_upstream(state, identity, headers, source, &name, tarball_base)
                .await
        }
        // A hosted denial answers per its gate tier (see `hosted_gate`): a
        // registry-default denial is a not-found mask, an explicit
        // `packages:` entry denies loudly so clients can prompt for auth.
        RegistrySource::Hosted(source) => {
            serve_hosted_packument(state, identity, headers, source, &name, tarball_base).await
        }
        RegistrySource::Unclaimed | RegistrySource::NotFound => not_found(),
    }
}

/// Serve a tarball addressed to `/~<name>/<pkg>/-/<file>` through the registry
/// graph. Routing is deterministic by package name, so the tarball resolves to
/// the same concrete source the packument did.
async fn serve_registry_tarball(
    state: &AppState,
    identity: &Identity,
    registry: &str,
    raw_name: &str,
    filename: &str,
) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };
    let resolved_source = resolve_registry_source(state, registry, name.as_str());
    match &resolved_source {
        RegistrySource::Upstream(source) => {
            // Per-package rules before serving — see `serve_registry_packument`.
            if let Err(err) =
                authorize(state, identity, &resolved_source, name.as_str(), Action::Access)
            {
                return error_response(&err);
            }
            serve_tarball_via_upstream(state, identity, source, name.as_str(), filename).await
        }
        // A hosted denial is a not-found mask, inside `serve_hosted_tarball`
        // — see `serve_registry_packument`.
        RegistrySource::Hosted(source) => {
            serve_hosted_tarball(state, identity, source, &name, filename).await
        }
        RegistrySource::Unclaimed | RegistrySource::NotFound => not_found(),
    }
}

/// How a hosted registry answers a read of `package` for `identity`:
/// admitted with the storage namespace to read from, or denied one of two
/// ways. The two denial shapes preserve the two authorization tiers the
/// merged `packages:` map folds together: an **explicit** entry's `access`
/// is declared, discoverable config — deny loudly (401/403, so a client can
/// prompt for credentials, the registry-mock `needs-auth` contract) — while
/// the registry-level **default** masks as not-found, so a blanket-private
/// registry never reveals which names exist.
enum HostedGate {
    Allowed(String),
    /// The registry default denies the caller: indistinguishable from an
    /// absent package.
    MaskNotFound,
    /// An explicit `packages:` entry denies the caller: 401 for an
    /// anonymous caller (authenticate and retry), 403 for an authenticated
    /// one outside the allowed set.
    Denied(RegistryError),
}

/// Evaluate the hosted read gate: the effective per-package `access` (most
/// specific `packages:` entry, falling back to the registry-level default)
/// gates reads and the write routing alike — a caller who may not read a
/// hosted package may not publish, tag, or unpublish it either.
fn hosted_gate(state: &AppState, identity: &Identity, source: &str, package: &str) -> HostedGate {
    let Some(hosted) = state.inner.config.hosted.get(source) else {
        return HostedGate::MaskNotFound;
    };
    let effective = hosted.rules.for_package(package);
    if effective.access.allows(identity) {
        return HostedGate::Allowed(hosted.org.clone());
    }
    // Loud denial only inside a registry the caller may see: the explicit
    // entry gates this name, but the registry-level default admits the
    // caller to the registry itself. When the default denies them too, the
    // mask below wins — an explicit rule on a blanket-private registry must
    // not become an existence probe.
    if effective.access_is_explicit && hosted.rules.default_access().allows(identity) {
        return HostedGate::Denied(match identity {
            Identity::Anonymous => {
                RegistryError::Unauthenticated { resource: format!("package {package:?}") }
            }
            Identity::User { username, .. } => RegistryError::Forbidden {
                user: username.clone(),
                action: "access",
                resource: format!("package {package:?}"),
            },
        });
    }
    HostedGate::MaskNotFound
}

/// [`hosted_gate`] flattened to a `Result` for the readers: the org to read
/// from, or the response to answer with.
fn hosted_read_namespace(
    state: &AppState,
    identity: &Identity,
    source: &str,
    package: &str,
) -> Result<String, Box<Response>> {
    match hosted_gate(state, identity, source, package) {
        HostedGate::Allowed(org) => Ok(org),
        HostedGate::MaskNotFound => Err(Box::new(not_found())),
        HostedGate::Denied(err) => Err(Box::new(error_response(&err))),
    }
}

async fn serve_hosted_packument(
    state: &AppState,
    identity: &Identity,
    headers: &HeaderMap,
    source: &str,
    name: &PackageName,
    tarball_base: &str,
) -> Response {
    let org = match hosted_read_namespace(state, identity, source, name.as_str()) {
        Ok(org) => org,
        Err(response) => return *response,
    };
    // A hosted org has no upstream fallback: a package it does not host is a
    // definitive not-found. Reads come from the org's own storage namespace.
    match state.inner.storage.for_hosted(&org).read_hosted_packument(name).await {
        Ok(Some(bytes)) => match packument_response(
            name,
            &bytes,
            tarball_base,
            state.inner.osv_index.as_ref(),
            wants_abbreviated(headers),
        ) {
            Ok(response) => response,
            Err(err) => error_response(&err),
        },
        Ok(None) => not_found(),
        Err(err) => error_response(&err),
    }
}

async fn serve_hosted_tarball(
    state: &AppState,
    identity: &Identity,
    source: &str,
    name: &PackageName,
    filename: &str,
) -> Response {
    let org = match hosted_read_namespace(state, identity, source, name.as_str()) {
        Ok(org) => org,
        Err(response) => return *response,
    };
    let (filename, name_version) = match name.parse_tarball_name(filename) {
        Ok(parsed) => parsed,
        Err(err) => return error_response(&err),
    };
    if let Err(err) = ensure_osv_allowed(state, name, &name_version) {
        return error_response(&err);
    }
    match state.inner.storage.for_hosted(&org).open_hosted_tarball(name, &filename).await {
        Ok(Some((body, len))) => tarball_response(body, len),
        Ok(None) => not_found(),
        Err(err) => {
            tracing::warn!(?err, package = %name.as_str(), %filename, "hosted tarball open failed");
            error_response(&err)
        }
    }
}

async fn serve_tarball(
    state: &AppState,
    identity: &Identity,
    raw_name: &str,
    filename: &str,
) -> Response {
    // The path-less base is an alias for the default-target registry — see
    // `serve_packument`. With no default target the bare host has no registry.
    match default_registry_target(state) {
        Some(target) => {
            let response =
                serve_registry_tarball(state, identity, &target, raw_name, filename).await;
            private_if_caller_gated(state, raw_name, response)
        }
        None => not_found(),
    }
}

/// The version a tarball request resolves to, plus that version's declared
/// `dist.integrity`. The version is found by matching `filename` against
/// each version's `dist.tarball` basename rather than parsing it out of
/// the filename, so a non-canonical name (see [`rewrite_tarball_urls`])
/// resolves to the right version, integrity, and OSV identity.
struct TarballDist {
    version: String,
    integrity: Integrity,
}

/// The `versions[v].dist` subset the tarball serve path reads. Every tarball
/// request re-reads its package's packument to bind the filename to a
/// declared version and integrity; deserializing into this projection instead
/// of a full `serde_json::Value` skips building (and allocating) the rest of
/// the document on that hot path.
#[derive(serde::Deserialize)]
struct PackumentDists {
    #[serde(default)]
    versions: IndexMap<String, VersionDist>,
}

#[derive(serde::Deserialize)]
struct VersionDist {
    #[serde(default)]
    dist: Option<DistBlock>,
}

#[derive(serde::Deserialize)]
struct DistBlock {
    #[serde(default)]
    tarball: Option<String>,
    #[serde(default)]
    integrity: Option<String>,
    /// Legacy hex sha1 — the only hash pre-2017 npm publishes carry.
    #[serde(default)]
    shasum: Option<String>,
}

fn expected_tarball_dist(
    packument: &[u8],
    name: &PackageName,
    filename: &str,
) -> Result<Option<TarballDist>, RegistryError> {
    let packument: PackumentDists = serde_json::from_slice(packument)?;
    let mut matches = packument.versions.iter().filter_map(|(version, manifest)| {
        let dist = manifest.dist.as_ref()?;
        dist.tarball
            .as_deref()
            .and_then(tarball_basename)
            .is_some_and(|basename| basename == filename)
            .then_some((version, dist))
    });
    let Some((version, dist)) = matches.next() else {
        return Ok(None);
    };
    // A tarball name must identify exactly one declaring version, or the
    // integrity and OSV checks below could bind to the wrong one. Two
    // versions sharing a basename is a malformed/hostile packument, never a
    // legitimate registry, so fail closed rather than pick by iteration order.
    if matches.next().is_some() {
        return Err(tarball_integrity_error(
            name,
            filename,
            "packument declares the same dist.tarball basename for multiple versions".to_string(),
        ));
    }
    // Prefer the SRI `integrity`; fall back to the legacy hex `shasum`
    // (pre-2017 npm publishes carry only that) so those packages stay
    // proxyable — still verified, just against sha1. A version declaring
    // neither stays unservable: bytes never leave unverified.
    let integrity = if let Some(declared) = dist.integrity.as_deref() {
        streaming::parse_integrity(declared).map_err(|err| {
            tarball_integrity_error(name, filename, format!("malformed dist.integrity: {err}"))
        })?
    } else {
        let shasum = dist.shasum.as_deref().ok_or_else(|| {
            tarball_integrity_error(
                name,
                filename,
                format!("packument has no dist.integrity or dist.shasum for {version:?}"),
            )
        })?;
        Integrity::from_hex(shasum, ssri::Algorithm::Sha1).map_err(|err| {
            tarball_integrity_error(name, filename, format!("malformed dist.shasum: {err}"))
        })?
    };
    Ok(Some(TarballDist { version: version.clone(), integrity }))
}

fn tarball_stream_error(
    err: streaming::TarballStreamError,
    name: &PackageName,
    filename: &str,
) -> RegistryError {
    match err {
        streaming::TarballStreamError::Upstream { url, source } => {
            RegistryError::Upstream { url, source }
        }
        streaming::TarballStreamError::Io(err) => RegistryError::Io(err),
        streaming::TarballStreamError::Integrity(err) => {
            tarball_integrity_error(name, filename, format!("integrity verification failed: {err}"))
        }
        streaming::TarballStreamError::TooLarge { limit, received } => tarball_integrity_error(
            name,
            filename,
            format!("tarball body exceeds {limit} byte limit (received {received} bytes)"),
        ),
    }
}

fn tarball_integrity_error(name: &PackageName, filename: &str, reason: String) -> RegistryError {
    RegistryError::TarballIntegrity {
        package: name.as_str().to_string(),
        filename: filename.to_string(),
        reason,
    }
}

/// Add a new user or log in an existing one. Mirrors verdaccio's
/// `/-/user/org.couchdb.user/:name` behavior:
///
/// * unknown user → create + return 201 with `{ ok, token }`.
/// * existing user, password matches → return 201 with `{ ok, token }`.
/// * existing user, password wrong → 401.
async fn add_user(state: &AppState, name: &str, body: &[u8]) -> Response {
    // axum's `Path` extractor already percent-decodes path segments
    // (`%2F` → `/`, `%40` → `@`, etc.), so we use `name` verbatim.
    let body: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(err) => return error_response(&RegistryError::Json(err)),
    };
    let body_name = body.get("name").and_then(Value::as_str).unwrap_or("");
    if body_name != name {
        return error_response(&RegistryError::BadRequest {
            reason: format!("username in URL ({name:?}) does not match body ({body_name:?})"),
        });
    }
    let Some(password) = body.get("password").and_then(Value::as_str) else {
        return error_response(&RegistryError::BadRequest {
            reason: "missing password".to_string(),
        });
    };

    let (outcome, username) = match state.inner.auth.users.add_or_login(name, password).await {
        Ok(o) => o,
        Err(err) => return error_response(&err),
    };
    let token = match state.inner.auth.tokens.issue(&username).await {
        Ok(t) => t,
        Err(err) => return error_response(&err),
    };
    let ok_msg = match outcome {
        UpsertOutcome::Created => format!("user '{username}' created"),
        UpsertOutcome::LoggedIn => format!("you are authenticated as '{username}'"),
    };
    let body =
        json!({ "ok": ok_msg, "token": token, "id": format!("org.couchdb.user:{username}") });
    let bytes = serde_json::to_vec(&body).expect("static-shape JSON serializes");
    Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(bytes))
        .expect("static-shape response always builds")
}

/// `GET /-/whoami` — return the username of the caller, or 401 if
/// the request is anonymous. `npm whoami` reads this. The check is
/// pure auth: no per-package policy applies, so anonymous always
/// gets 401 even when `$all` would let it through for packument
/// reads.
fn serve_whoami(identity: &Identity) -> Response {
    let username = match require_caller(identity, "user identity") {
        Ok(username) => username,
        Err(err) => return error_response(&err),
    };
    json_response(StatusCode::OK, &json!({ "username": username }))
}

/// `GET /-/npm/v1/user` — return the profile of the authenticated
/// caller. `npm profile get` reads this. pnpr doesn't track email,
/// 2FA, or anything beyond the username; the absent fields surface
/// as their zero-value defaults so the npm CLI's table renderer
/// doesn't choke on a missing key.
fn serve_profile(identity: &Identity) -> Response {
    let username = match require_caller(identity, "user profile") {
        Ok(username) => username,
        Err(err) => return error_response(&err),
    };
    json_response(
        StatusCode::OK,
        &json!({
            "name": username,
            "email": "",
            "email_verified": false,
            "tfa": false,
            "fullname": "",
            "cidr_whitelist": null,
        }),
    )
}

/// `GET /-/npm/v1/tokens` — list every bearer token issued to the
/// authenticated caller. Returns the npm-CLI-compatible wrapper
/// (`{ objects, urls }`) so `npm token list` parses it cleanly. The
/// raw token itself is never persisted; the `token` field surfaces
/// the leading 6 hex characters of the key as a preview, matching
/// what verdaccio does when it can't reconstruct the original.
async fn list_tokens(state: &AppState, identity: &Identity) -> Response {
    let username = match require_caller(identity, "token list") {
        Ok(username) => username,
        Err(err) => return error_response(&err),
    };
    let tokens = match state.inner.auth.tokens.list_for_user(&username).await {
        Ok(tokens) => tokens,
        Err(err) => return error_response(&err),
    };
    let objects: Vec<Value> =
        tokens.into_iter().map(|(key, record)| token_response_object(&key, &record)).collect();
    json_response(StatusCode::OK, &json!({ "objects": objects, "urls": {} }))
}

/// `DELETE /-/npm/v1/tokens/token/:key` — revoke a token by its
/// listing-side key. The caller must be the owner of the token
/// (anonymous is 401, a different authenticated user is 403); an
/// unknown key returns 404. `npm token revoke` calls this with the
/// `key` it pulled from [`list_tokens`].
async fn revoke_token_by_key(state: &AppState, identity: &Identity, key: &str) -> Response {
    let username = match require_caller(identity, "token revocation") {
        Ok(username) => username,
        Err(err) => return error_response(&err),
    };
    match state.inner.auth.tokens.find_by_key(key).await {
        Ok(Some(record)) if record.username != username => {
            error_response(&RegistryError::Forbidden {
                user: username,
                action: "revoke",
                resource: "this token".to_string(),
            })
        }
        Ok(Some(_)) => match state.inner.auth.tokens.revoke_by_key(key).await {
            Ok(Some(_)) => json_response(StatusCode::OK, &json!({ "ok": "token revoked" })),
            Ok(None) => not_found(),
            Err(err) => error_response(&err),
        },
        Ok(None) => not_found(),
        Err(err) => error_response(&err),
    }
}

/// `DELETE /-/user/token/:tok` — npm logout. The path holds the raw
/// bearer token (npm sends it verbatim alongside an
/// `Authorization: Bearer <tok>` header). We require authentication
/// and require that the auth identifies the same user who owns the
/// token being deleted.
async fn logout(state: &AppState, identity: &Identity, raw_token: &str) -> Response {
    let username = match require_caller(identity, "logout") {
        Ok(username) => username,
        Err(err) => return error_response(&err),
    };
    let target_owner = match state.inner.auth.tokens.lookup(raw_token).await {
        Ok(Some(owner)) => owner,
        Ok(None) => return not_found(),
        Err(err) => return error_response(&err),
    };
    if target_owner != username {
        return error_response(&RegistryError::Forbidden {
            user: username,
            action: "revoke",
            resource: "this token".to_string(),
        });
    }
    match state.inner.auth.tokens.revoke_by_raw(raw_token).await {
        Ok(Some(_)) => json_response(StatusCode::OK, &json!({ "ok": true })),
        Ok(None) => not_found(),
        Err(err) => error_response(&err),
    }
}

fn token_response_object(key: &str, record: &crate::auth::TokenRecord) -> Value {
    let preview: String = key.chars().take(6).collect();
    let created = token_timestamp_iso(record.created_at);
    let updated = token_timestamp_iso(record.last_used_at);
    json!({
        "key": key,
        "token": preview,
        "user": record.username,
        "cidr_whitelist": record.cidr_whitelist,
        "readonly": record.readonly,
        "created": created,
        "updated": updated,
    })
}

fn token_timestamp_iso(seconds: u64) -> String {
    iso_from_unix_millis(token_timestamp_millis(seconds))
}

fn token_timestamp_millis(seconds: u64) -> i64 {
    const MILLIS_PER_SECOND: u64 = 1000;
    let max_seconds = i64::MAX as u64 / MILLIS_PER_SECOND;
    (seconds.min(max_seconds) * MILLIS_PER_SECOND) as i64
}

mod staged;

#[cfg(test)]
mod tests;

/// Require that an endpoint's caller is authenticated, returning their
/// username or the 401 error to send back. The identity was already
/// resolved by the [`authenticate`] middleware (which is also where an
/// auth-backend outage surfaces as a 5xx), so this is a pure check.
/// `resource` names what the 401 is about.
fn require_caller(identity: &Identity, resource: &str) -> Result<String, RegistryError> {
    match identity {
        Identity::User { username, .. } => Ok(username.clone()),
        Identity::Anonymous => {
            Err(RegistryError::Unauthenticated { resource: resource.to_string() })
        }
    }
}

async fn caller_username(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<String>, RegistryError> {
    let authorization = single_authorization_header(headers)?;
    identify(authorization, state.inner.auth.tokens.as_ref()).await
}

async fn require_resolver_caller(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    match caller_username(&state, request.headers()).await {
        Ok(Some(_username)) => next.run(request).await,
        Ok(None) => error_response(&RegistryError::Unauthenticated {
            resource: "dependency resolution".to_string(),
        }),
        Err(error) => error_response(&error),
    }
}

fn single_authorization_header(headers: &HeaderMap) -> Result<Option<&str>, RegistryError> {
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(RegistryError::BadRequest {
            reason: "multiple Authorization headers are not allowed".to_string(),
        });
    }
    value.to_str().map(Some).map_err(|_| RegistryError::BadRequest {
        reason: "Authorization header is not valid text".to_string(),
    })
}

fn json_response(status: StatusCode, body: &Value) -> Response {
    let bytes = serde_json::to_vec(body).expect("static-shape JSON serializes");
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .expect("static-shape response always builds")
}

/// Mark a response as caller-scoped and uncacheable. The auth
/// endpoints (whoami, profile, token list/revoke, logout) return
/// per-user data keyed on the `Authorization` header, so a shared
/// HTTP cache that ignored `Vary` could happily hand one user's
/// identity to another. Applied to *every* branch of those handlers
/// — success and error alike — so an intermediary can't latch onto
/// a 401 either.
fn private_no_cache(mut response: Response) -> Response {
    use axum::http::HeaderValue;
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    headers.insert(header::VARY, HeaderValue::from_static("Authorization"));
    response
}

/// The hosted storage view a publish writes to: a hosted namespace, or
/// the flat (path-less) store when `org` is `None`.
fn hosted_storage(state: &AppState, org: Option<&str>) -> Storage {
    match org {
        Some(org) => state.inner.storage.for_hosted(org),
        None => state.inner.storage.clone(),
    }
}

/// Where a publish of `package` writes, given an optional explicit `/~<name>/`.
enum PublishTarget {
    /// Write into the hosted registry `source`'s storage namespace `org`.
    /// The source name is carried so the write's `publish`/`unpublish`
    /// authorization can consult that registry's `packages:` rules.
    Hosted { source: String, org: String },
    /// The resolved target is not a hosted org; reject with this reason.
    Reject(String),
    /// The resolved upstream registry denies this caller; answer with the
    /// same response its reads give (a 403), before any rejection that would
    /// narrate routing config.
    Denied(Box<Response>),
    /// The addressed registry or route does not exist (or the path-less base has
    /// no default target).
    NotFound,
}

/// Resolve where a publish lands. A write may only target a hosted registry
/// whose declared patterns claim the name: a selection of an upstream is
/// rejected ("name a hosted registry"), never silently landing on an upstream,
/// and an unclaimed name is rejected with the reason — so a typo'd scope
/// fails loudly at publish time instead of storing a name the registry's
/// namespace can never serve. The registry's `access` list gates the write
/// exactly as it gates reads — a caller the registry denies gets the same
/// not-found mask as on a read, whether the name is claimed or not
/// ([`registry_visible_to_caller`] gates the loud rejection), so a private
/// registry neither accepts the write nor reveals that it exists. The
/// path-less base routes through its default-target registry; with no default
/// target the bare host has no registry and the publish is a not-found,
/// exactly like a read.
fn resolve_publish_target(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    package: &str,
) -> PublishTarget {
    let (target, context) = match registry {
        Some(registry) => (registry.to_string(), format!("through registry {registry:?}")),
        None => match default_registry_target(state) {
            Some(target) => (target, "to the path-less base".to_string()),
            None => return PublishTarget::NotFound,
        },
    };
    match resolve_registry_source(state, &target, package) {
        RegistrySource::Hosted(registry) => {
            match hosted_gate(state, identity, &registry, package) {
                HostedGate::Allowed(org) => PublishTarget::Hosted { source: registry, org },
                HostedGate::MaskNotFound => PublishTarget::NotFound,
                HostedGate::Denied(err) => PublishTarget::Denied(Box::new(error_response(&err))),
            }
        }
        // A write can never land on an upstream — but the upstream's `access:`
        // gates the write endpoints exactly as it gates reads, so a caller the
        // upstream denies gets the read path's 403 (`authorized_upstream`), not
        // a rejection that narrates where the name routes.
        RegistrySource::Upstream(source) => match authorized_upstream(state, identity, &source) {
            Err(response) => PublishTarget::Denied(response),
            Ok(_) => PublishTarget::Reject(format!(
                "cannot publish {package:?} {context}: it routes to an upstream registry; name \
                 a hosted registry",
            )),
        },
        // The loud rejection explains a config fact about the addressed
        // registry, so only a caller the registry is visible to gets it;
        // anyone else keeps the same not-found mask a read gives, so an
        // off-pattern probe cannot distinguish a private registry from an
        // undefined one.
        RegistrySource::Unclaimed => {
            if registry_visible_to_caller(state, identity, &target) {
                PublishTarget::Reject(format!(
                    "cannot publish {package:?} {context}: no registry's declared `patterns:` \
                     claim this package name",
                ))
            } else {
                PublishTarget::NotFound
            }
        }
        RegistrySource::NotFound => PublishTarget::NotFound,
    }
}

/// Whether `identity` may learn that the registry `name` exists. A hosted
/// registry is masked behind its access list — a denied caller sees the same
/// not-found as for an undefined name on every read, so nothing on the write
/// path may answer differently. An upstream registry is not masked (a denied
/// caller gets an explicit 403 on reads), and a router is visible whenever
/// any of its sources is.
fn registry_visible_to_caller(state: &AppState, identity: &Identity, name: &str) -> bool {
    let concrete_visible = |name: &str| match state.inner.config.registries.get(name) {
        // The name being probed is unclaimed, so there is no per-package
        // entry to consult: the registry-level default `access:` decides
        // whether the caller may learn the registry exists at all.
        Some(Registry::Hosted { .. }) => state
            .inner
            .config
            .hosted
            .get(name)
            .is_some_and(|hosted| hosted.rules.default_access().allows(identity)),
        Some(Registry::Upstream { .. }) => true,
        Some(Registry::Router { .. }) | None => false,
    };
    match state.inner.config.registries.get(name) {
        Some(Registry::Router { sources }) => sources.iter().any(|source| concrete_visible(source)),
        Some(_) => concrete_visible(name),
        None => false,
    }
}

/// `PUT /:pkg` (path-less) or `PUT /~<name>/:pkg` — publish a new version (or
/// republish). Body is the full packument with `_attachments` carrying the
/// tarball bytes base64-encoded.
async fn publish_package(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    body: axum::body::Bytes,
) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };

    let incoming: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(err) => return error_response(&RegistryError::Json(err)),
    };

    // Reject a publish whose body name disagrees with the URL.
    // npm/verdaccio return 400 here too; without this check a
    // misrouted PUT silently overwrites the wrong on-disk
    // package.json with another package's manifest.
    let body_name = incoming.get("name").and_then(Value::as_str);
    if body_name.is_some_and(|body_name| body_name != name.as_str()) {
        return error_response(&RegistryError::BadRequest {
            reason: format!(
                "package in URL ({:?}) does not match body ({:?})",
                name.as_str(),
                body_name.unwrap_or(""),
            ),
        });
    }

    // Routing, masking, and the publish rule all run inside
    // `validate_publish_doc`: the write resolves to a hosted registry (or
    // fails closed), and that registry's `packages:` rules authorize it.
    let (validated, target) =
        match validate_publish_doc(state, identity, registry, name, incoming).await {
            Ok(validated) => validated,
            Err(response) => return *response,
        };

    // Serialize the read-merge-write against other writers of this same
    // package on this instance, so a concurrent publish can't read the
    // same `existing`, merge a different version, and overwrite ours.
    // Held until this function returns, past the packument write below.
    let _packument_guard = state.inner.package_locks.lock(validated.name.as_str()).await;

    let staged = match stage_publish(state, validated, &now_iso(), Some(&target.org)).await {
        Ok(staged) => staged,
        Err(err) => return error_response(&err),
    };
    if let Err(err) = commit_publishes(state, vec![staged]).await {
        return error_response(&err);
    }
    publish_created_response()
}

/// `PUT /-/pnpm/v1/publish` — publish several packages with one
/// request. The body is `{"packages": [<publish doc>, ...]}` where
/// each entry is exactly the JSON body that `PUT /:pkg` takes
/// (packument with `_attachments`). `pnpm publish --batch` sends
/// this; the endpoint is not part of the standard npm registry API.
///
/// The batch is all-or-nothing up to the commit point: every
/// document is validated (name, publish policy, attachment
/// integrity) and every tarball of every package is fully written
/// to a tmp slot before anything becomes visible to readers, so a
/// batch that fails validation or staging leaves no new versions
/// behind.
async fn serve_batch_publish(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    body: axum::body::Bytes,
) -> Response {
    let incoming: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(err) => return error_response(&RegistryError::Json(err)),
    };
    let Value::Object(mut incoming) = incoming else {
        return error_response(&RegistryError::BadRequest {
            reason: "body must be a JSON object".to_string(),
        });
    };
    let Some(Value::Array(docs)) = incoming.remove("packages") else {
        return error_response(&RegistryError::BadRequest {
            reason: "body must have a `packages` array".to_string(),
        });
    };
    if docs.is_empty() {
        return error_response(&RegistryError::BadRequest {
            reason: "`packages` must not be empty".to_string(),
        });
    }

    let mut validated = Vec::with_capacity(docs.len());
    let mut seen_names = std::collections::BTreeSet::new();
    for doc in docs {
        let Some(doc_name) = doc.get("name").and_then(Value::as_str) else {
            return error_response(&RegistryError::BadRequest {
                reason: "every entry in `packages` must have a string `name`".to_string(),
            });
        };
        let name = match PackageName::parse(doc_name) {
            Ok(name) => name,
            Err(err) => return error_response(&err),
        };
        // One packument read-merge-write per package: with the same
        // package twice in a batch, the second entry's merge would
        // depend on the first's uncommitted result. Senders carry
        // multiple versions of one package as several `versions`
        // entries in a single document instead.
        if !seen_names.insert(name.as_str().to_string()) {
            return error_response(&RegistryError::BadRequest {
                reason: format!("duplicate package {:?} in `packages`", name.as_str()),
            });
        }
        // The batch endpoint is path-less, so each package routes via the
        // default target; validation resolves that route and checks the
        // resolved hosted registry's publish rule per document.
        match validate_publish_doc(&state, &identity, None, name, doc).await {
            Ok(doc) => validated.push(doc),
            Err(response) => return *response,
        }
    }

    // Hold every affected package's lock across the whole
    // stage-and-commit, so concurrent writers of any package in the
    // batch serialize with us just like with a single publish.
    let names: Vec<&str> = validated.iter().map(|(doc, _)| doc.name.as_str()).collect();
    let _guards = state.inner.package_locks.lock_many(&names).await;

    let now = now_iso();
    let mut staged: Vec<StagedPublish> = Vec::with_capacity(validated.len());
    for (doc, target) in validated {
        // Each document's write target was resolved during validation, so a
        // routing failure surfaced before any tarball was staged.
        match stage_publish(&state, doc, &now, Some(&target.org)).await {
            Ok(stage) => staged.push(stage),
            Err(err) => {
                for stage in staged {
                    cleanup_tmp_slots(stage.slots).await;
                }
                return error_response(&err);
            }
        }
    }
    if let Err(err) = commit_publishes(&state, staged).await {
        return error_response(&err);
    }
    publish_created_response()
}

/// A publish document that passed every check that can run before
/// taking the package lock: the caller may publish the package, and
/// each attachment maps to a canonical disk filename and a
/// `versions[v].dist` block.
struct ValidatedPublish {
    name: PackageName,
    /// The publish body with `_attachments` stripped.
    incoming: Value,
    /// One entry per attachment.
    prepared: Vec<PreparedAttachment>,
}

/// One publish attachment resolved to its canonical on-disk filename and its
/// `versions[version].dist` block.
struct PreparedAttachment {
    attachment: PendingAttachment,
    /// Canonical on-disk filename.
    canonical: String,
    /// The version this attachment publishes, parsed from its filename.
    /// Lets the re-publish guard tell a content publish from a metadata-only
    /// update (which carries no attachments).
    version: String,
    /// The matching `dist` block, or `Value::Null` when absent.
    dist: Value,
}

async fn validate_publish_doc(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    name: PackageName,
    incoming: Value,
) -> Result<(ValidatedPublish, WriteTarget), Box<Response>> {
    // Route the write to its hosted registry first (masking a denied caller
    // as not-found, rejecting an upstream target), then check that
    // registry's `publish` rule for this package — so routing failures
    // surface before any 401/403 that would reveal a masked name exists.
    let target = resolve_write_target(state, identity, registry, &name)?;
    authorize(
        state,
        identity,
        &RegistrySource::Hosted(target.source.clone()),
        name.as_str(),
        Action::Publish,
    )
    .map_err(|err| Box::new(error_response(&err)))?;

    let (validated, target) = validate_publish_attachments(name, incoming, target)
        .map_err(|err| Box::new(error_response(&err)))?;
    Ok((validated, target))
}

/// The attachment half of [`validate_publish_doc`], split out so the
/// routing/authorization half above can use `?` on `Box<Response>` while
/// this half keeps plain [`RegistryError`]s.
fn validate_publish_attachments(
    name: PackageName,
    mut incoming: Value,
    target: WriteTarget,
) -> Result<(ValidatedPublish, WriteTarget), RegistryError> {
    let attachments = extract_attachments(&mut incoming)?;

    // Resolve each attachment's canonical disk filename + matching
    // `versions[v].dist` block. Attachment names that don't match the
    // package (`bar-1.0.0.tgz` for `foo`) or that try to escape the
    // package dir (`../../etc/passwd.tgz`) are rejected here, before
    // any I/O. The canonical name is what we actually persist — for
    // scoped libnpmpublish bodies the wire form is `@scope/name-version.tgz`
    // but on disk it lives at `<root>/@scope/name/name-version.tgz`,
    // matching what `serve_tarball` expects.
    let mut prepared: Vec<PreparedAttachment> = Vec::with_capacity(attachments.len());
    for attachment in attachments {
        let (canonical, version) = name.parse_tarball_name(&attachment.filename)?;
        let dist = incoming
            .get("versions")
            .and_then(|versions| versions.get(&version))
            .and_then(|manifest| manifest.get("dist"))
            .cloned()
            .unwrap_or(Value::Null);
        prepared.push(PreparedAttachment { attachment, canonical, version, dist });
    }
    Ok((ValidatedPublish { name, incoming, prepared }, target))
}

/// A publish whose packument is merged and whose tarballs are fully
/// written to tmp slots — everything verified, nothing visible to
/// readers yet. [`commit_publishes`] makes it visible.
struct StagedPublish {
    name: PackageName,
    merged_bytes: Vec<u8>,
    base_version: Option<HostedPackumentVersion>,
    slots: Vec<crate::storage::TarballSlot>,
    /// Hosted-org storage namespace this publish targets, or `None` for the
    /// flat (path-less) hosted store. Threaded into the commit and journal so
    /// the write — and any crash-recovery roll-forward — lands in the right org.
    org: Option<String>,
}

/// Merge the incoming packument with the on-disk / upstream state
/// and stream every tarball to a tmp slot. The caller must hold the
/// package lock for `doc.name` from before this call until after
/// [`commit_publishes`]. On error, every tmp file this call wrote is
/// removed.
async fn stage_publish(
    state: &AppState,
    doc: ValidatedPublish,
    now_iso: &str,
    org: Option<&str>,
) -> Result<StagedPublish, RegistryError> {
    let ValidatedPublish { name, incoming, prepared } = doc;
    let storage = hosted_storage(state, org);

    let hosted_packument = storage.read_hosted_packument_for_update(&name).await?;
    let (hosted_bytes, base_version) = match hosted_packument {
        Some(packument) => (Some(packument.bytes), Some(packument.version)),
        None => (None, None),
    };
    let hosted: Option<Value> = match hosted_bytes.as_deref().map(serde_json::from_slice) {
        Some(Ok(value)) => Some(value),
        Some(Err(err)) => return Err(RegistryError::Json(err)),
        None => None,
    };

    // Validate each incoming version against the locally hosted packument
    // (a hosted packument is served as-is, so anything not in it is genuinely
    // new here, even if it exists upstream):
    //
    // * Already hosted — published content is immutable, so reject a *content*
    //   re-publish with 409 (as npm/verdaccio do): one that carries a new
    //   tarball (an attachment) or changes `dist.integrity` (the content
    //   anchor; the `tarball` URL is rewritten on read, so don't compare it).
    //   A clash that does neither is a metadata-only update (`pnpm deprecate`),
    //   which is allowed — `merge_versions` keeps the hosted `dist`.
    // * New — it must ship a tarball. A version entry with no attachment would
    //   be advertised with no hosted tarball (installs 404) and would block a
    //   later real publish of it (409): reject with 400.
    let attachment_versions: HashSet<&str> =
        prepared.iter().map(|attachment| attachment.version.as_str()).collect();
    let hosted_versions =
        hosted.as_ref().and_then(|h| h.get("versions")).and_then(Value::as_object);
    if let Some(incoming_versions) = incoming.get("versions").and_then(Value::as_object) {
        for (version, incoming_manifest) in incoming_versions {
            let has_attachment = attachment_versions.contains(version.as_str());
            match hosted_versions.and_then(|hosted| hosted.get(version)) {
                Some(hosted_manifest) => {
                    let incoming_integrity =
                        incoming_manifest.pointer("/dist/integrity").and_then(Value::as_str);
                    let hosted_integrity =
                        hosted_manifest.pointer("/dist/integrity").and_then(Value::as_str);
                    let integrity_changed = incoming_integrity
                        .is_some_and(|integrity| Some(integrity) != hosted_integrity);
                    if has_attachment || integrity_changed {
                        return Err(RegistryError::VersionAlreadyPublished {
                            package: name.as_str().to_string(),
                            version: version.clone(),
                        });
                    }
                }
                None if !has_attachment => {
                    return Err(RegistryError::BadRequest {
                        reason: format!(
                            "cannot publish version {version} of {:?} without a tarball",
                            name.as_str(),
                        ),
                    });
                }
                None => {}
            }
        }
    }

    // A hosted registry has no upstream, so a publish seeds the merge only from
    // the org's own hosted packument; a brand-new package starts from `None`.
    let existing: Option<Value> = hosted.clone();
    let merged = merge_manifest(existing.as_ref(), &incoming, hosted.as_ref(), now_iso);
    let merged_bytes = serde_json::to_vec_pretty(&merged).map_err(RegistryError::Json)?;
    // `incoming` is no longer needed; drop it so the base64 strings
    // inside go away as soon as `prepared` (which owns each one) is
    // drained below.
    drop(incoming);

    // Stream-decode + verify + write each tarball. A mismatch — or a
    // missing integrity field — short-circuits the publish with a
    // 400; any tmp files written before the failure get removed
    // along the way so a bad upload leaves no on-disk artifact.
    let mut written_slots = Vec::with_capacity(prepared.len());
    for PreparedAttachment { attachment, canonical, version: _, dist } in prepared {
        let slot = match storage.reserve_hosted_tarball(&name, &canonical).await {
            Ok(slot) => slot,
            Err(err) => {
                cleanup_tmp_slots(written_slots).await;
                return Err(err);
            }
        };
        let PendingAttachment { filename, data, declared_length } = attachment;
        let tmp_path = slot.tmp_path.clone();
        let dist_for_task = (!dist.is_null()).then_some(dist);
        let result = tokio::task::spawn_blocking(move || {
            let dist_ref = dist_for_task.as_ref();
            stream_decode_verify_and_write(&filename, &data, declared_length, dist_ref, &tmp_path)
        })
        .await;
        match result {
            Ok(Ok(_)) => written_slots.push(slot),
            Ok(Err(err)) => {
                cleanup_tmp_slots(written_slots).await;
                return Err(err);
            }
            Err(join_err) => {
                let _ = tokio::fs::remove_file(&slot.tmp_path).await;
                cleanup_tmp_slots(written_slots).await;
                return Err(RegistryError::Io(std::io::Error::other(join_err.to_string())));
            }
        }
    }
    Ok(StagedPublish {
        name,
        merged_bytes,
        base_version,
        slots: written_slots,
        org: org.map(str::to_string),
    })
}

/// Make every staged publish visible. The full intent — merged
/// packument bytes plus the staged tmp-file locations — is sealed into
/// the commit journal first, so a crash or I/O failure mid-apply can
/// never leave the batch partially published: startup recovery rolls
/// a sealed transaction forward. If sealing itself fails, nothing was
/// promoted and the staged tmp files are cleaned up here.
///
/// Within each package, tarballs are promoted before the packument so
/// a successful packument write never advertises a tarball that's
/// missing from disk.
async fn commit_publishes(
    state: &AppState,
    staged: Vec<StagedPublish>,
) -> Result<(), RegistryError> {
    let journal = state.inner.storage.publish_journal();
    let entries: Vec<JournaledPublish<'_>> = staged
        .iter()
        .map(|stage| JournaledPublish {
            name: &stage.name,
            org: stage.org.as_deref(),
            packument: &stage.merged_bytes,
            slots: &stage.slots,
        })
        .collect();
    let sealed = journal.seal(&entries).await;
    drop(entries);
    let txn = match sealed {
        Ok(txn) => txn,
        Err(err) => {
            for stage in staged {
                cleanup_tmp_slots(stage.slots).await;
            }
            return Err(err);
        }
    };
    // Past the seal the transaction is committed: the apply below is pure
    // roll-forward, and failures must NOT clean up the staged files. If
    // the apply fails partway, complete it immediately via the same
    // idempotent recovery path so a running server never leaves the batch
    // partially visible; startup recovery is the final backstop if even
    // that fails.
    let apply_result = async {
        for stage in staged {
            // Promote into the package's hosted namespace (or the flat
            // store when it has none) — the same target the journal recorded,
            // so an inline failure and a startup roll-forward land identically.
            let store = hosted_storage(state, stage.org.as_deref());
            for slot in stage.slots {
                store.finalize_tarball_slot(slot).await?;
            }
            match store
                .write_hosted_packument_if_current(
                    &stage.name,
                    &stage.merged_bytes,
                    stage.base_version.as_ref(),
                )
                .await?
            {
                PackumentWrite::Written => {}
                PackumentWrite::Conflict => {
                    return Err(RegistryError::PackumentWriteConflict {
                        package: stage.name.as_str().to_string(),
                    });
                }
            }
        }
        Ok::<(), RegistryError>(())
    }
    .await;
    match apply_result {
        Ok(()) => {
            txn.finish().await;
            Ok(())
        }
        Err(apply_err) => {
            tracing::warn!(error = %apply_err, "publish apply failed after seal; rolling forward");
            txn.roll_forward(&state.inner.storage).await.map_err(|_| apply_err)
        }
    }
}

fn publish_created_response() -> Response {
    let body = json!({ "ok": true, "success": true });
    let bytes = serde_json::to_vec(&body).expect("static-shape JSON serializes");
    Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .expect("static-shape response always builds")
}

/// Remove every tmp tarball file that a partially-completed publish
/// already wrote. Errors are swallowed: the caller is already
/// returning an error response, and a leftover `*.tmp.*` file is
/// harmless beyond a small amount of disk.
async fn cleanup_tmp_slots(slots: Vec<crate::storage::TarballSlot>) {
    for slot in slots {
        let _ = tokio::fs::remove_file(&slot.tmp_path).await;
    }
}

/// `GET /-/v1/search?text=...&size=...` — npm search v1 endpoint.
///
/// Local-only: scans the on-disk storage and matches package names
/// as a case-insensitive substring on `text`. Matches verdaccio's
/// default behavior. We deliberately do NOT proxy to upstream npm
/// even in proxy mode — the tests rely on the local-search semantics
/// (`releasing/commands/test/search.ts` asserts that a guaranteed-not
/// -to-exist query returns "No packages found", which an upstream
/// proxy can't deliver because npm's search is fuzzy and returns
/// dozens of unrelated matches for almost anything).
///
/// Results are served through the registry graph and gated exactly like the
/// packument and tarball GETs:
///
/// * Only the hosted registries the addressed registry serves are scanned (see
///   [`hosted_search_sources`]), each gated by its **registry access list** — a
///   caller a registry denies gets nothing from it, the same existence mask the
///   read paths apply. Without this, search would enumerate a private registry's
///   packages by name/version/description while the packument GET correctly
///   404s.
/// * Under a router, a name is kept only when the router actually **routes it
///   to the scanned source**, so a hosted package shadowed by an earlier route
///   is as invisible to search as it is to a packument GET.
/// * The **per-package access policy** drops any package the caller can't
///   read (e.g. anonymous + `@private/*` with the default rules).
///
/// `total` counts the returned (post-filter, size-capped) objects so clients
/// can't infer the existence of hidden packages from a mismatched total.
async fn serve_search(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    query_string: &str,
) -> Response {
    let result = |objects: Vec<Value>| {
        let total = objects.len();
        let body = json!({ "objects": objects, "total": total, "time": now_iso() });
        let bytes = serde_json::to_vec(&body).expect("search response serializes");
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(bytes))
            .expect("static-shape response always builds")
    };
    let Some(text) = crate::search::parse_query(query_string) else {
        return result(Vec::new());
    };
    let Some(registry) = registry.map(str::to_string).or_else(|| default_registry_target(state))
    else {
        return result(Vec::new());
    };
    let size = crate::search::parse_size(query_string, 20);
    let mut objects: Vec<Value> = Vec::new();
    for source in hosted_search_sources(state, &registry) {
        if objects.len() >= size {
            break;
        }
        let Some(hosted) = state.inner.config.hosted.get(&source) else {
            continue;
        };
        // Fast path: a caller no rule of this registry could ever admit
        // gets the empty result without a storage scan — the blanket mask
        // must not become an enumeration (or scan-timing) primitive.
        if !hosted.rules.any_access_admits(identity) {
            continue;
        }
        let org = hosted.org.clone();
        let storage = hosted_storage(state, Some(&org));
        // The caller was resolved once by the middleware; both filters run
        // synchronously against it inside the scan. Visibility is
        // per-package: each hit is gated by this hosted registry's effective
        // access for that name, so a per-package rule can open (or close) a
        // name regardless of the registry-level default.
        let keep = |name: &str| {
            matches!(
                resolve_registry_source(state, &registry, name),
                RegistrySource::Hosted(resolved) if resolved == source,
            ) && matches!(hosted_gate(state, identity, &source, name), HostedGate::Allowed(_))
        };
        match crate::search::run_local_search(&storage, &text, size - objects.len(), keep).await {
            Ok(mut entries) => objects.append(&mut entries),
            Err(err) => return error_response(&err),
        }
    }
    result(objects)
}

/// The hosted registries a search addressed to `registry` scans, in source order.
/// A hosted registry scans itself; a router scans each of its hosted sources; an
/// upstream registry scans nothing — search is local-only, never proxied (an
/// upstream is reached only through its own registry, by exact package name;
/// there is no cross-origin search merge).
fn hosted_search_sources(state: &AppState, registry: &str) -> Vec<String> {
    match state.inner.config.registries.get(registry) {
        Some(Registry::Hosted { .. }) => vec![registry.to_string()],
        Some(Registry::Router { sources }) => sources
            .iter()
            .filter(|source| {
                matches!(
                    state.inner.config.registries.get(source.as_str()),
                    Some(Registry::Hosted { .. }),
                )
            })
            .cloned()
            .collect(),
        Some(Registry::Upstream { .. }) | None => Vec::new(),
    }
}

/// `PUT /:pkg/-rev/:rev` (path-less) or `PUT /~<name>/:pkg/-rev/:rev` —
/// overwrite the on-disk packument with the client-supplied body. pnpm uses
/// this in the partial-unpublish flow: it fetches the packument, removes the
/// unpublished version from `versions` / `dist-tags`, then PUTs the result
/// back. We strip any `_attachments` so we don't persist base64 payloads
/// alongside the manifest, and run
/// [`enforce_published_version_immutability`] so the body can't tamper with
/// a published version's `dist` or smuggle in a new one — everything else in
/// the body is trusted verbatim, the same trust verdaccio extends.
async fn update_packument(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    body: &[u8],
) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };
    let target = match resolve_write_target(state, identity, registry, &name) {
        Ok(target) => target,
        Err(response) => return *response,
    };
    let source = RegistrySource::Hosted(target.source.clone());
    for action in [Action::Publish, Action::Unpublish] {
        if let Err(err) = authorize(state, identity, &source, name.as_str(), action) {
            return error_response(&err);
        }
    }
    let org = target.org;
    let storage = hosted_storage(state, Some(&org));
    let mut packument: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(err) => return error_response(&RegistryError::Json(err)),
    };
    // The write destination is the URL package name; a mismatched body name
    // would otherwise land under the URL package and persist an inconsistent
    // manifest.
    if let Some(body_name) = packument.get("name").and_then(Value::as_str)
        && body_name != name.as_str()
    {
        return error_response(&RegistryError::BadRequest {
            reason: format!(
                "packument name {body_name:?} does not match the URL package {:?}",
                name.as_str(),
            ),
        });
    }
    if let Some(obj) = packument.as_object_mut() {
        obj.remove("_attachments");
        obj.remove("_rev");
        obj.remove("_revisions");
    }
    // Serialize the write against this instance's other same-package
    // packument writers (publish / dist-tag), so the client-supplied
    // rewrite can't interleave with a concurrent merge.
    let _packument_guard = state.inner.package_locks.lock(name.as_str()).await;
    let hosted_packument = match storage.read_hosted_packument_for_update(&name).await {
        Ok(Some(packument)) => packument,
        Ok(None) => {
            return error_response(&RegistryError::BadRequest {
                reason: format!(
                    "cannot update {:?}: it has no published packument to unpublish from",
                    name.as_str(),
                ),
            });
        }
        Err(err) => return error_response(&err),
    };
    let hosted: Value = match serde_json::from_slice(&hosted_packument.bytes) {
        Ok(value) => value,
        Err(err) => return error_response(&RegistryError::Json(err)),
    };
    if let Some(err) = enforce_published_version_immutability(&hosted, &name, &mut packument) {
        return error_response(&err);
    }
    let bytes = match serde_json::to_vec_pretty(&packument) {
        Ok(b) => b,
        Err(err) => return error_response(&RegistryError::Json(err)),
    };
    match storage
        .write_hosted_packument_if_current(&name, &bytes, Some(&hosted_packument.version))
        .await
    {
        Ok(PackumentWrite::Written) => {}
        Ok(PackumentWrite::Conflict) => {
            return error_response(&RegistryError::PackumentWriteConflict {
                package: name.as_str().to_string(),
            });
        }
        Err(err) => return error_response(&err),
    }
    let body = json!({ "ok": true });
    let bytes = serde_json::to_vec(&body).expect("static-shape JSON serializes");
    Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .expect("static-shape response always builds")
}

/// Hold a published version's security-critical `dist` fields immutable across
/// the partial-unpublish `PUT`, which otherwise persists the body verbatim.
/// [`expected_tarball_dist`] resolves a tarball request to a version by
/// `dist.tarball` basename and verifies the bytes against that version's string
/// `dist.integrity`, so letting either drift — while the bytes on disk stay put —
/// breaks installs of that version (`EINTEGRITY`, or a 404/502 redirect).
///
/// For each version in the body, given a hosted packument: changing the
/// `dist.integrity` or `dist.tarball` basename of an already-published version is
/// rejected; omitting either is repaired from the hosted value (the round-trip
/// drops them on retained versions); and a version not already published is
/// rejected — this endpoint only removes versions, and an added entry could
/// collide a basename or seed a tarball-less one. A `PUT` to a package with no
/// hosted packument is rejected outright (nothing to unpublish, and the write
/// would seed versions that publish can never overwrite).
///
/// Returns the rejection, or `None` when the body is acceptable (after any
/// restores). Must hold the package lock so a concurrent publish can't race it.
fn enforce_published_version_immutability(
    hosted: &Value,
    name: &PackageName,
    incoming: &mut Value,
) -> Option<RegistryError> {
    // None (no versions to enforce) means "accept", not "error" here.
    let incoming_versions = incoming.get("versions").and_then(Value::as_object)?;
    let hosted_versions = hosted.get("versions").and_then(Value::as_object);
    // Fields to re-insert after the scan; deferred because the scan borrows
    // `incoming` and the restore mutates it.
    let mut restore: Vec<(String, &'static str, Value)> = Vec::new();
    for (version, manifest) in incoming_versions {
        let Some(existing) = hosted_versions.and_then(|versions| versions.get(version)) else {
            return Some(RegistryError::BadRequest {
                reason: format!(
                    "version {version:?} is not in the published package; this endpoint removes versions, it does not add them",
                ),
            });
        };
        // A present dist.integrity must be a string; a non-string would slip past
        // the string-only checks below.
        let incoming_integrity = match manifest.get("dist").and_then(|dist| dist.get("integrity")) {
            None => None,
            Some(Value::String(value)) => Some(value.as_str()),
            Some(_) => {
                return Some(RegistryError::BadRequest {
                    reason: format!("dist.integrity for version {version:?} must be a string"),
                });
            }
        };
        let existing_dist = existing.get("dist");
        let existing_integrity =
            existing_dist.and_then(|dist| dist.get("integrity")).and_then(Value::as_str);
        match (existing_integrity, incoming_integrity) {
            (Some(stored), Some(submitted)) if stored != submitted => {
                return Some(RegistryError::BadRequest {
                    reason: format!(
                        "dist.integrity for the published version {version:?} is immutable",
                    ),
                });
            }
            (Some(stored), None) => {
                if let Some(err) = require_object_dist(manifest, version) {
                    return Some(err);
                }
                restore.push((version.clone(), "integrity", Value::String(stored.to_string())));
            }
            _ => {}
        }
        // Compare basenames, not URLs: the round-trip carries the rewritten URL
        // (see [`rewrite_tarball_urls`]) while the hosted side keeps the original,
        // and [`served_tarball_basename`] applies the same version-derived
        // fallback so a basename-less stored URL is still pinned.
        let existing_tarball = existing_dist.and_then(|dist| dist.get("tarball"));
        if let Some(stored_basename) = served_tarball_basename(existing, name) {
            let incoming_basename = manifest
                .get("dist")
                .and_then(|dist| dist.get("tarball"))
                .and_then(Value::as_str)
                .and_then(tarball_basename);
            match incoming_basename {
                Some(submitted) if submitted != stored_basename => {
                    return Some(RegistryError::BadRequest {
                        reason: format!(
                            "dist.tarball for the published version {version:?} is immutable",
                        ),
                    });
                }
                Some(_) => {}
                None => {
                    if let Some(err) = require_object_dist(manifest, version) {
                        return Some(err);
                    }
                    let stored = existing_tarball.cloned().unwrap_or(Value::Null);
                    restore.push((version.clone(), "tarball", stored));
                }
            }
        }
    }
    for (version, key, value) in restore {
        if let Some(dist) = incoming
            .get_mut("versions")
            .and_then(|versions| versions.get_mut(&version))
            .and_then(|manifest| manifest.get_mut("dist"))
            .and_then(Value::as_object_mut)
        {
            dist.insert(key.to_string(), value);
        }
    }
    None
}

/// The tarball basename a version is actually served under, mirroring
/// [`rewrite_tarball_urls`]: the `dist.tarball` URL's own basename when it has
/// one, otherwise the version-derived canonical name the rewrite falls back to.
/// Returns `None` when the manifest carries no string `dist.tarball` to serve.
fn served_tarball_basename(manifest: &Value, pkg: &PackageName) -> Option<String> {
    let url = manifest.get("dist").and_then(|dist| dist.get("tarball")).and_then(Value::as_str)?;
    if let Some(basename) = tarball_basename(url) {
        return Some(basename.to_owned());
    }
    let version = manifest.get("version").and_then(Value::as_str)?;
    Some(pkg.tarball_name_for_version(version))
}

/// Reject a published version whose `dist` isn't an object: a restore needs an
/// object to write into, so otherwise it would no-op and persist the version
/// without the field — the stripping this guards against.
fn require_object_dist(manifest: &Value, version: &str) -> Option<RegistryError> {
    if manifest.get("dist").is_some_and(Value::is_object) {
        return None;
    }
    Some(RegistryError::BadRequest {
        reason: format!("dist for the published version {version:?} must be an object"),
    })
}

/// `DELETE /:pkg/-rev/:rev` (path-less) or `DELETE /~<name>/:pkg/-rev/:rev`
/// — remove the entire package directory, packument and all tarballs. Used
/// by `pnpm unpublish --force`.
async fn delete_package(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };
    let target = match resolve_write_target(state, identity, registry, &name) {
        Ok(target) => target,
        Err(response) => return *response,
    };
    if let Err(err) = authorize(
        state,
        identity,
        &RegistrySource::Hosted(target.source.clone()),
        name.as_str(),
        Action::Unpublish,
    ) {
        return error_response(&err);
    }
    let org = target.org;
    // Serialize against same-package publishers so a delete can't race a
    // stage-and-commit and remove the package mid-write.
    let _packument_guard = state.inner.package_locks.lock(name.as_str()).await;
    if let Err(err) = hosted_storage(state, Some(&org)).remove_package(&name).await {
        return error_response(&err);
    }
    let body = json!({ "ok": true });
    let bytes = serde_json::to_vec(&body).expect("static-shape JSON serializes");
    Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .expect("static-shape response always builds")
}

/// `DELETE /:pkg/-/:filename/-rev/:rev` — remove a single tarball
/// file from the package directory. The partial-unpublish flow calls
/// this after PUT'ing the modified packument back. Accept the
/// libnpmpublish-style scoped filename as well as the canonical one
/// by going through `canonicalize_tarball_name` first.
async fn delete_tarball(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    filename: &str,
) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };
    let canonical = match name.canonicalize_tarball_name(filename) {
        Ok(c) => c,
        Err(err) => return error_response(&err),
    };
    let target = match resolve_write_target(state, identity, registry, &name) {
        Ok(target) => target,
        Err(response) => return *response,
    };
    if let Err(err) = authorize(
        state,
        identity,
        &RegistrySource::Hosted(target.source.clone()),
        name.as_str(),
        Action::Unpublish,
    ) {
        return error_response(&err);
    }
    let org = target.org;
    // Serialize against same-package publishers so a delete can't race a
    // stage-and-commit and remove a tarball mid-write.
    let _packument_guard = state.inner.package_locks.lock(name.as_str()).await;
    if let Err(err) = hosted_storage(state, Some(&org)).remove_tarball(&name, &canonical).await {
        return error_response(&err);
    }
    let body = json!({ "ok": true });
    let bytes = serde_json::to_vec(&body).expect("static-shape JSON serializes");
    Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .expect("static-shape response always builds")
}

/// `GET /-/package/:pkg/dist-tags` (path-less) or
/// `GET /~<name>/-/package/:pkg/dist-tags` — return the packument's
/// `dist-tags` object.
async fn get_dist_tags(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };
    let bytes = match load_packument_for_read(state, identity, registry, &name).await {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return not_found(),
        Err(response) => return *response,
    };
    let packument: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(err) => return error_response(&RegistryError::Json(err)),
    };
    let mut tags = packument.get("dist-tags").cloned().unwrap_or_else(|| json!({}));
    filter_osv_vulnerable_dist_tags(&mut tags, &packument, &name, state.inner.osv_index.as_ref());
    let bytes = serde_json::to_vec(&tags).expect("dist-tags object serializes");
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .expect("static-shape response always builds")
}

/// `PUT /-/package/:pkg/dist-tags/:tag` (path-less) or
/// `PUT /~<name>/-/package/:pkg/dist-tags/:tag` — set a dist-tag. Body is
/// a JSON-encoded version string (e.g. `"1.0.0"`).
async fn set_dist_tag(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    tag: &str,
    body: &[u8],
) -> Response {
    update_dist_tag(state, identity, registry, raw_name, tag, |tags| {
        let version: String = match serde_json::from_slice(body) {
            Ok(s) => s,
            Err(err) => return Err(RegistryError::Json(err)),
        };
        tags.insert(tag.to_string(), Value::String(version));
        Ok(())
    })
    .await
}

async fn remove_dist_tag(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    tag: &str,
) -> Response {
    update_dist_tag(state, identity, registry, raw_name, tag, |tags| {
        tags.remove(tag);
        Ok(())
    })
    .await
}

/// Shared "read packument, mutate dist-tags, write back" helper for
/// add/remove. Returns 201 on success — verdaccio uses 201 for both
/// add and remove and the anonymous-npm-registry-client tolerates
/// 200 or 201, so we standardize on 201.
async fn update_dist_tag<Mutate>(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    tag: &str,
    mutate: Mutate,
) -> Response
where
    Mutate: Fn(&mut serde_json::Map<String, Value>) -> Result<(), RegistryError>,
{
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };
    // A dist-tag change is a write, so it routes to a hosted namespace like
    // a publish — a name routed to an upstream is rejected — and the
    // resolved registry's `publish` rule gates it.
    let target = match resolve_write_target(state, identity, registry, &name) {
        Ok(target) => target,
        Err(response) => return *response,
    };
    if let Err(err) = authorize(
        state,
        identity,
        &RegistrySource::Hosted(target.source.clone()),
        name.as_str(),
        Action::Publish,
    ) {
        return error_response(&err);
    }
    let org = target.org;
    let storage = hosted_storage(state, Some(&org));

    // Serialize the read-modify-write against other same-package writers
    // on this instance (held until this function returns).
    let _packument_guard = state.inner.package_locks.lock(name.as_str()).await;

    let mut written = false;
    for _ in 0..PACKUMENT_WRITE_RETRIES {
        let hosted_packument = match storage.read_hosted_packument_for_update(&name).await {
            Ok(Some(packument)) => packument,
            Ok(None) => return not_found(),
            Err(err) => return error_response(&err),
        };
        let mut packument: Value = match serde_json::from_slice(&hosted_packument.bytes) {
            Ok(value) => value,
            Err(err) => return error_response(&RegistryError::Json(err)),
        };

        let Some(packument_obj) = packument.as_object_mut() else {
            return error_response(&RegistryError::BadRequest {
                reason: "stored packument is not an object".to_string(),
            });
        };
        let tags_entry = packument_obj
            .entry("dist-tags".to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        let Some(tags) = tags_entry.as_object_mut() else {
            return error_response(&RegistryError::BadRequest {
                reason: "stored dist-tags is not an object".to_string(),
            });
        };
        if let Err(err) = mutate(tags) {
            return error_response(&err);
        }
        let _ = tag;
        // Refresh `time.modified` so clients do not lag behind a
        // dist-tag change when deciding packument freshness.
        let time_entry = packument_obj
            .entry("time".to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        let Some(time_obj) = time_entry.as_object_mut() else {
            return error_response(&RegistryError::BadRequest {
                reason: "stored time is not an object".to_string(),
            });
        };
        time_obj.insert("modified".to_string(), Value::String(now_iso()));
        let new_bytes = match serde_json::to_vec_pretty(&packument) {
            Ok(b) => b,
            Err(err) => return error_response(&RegistryError::Json(err)),
        };
        match storage
            .write_hosted_packument_if_current(&name, &new_bytes, Some(&hosted_packument.version))
            .await
        {
            Ok(PackumentWrite::Written) => {
                written = true;
                break;
            }
            Ok(PackumentWrite::Conflict) => continue,
            Err(err) => return error_response(&err),
        }
    }
    if !written {
        return error_response(&RegistryError::PackumentWriteConflict {
            package: name.as_str().to_string(),
        });
    }
    let body = json!({ "ok": true });
    let bytes = serde_json::to_vec(&body).expect("static-shape JSON serializes");
    Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .expect("static-shape response always builds")
}

// --------------------------------------------------------------------
// npm team API — read-only views over the config-declared `teams:` maps.
// Team membership is part of the registry configuration (it feeds the
// compiled access lists), so the API serves listings and rejects
// mutations with an explicit "config-managed" error.
// --------------------------------------------------------------------

/// The hosted registry whose teams `@{scope}` addresses: the scope routes
/// through the addressed registry (an explicit `/~<name>/`, or the
/// path-less default) exactly as a package read in that scope would, then
/// the registry-level default `access` gates the caller. A denial is
/// masked as not-found — team and member names must not become an
/// existence probe for a private registry.
fn team_registry<'a>(
    state: &'a AppState,
    identity: &Identity,
    registry: Option<&str>,
    scope: &str,
) -> Result<&'a HostedConfig, Box<Response>> {
    let scope = scope.strip_prefix('@').unwrap_or(scope);
    if scope.is_empty() {
        return Err(Box::new(not_found()));
    }
    let target = match registry {
        Some(registry) => registry.to_string(),
        None => match default_registry_target(state) {
            Some(target) => target,
            None => return Err(Box::new(not_found())),
        },
    };
    let probe = format!("@{scope}/-");
    let RegistrySource::Hosted(source) = resolve_registry_source(state, &target, &probe) else {
        return Err(Box::new(not_found()));
    };
    let Some(hosted) = state.inner.config.hosted.get(&source) else {
        return Err(Box::new(not_found()));
    };
    if !hosted.rules.default_access().allows(identity) {
        return Err(Box::new(not_found()));
    }
    Ok(hosted)
}

/// `GET /-/org/{scope}/team` (path-less) or `GET /~<name>/-/org/{scope}/team`
/// — list the teams of the hosted registry that claims `@{scope}`, in the
/// shape the pnpm team command consumes: an array of `{"name": ...}`.
fn get_org_teams(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    scope: &str,
) -> Response {
    let hosted = match team_registry(state, identity, registry, scope) {
        Ok(hosted) => hosted,
        Err(response) => return *response,
    };
    let teams: Vec<Value> = hosted.teams.keys().map(|name| json!({ "name": name })).collect();
    (StatusCode::OK, axum::Json(Value::Array(teams))).into_response()
}

/// `GET /-/team/{scope}/{team}/user` (path-less) or
/// `GET /~<name>/-/team/{scope}/{team}/user` — list a team's members, in
/// the shape the pnpm team command consumes: an array of `{"name": ...}`.
fn get_team_members(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    scope: &str,
    team: &str,
) -> Response {
    let hosted = match team_registry(state, identity, registry, scope) {
        Ok(hosted) => hosted,
        Err(response) => return *response,
    };
    let Some(members) = hosted.teams.get(team) else {
        return not_found();
    };
    let members: Vec<Value> = members.iter().map(|name| json!({ "name": name })).collect();
    (StatusCode::OK, axum::Json(Value::Array(members))).into_response()
}

/// Every team mutation — create (`PUT /-/org/{scope}/team`), destroy
/// (`DELETE /-/team/{scope}/{team}`), member add/remove
/// (`PUT`/`DELETE /-/team/{scope}/{team}/user`) — answers 403: pnpr teams
/// are declared in the registry config. The same gate as the reads runs
/// first, so a caller who may not see the registry keeps the not-found
/// mask.
fn reject_team_mutation(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    scope: &str,
    action: &'static str,
) -> Response {
    if let Err(response) = team_registry(state, identity, registry, scope) {
        return *response;
    }
    error_response(&RegistryError::TeamsConfigManaged { action })
}

// --------------------------------------------------------------------
// Helpers.
// --------------------------------------------------------------------

/// Resolve the hosted storage namespace a non-publish write (dist-tag,
/// unpublish, packument update) targets, or the [`Response`] to return. A
/// write routes like a publish: through the addressed `/~<name>/` (or,
/// path-less, the default-target registry) to a hosted org, rejecting a name
/// routed to an upstream and 404ing when the path-less base has no default
/// target or the registry's access list denies the caller.
fn resolve_write_target(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    name: &PackageName,
) -> Result<WriteTarget, Box<Response>> {
    match resolve_publish_target(state, identity, registry, name.as_str()) {
        PublishTarget::Hosted { source, org } => Ok(WriteTarget { source, org }),
        PublishTarget::Reject(reason) => {
            Err(Box::new(error_response(&RegistryError::BadRequest { reason })))
        }
        PublishTarget::Denied(response) => Err(response),
        PublishTarget::NotFound => Err(Box::new(not_found())),
    }
}

/// The hosted registry a write resolved to: its name (for the
/// `publish`/`unpublish` rule lookup) and its storage namespace.
struct WriteTarget {
    source: String,
    org: String,
}

/// What the caller is trying to do with a package. Drives which
/// rule from the access policy applies.
#[derive(Debug, Clone, Copy)]
enum Action {
    Access,
    Publish,
    Unpublish,
}

impl Action {
    fn label(self) -> &'static str {
        match self {
            Action::Access => "access",
            Action::Publish => "publish",
            Action::Unpublish => "unpublish",
        }
    }
}

/// The caller resolved once by the [`authenticate`] middleware and stored
/// in request extensions. Every registry handler that needs to know who is
/// calling reads it back through this extractor rather than re-inspecting
/// the `Authorization` header — so a request hits the auth backend exactly
/// once, and the identity a handler sees is the same one the restriction
/// gate already approved (no second lookup, no policy/identity race).
#[derive(Clone)]
struct AuthedCaller(Identity);

impl<RouterState: Send + Sync> FromRequestParts<RouterState> for AuthedCaller {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &RouterState,
    ) -> Result<Self, Self::Rejection> {
        // The middleware runs on every route, so the context is always
        // present; a miss means a wiring bug, surfaced as a 5xx.
        parts.extensions.get::<AuthedCaller>().cloned().ok_or_else(|| {
            error_response(&RegistryError::Internal {
                reason: "authentication middleware did not run".to_string(),
            })
        })
    }
}

/// Authenticate every request once, up front, and stash the resolved
/// [`Identity`] in request extensions for the handlers (via
/// [`AuthedCaller`]).
///
/// This is also where bearer-token restrictions are enforced — ahead of
/// every route handler, so a restricted token is rejected before a write
/// handler buffers its (up to 100 MiB) request body. npm bearer tokens can
/// be marked read-only or pinned to a set of CIDR ranges; pnpr persists
/// both and surfaces them on `npm token list`, so it must enforce them too
/// — otherwise a token the operator restricted could still publish, or be
/// used from any network. Basic-auth and anonymous requests carry no
/// restriction and are still subject to the per-package access policy in
/// the handlers; an unknown or revoked bearer token resolves to anonymous.
async fn authenticate(State(state): State<AppState>, mut request: Request, next: Next) -> Response {
    // Copy what resolution needs out of the request before mutating its
    // extensions below — the header and method borrows can't outlive the
    // `extensions_mut` call.
    let header = match single_authorization_header(request.headers()) {
        Ok(header) => header.map(str::to_owned),
        Err(err) => return error_response(&err),
    };
    let method = request.method().clone();
    let peer = request.extensions().get::<ConnectInfo<PeerAddr>>().map(|info| info.0.0);

    let identity = match resolve_caller(&state, header.as_deref(), &method, peer).await {
        Ok(identity) => identity,
        Err(err) => return error_response(&err),
    };
    request.extensions_mut().insert(AuthedCaller(identity));
    next.run(request).await
}

/// Resolve the `Authorization` header to an [`Identity`], hitting the auth
/// backend exactly once. A bearer token is looked up as a full record so
/// its read-only / CIDR restrictions can be enforced here (a violation is
/// a `Forbidden` error); an unknown bearer token, a non-`Bearer` scheme
/// (e.g. legacy `Basic`), and a missing header all resolve to
/// [`Identity::Anonymous`]. `Err` is a backing-store failure, surfaced as a
/// 5xx so an outage isn't mistaken for "not authenticated".
async fn resolve_caller(
    state: &AppState,
    header: Option<&str>,
    method: &Method,
    peer: Option<SocketAddr>,
) -> Result<Identity, RegistryError> {
    if let Some(raw_token) = header.and_then(bearer_credentials) {
        let Some(record) = state.inner.auth.tokens.lookup_record(raw_token).await? else {
            return Ok(Identity::Anonymous);
        };
        check_token_restrictions(&record, method, peer)?;
        return Ok(Identity::user(record.username));
    }
    // Anything that is not a bearer token — Basic, another scheme, or no
    // credentials — carries no request identity. Going through `identify`
    // here would re-run the bearer lookup and bypass the restriction checks
    // above, so resolve straight to anonymous.
    Ok(Identity::Anonymous)
}

/// Enforce a bearer token's own restrictions. A read-only token may not
/// drive a mutating request; a CIDR-pinned token may only be used from a
/// whitelisted peer (and is refused when the peer address is unavailable,
/// so the check fails closed).
fn check_token_restrictions(
    record: &TokenRecord,
    method: &Method,
    peer: Option<SocketAddr>,
) -> Result<(), RegistryError> {
    if record.readonly && is_write_method(method) {
        return Err(RegistryError::Forbidden {
            user: record.username.clone(),
            action: "write with",
            resource: "a read-only token".to_string(),
        });
    }
    if !record.cidr_whitelist.is_empty() {
        // The peer address comes from the accepted socket (`ConnectInfo`),
        // never a client-supplied forwarding header.
        let allowed = peer.is_some_and(|addr| cidr_whitelist_allows(&record.cidr_whitelist, addr));
        if !allowed {
            return Err(RegistryError::Forbidden {
                user: record.username.clone(),
                action: "use",
                resource: "this token from your network address".to_string(),
            });
        }
    }
    Ok(())
}

/// The `packages:` rules of the concrete registry a request resolved to.
/// Authorization is entirely registry-scoped — there is no global,
/// name-keyed ACL — so every check consults the one registry that serves
/// the package. The fallback (safe defaults: reads open, publishes need
/// auth, destructive writes denied) only fires for a programmatically
/// built config whose serving tables miss the graph entry.
fn source_rules<'a>(state: &'a AppState, source: &RegistrySource) -> &'a PackageRules {
    static SAFE_DEFAULTS: LazyLock<PackageRules> = LazyLock::new(PackageRules::default);
    match source {
        RegistrySource::Hosted(name) => {
            state.inner.config.hosted.get(name).map(|hosted| &hosted.rules)
        }
        RegistrySource::Upstream(name) => {
            state.inner.config.upstreams.get(name).map(|upstream| &upstream.rules)
        }
        RegistrySource::Unclaimed | RegistrySource::NotFound => None,
    }
    .unwrap_or(&SAFE_DEFAULTS)
}

/// Check an already-resolved `identity` against the resolved source
/// registry's per-package rule (the most specific `packages:` entry, its
/// omitted fields falling back to the registry defaults). Returns `Ok(())`
/// when the call is allowed; otherwise the appropriate `Unauthenticated` /
/// `Forbidden` error. The identity is resolved once by [`authenticate`], so
/// every handler — including the search endpoint that filters many packages —
/// authorizes synchronously against it.
fn authorize(
    state: &AppState,
    identity: &Identity,
    source: &RegistrySource,
    package: &str,
    action: Action,
) -> Result<(), RegistryError> {
    let effective = source_rules(state, source).for_package(package);
    let list = match action {
        Action::Access => effective.access,
        Action::Publish => effective.publish,
        Action::Unpublish => effective.unpublish,
    };
    if list.allows(identity) {
        return Ok(());
    }
    // Denied: an anonymous caller gets a chance to authenticate (401);
    // an authenticated caller simply isn't in the allowed set (403).
    match identity {
        Identity::Anonymous => {
            Err(RegistryError::Unauthenticated { resource: format!("package {package:?}") })
        }
        Identity::User { username, .. } => Err(RegistryError::Forbidden {
            user: username.clone(),
            action: action.label(),
            resource: format!("package {package:?}"),
        }),
    }
}

/// The raw credentials of an `Authorization: Bearer <token>` header, or
/// `None` for any other scheme. The scheme is matched case-insensitively,
/// matching [`identify`].
fn bearer_credentials(header_value: &str) -> Option<&str> {
    let (scheme, credentials) = header_value.trim().split_once(' ')?;
    scheme.eq_ignore_ascii_case("Bearer").then(|| credentials.trim())
}

/// Whether `method` mutates registry state. Every write surface (publish,
/// unpublish, dist-tag add/remove, adduser, logout, token revoke) is a
/// PUT or DELETE; reads and the resolver POSTs are not. A read-only token
/// is confined to the non-mutating methods.
fn is_write_method(method: &Method) -> bool {
    matches!(*method, Method::PUT | Method::DELETE | Method::PATCH)
}

/// Whether `peer` falls inside any range of a token's CIDR whitelist. An
/// IPv4-mapped IPv6 peer is normalized to its IPv4 form first, so a
/// dual-stack listener still matches plain IPv4 ranges.
fn cidr_whitelist_allows(whitelist: &[String], peer: SocketAddr) -> bool {
    let peer = canonical_ip(peer.ip());
    whitelist.iter().any(|entry| cidr_contains(entry.trim(), peer))
}

fn canonical_ip(addr: IpAddr) -> IpAddr {
    match addr {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
        v4 @ IpAddr::V4(_) => v4,
    }
}

/// Whether `peer` is inside one `addr/prefix` (or bare `addr`) whitelist
/// entry. A bare address matches only itself; a malformed entry (bad
/// address, or a non-numeric / out-of-range prefix) matches nothing, so
/// the restriction fails closed rather than open.
fn cidr_contains(entry: &str, peer: IpAddr) -> bool {
    let (net, prefix) = match entry.split_once('/') {
        Some((net, prefix)) => (net.trim(), Some(prefix.trim())),
        None => (entry, None),
    };
    let Ok(net) = net.parse::<IpAddr>() else {
        return false;
    };
    match (net, peer) {
        (IpAddr::V4(net), IpAddr::V4(peer)) => {
            let Some(bits) = parse_prefix(prefix, 32) else {
                return false;
            };
            let mask = ipv4_mask(bits);
            (u32::from(net) & mask) == (u32::from(peer) & mask)
        }
        (IpAddr::V6(net), IpAddr::V6(peer)) => {
            let Some(bits) = parse_prefix(prefix, 128) else {
                return false;
            };
            let mask = ipv6_mask(bits);
            (u128::from(net) & mask) == (u128::from(peer) & mask)
        }
        // Different address families never match.
        _ => false,
    }
}

/// Parse a CIDR prefix length, defaulting to a full-width match (an exact
/// host) when the entry carried no `/prefix`. `None` for a non-numeric or
/// too-large value.
fn parse_prefix(prefix: Option<&str>, max_bits: u8) -> Option<u8> {
    match prefix {
        None => Some(max_bits),
        Some(prefix) => {
            let bits: u8 = prefix.parse().ok()?;
            (bits <= max_bits).then_some(bits)
        }
    }
}

fn ipv4_mask(prefix: u8) -> u32 {
    if prefix == 0 { 0 } else { u32::MAX << (32 - prefix) }
}

fn ipv6_mask(prefix: u8) -> u128 {
    if prefix == 0 { 0 } else { u128::MAX << (128 - prefix) }
}

/// True when the client's `Accept` header offers the
/// `application/vnd.npm.install-v1+json` abbreviated MIME. We do a
/// substring match rather than full RFC-7231 q-value parsing — the
/// npm client always sends it as the top-priority option and a
/// substring presence is a reliable signal.
fn wants_abbreviated(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|accept| accept.contains(ABBREVIATED_CONTENT_TYPE))
}

/// Parse the on-disk packument, rewrite `dist.tarball` URLs, and
/// build the response. When `abbreviated` is true, strip down to
/// the npm spec's install-v1 field set (mirrors verdaccio's
/// `convertAbbreviatedManifest`) and tag the response with the
/// `application/vnd.npm.install-v1+json` content type. Parse
/// failures surface as 502 via `RegistryError::Json`.
fn packument_response(
    name: &PackageName,
    bytes: &[u8],
    tarball_base: &str,
    osv_index: Option<&Arc<crate::resolver::OsvIndex>>,
    abbreviated: bool,
) -> Result<Response, RegistryError> {
    let mut doc: Value = serde_json::from_slice(bytes)?;
    filter_osv_vulnerable_versions(&mut doc, name, osv_index);
    rewrite_tarball_urls(&mut doc, name, tarball_base);
    let (body, content_type) = if abbreviated {
        let trimmed = abbreviate_packument(&doc, Utc::now());
        (serde_json::to_vec(&trimmed)?, ABBREVIATED_CONTENT_TYPE)
    } else {
        (serde_json::to_vec(&doc)?, "application/json")
    };
    Ok(packument_bytes_response(body, content_type))
}

fn filter_osv_vulnerable_versions(
    packument: &mut Value,
    name: &PackageName,
    osv_index: Option<&Arc<crate::resolver::OsvIndex>>,
) {
    let Some(osv_index) = osv_index else { return };
    let package_name = name.as_str();
    let mut blocked_keys = HashSet::new();
    let mut retained_version_keys = HashSet::new();
    let has_time = packument.get("time").and_then(Value::as_object).is_some();
    if let Some(versions) = packument.get_mut("versions").and_then(Value::as_object_mut) {
        versions.retain(|key, manifest| {
            let manifest_version = manifest.get("version").and_then(Value::as_str);
            let key_is_vulnerable = osv_index.is_vulnerable(package_name, key);
            let manifest_is_vulnerable = manifest_version.is_some_and(|version| {
                version != key && osv_index.is_vulnerable(package_name, version)
            });
            if key_is_vulnerable || manifest_is_vulnerable {
                blocked_keys.insert(key.clone());
                false
            } else {
                if has_time {
                    retained_version_keys.insert(key.clone());
                }
                true
            }
        });
    }
    if let Some(tags) = packument.get_mut("dist-tags").and_then(Value::as_object_mut) {
        tags.retain(|_, version| {
            version.as_str().is_none_or(|version| {
                !blocked_keys.contains(version) && !osv_index.is_vulnerable(package_name, version)
            })
        });
    }
    if let Some(time) = packument.get_mut("time").and_then(Value::as_object_mut) {
        time.retain(|key, _| {
            !blocked_keys.contains(key)
                && (matches!(key.as_str(), "created" | "modified")
                    || retained_version_keys.contains(key)
                    || !osv_index.is_vulnerable(package_name, key))
        });
    }
}

fn filter_osv_vulnerable_dist_tags(
    tags: &mut Value,
    packument: &Value,
    name: &PackageName,
    osv_index: Option<&Arc<crate::resolver::OsvIndex>>,
) {
    let Some(osv_index) = osv_index else { return };
    let Some(tags) = tags.as_object_mut() else {
        return;
    };
    let package_name = name.as_str();
    tags.retain(|_, version| {
        version.as_str().is_none_or(|version| {
            !is_osv_vulnerable_packument_version(packument, package_name, version, osv_index)
        })
    });
}

fn is_osv_vulnerable_packument_version(
    packument: &Value,
    package_name: &str,
    version: &str,
    osv_index: &crate::resolver::OsvIndex,
) -> bool {
    if osv_index.is_vulnerable(package_name, version) {
        return true;
    }
    let manifest_version = packument
        .get("versions")
        .and_then(|versions| versions.get(version))
        .and_then(|manifest| manifest.get("version"))
        .and_then(Value::as_str);
    manifest_version.is_some_and(|manifest_version| {
        manifest_version != version && osv_index.is_vulnerable(package_name, manifest_version)
    })
}

fn resolve_version_or_tag<'a>(packument: &'a Value, version_or_tag: &'a str) -> &'a str {
    packument
        .get("dist-tags")
        .and_then(|tags| tags.get(version_or_tag))
        .and_then(Value::as_str)
        .unwrap_or(version_or_tag)
}

fn ensure_osv_allowed(
    state: &AppState,
    name: &PackageName,
    version: &str,
) -> Result<(), RegistryError> {
    let Some(osv_index) = state.inner.osv_index.as_ref() else {
        return Ok(());
    };
    let ids = osv_index.vulnerability_ids(name.as_str(), version);
    if ids.is_empty() {
        return Ok(());
    }
    Err(RegistryError::OsvVulnerability {
        package: name.as_str().to_string(),
        version: version.to_string(),
        advisories: crate::resolver::format_advisory_ids(&ids),
    })
}

fn packument_bytes_response(bytes: Vec<u8>, content_type: &'static str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(bytes))
        .expect("static-shape response always builds")
}

fn tarball_response(body: Body, content_length: Option<u64>) -> Response {
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream");
    if let Some(len) = content_length {
        builder = builder.header(header::CONTENT_LENGTH, len);
    }
    builder.body(body).expect("static-shape response always builds")
}

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "Not Found").into_response()
}

fn error_response(err: &RegistryError) -> Response {
    let status = err.status_code();
    let error_kind = err.log_kind();
    if status.is_server_error() {
        let err = err.log_message();
        tracing::error!(%err, %error_kind, %status, "request failed");
    } else if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        tracing::debug!(%err, %error_kind, %status, "request failed");
    } else {
        tracing::warn!(%err, %error_kind, %status, "request failed");
    }
    (status, err.public_message()).into_response()
}

async fn serve_ping(State(_state): State<AppState>) -> Response {
    (StatusCode::OK, axum::Json(serde_json::json!({}))).into_response()
}

/// `GET /-/pnpr` — capability handshake for the pnpr resolver
/// protocol. A plain npm registry has no such route and 404s, so a
/// client can fail fast against a misconfigured server. `versions`
/// lists the `/-/pnpr/vN/resolve` protocol versions this server speaks.
async fn serve_pnpr_handshake() -> Response {
    (StatusCode::OK, axum::Json(serde_json::json!({ "pnpr": { "versions": [0] } }))).into_response()
}

/// 404 stub mounted on the resolver paths when the resolver feature is
/// disabled. Registered so these specific paths return a clean
/// not-found — in particular `/-/pnpr`, whose 404 is how a client
/// detects "no resolver here" — rather than being shadowed by the
/// registry's catch-all param routes and proxied upstream.
async fn resolver_disabled() -> Response {
    StatusCode::NOT_FOUND.into_response()
}

async fn serve_resolve(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    body: axum::body::Bytes,
) -> Response {
    // The caller's identity drives both resolution and gateway access:
    // it selects which pnpr-managed upstream credentials and hosted
    // packages the resolve may use, and gates which cached resolutions
    // it may receive.
    let runtime = crate::resolver::Resolver::get_or_init(
        &state.inner.resolver,
        &state.inner.config,
        state.inner.osv_index.clone(),
    );
    crate::resolver::handle_resolve(runtime, identity, body).await
}

async fn serve_verify_lockfile(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    body: axum::body::Bytes,
) -> Response {
    let runtime = crate::resolver::Resolver::get_or_init(
        &state.inner.resolver,
        &state.inner.config,
        state.inner.osv_index.clone(),
    );
    crate::resolver::handle_verify_lockfile(runtime, identity, body).await
}
