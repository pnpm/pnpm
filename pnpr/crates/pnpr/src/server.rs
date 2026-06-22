use crate::{
    auth::{AuthState, TokenRecord, UpsertOutcome, identify},
    config::Config,
    error::RegistryError,
    journal::JournaledPublish,
    package_name::PackageName,
    policy::Identity,
    publish::{
        PendingAttachment, extract_attachments, iso_from_unix_millis, merge_manifest, now_iso,
        stream_decode_verify_and_write,
    },
    storage::{CachedPackument, CachedTarballIntegrity, Storage},
    streaming,
    upstream::{
        CacheValidators, FetchOutcome, FetchedPackument, PackumentFetch, Upstream,
        abbreviate_packument, extract_version_manifest, rewrite_tarball_urls, tarball_basename,
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
    sync::Arc,
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

#[derive(Clone)]
struct AppState {
    inner: Arc<AppInner>,
}

struct AppInner {
    storage: Storage,
    /// One [`Upstream`] per declared uplink, keyed by the same name
    /// used in [`Config::uplinks`]. Built once at router construction
    /// time so each request avoids re-allocating a `ThrottledClient`.
    upstreams: IndexMap<String, Upstream>,
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
pub fn try_router_with_auth(config: Config, auth: AuthState) -> crate::error::Result<Router> {
    // Enforce the "at least one surface enabled" invariant for embedders
    // that build and serve the router themselves rather than going through
    // `serve`/`serve_listener`.
    config.ensure_a_feature_is_enabled()?;
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

/// Run startup side effects and load auth backends for surfaces that
/// consult caller identity. The registry needs publish-journal recovery;
/// both the registry and resolver need auth because resolver requests
/// control outbound dependency-resolution work.
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
    // Only the registry routes consult the uplinks, so a resolver-only
    // server builds none — skipping a `ThrottledClient` allocation per
    // configured uplink.
    let upstreams: IndexMap<String, Upstream> = if registry_enabled {
        config
            .uplinks
            .iter()
            .map(|(name, uplink)| (name.clone(), Upstream::new(name, uplink)))
            .collect()
    } else {
        IndexMap::new()
    };
    let state = AppState {
        inner: Arc::new(AppInner {
            storage,
            upstreams,
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
    // unpublish, dist-tag, search, and the user/login endpoint. When the
    // feature is disabled, none of these routes are mounted — not merely
    // hidden — so a resolver-only tier exposes no registry surface at all.
    if registry_enabled {
        router = router
            // Batch publish: one request carrying many packages' publish
            // documents. Not part of the standard npm registry API —
            // `pnpm publish --batch` opts into it explicitly.
            .route("/-/pnpm/v1/publish", put(serve_batch_publish))
            .route("/{name}", get(get_packument_unscoped).put(put_one_segment))
            .route("/{first}/{second}", get(get_two_segments).put(put_two_segments))
            .route(
                "/{first}/{second}/{third}",
                get(get_three_segments).put(put_three_segments).delete(delete_three_segments),
            )
            .route("/{scope}/{name}/-/{filename}", get(get_tarball_scoped))
            .route("/{a}/{b}/{c}/{d}", get(get_four_segments).delete(delete_four_segments))
            .route(
                "/{a}/{b}/{c}/{d}/{e}",
                get(get_five_segments).put(put_five_segments).delete(delete_five_segments),
            )
            // Scoped tarball delete: `DELETE /@scope/name/-/<basename-version>.tgz/-rev/<rev>`
            .route("/{a}/{b}/{c}/{d}/{e}/{f}", delete(delete_six_segments));
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
                        uri = %request.uri(),
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

/// Bind to `config.listen` and serve forever. Loads auth state before
/// binding so a startup-time auth error surfaces before we accept any
/// client connections. Registry startup additionally recovers the publish
/// journal.
pub async fn serve(config: Config) -> crate::error::Result<()> {
    // Enforce the "at least one surface" invariant here too, not only at
    // YAML load / CLI: embedders build `Config` programmatically and call
    // straight into `serve`, so a both-disabled config must fail loudly
    // rather than start a server that only answers `/-/ping`.
    config.ensure_a_feature_is_enabled()?;
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

/// Log which surfaces are mounted at startup. A misconfiguration — most
/// importantly a typo'd `registry:` / `resolver:` block name, which the
/// intentionally verdaccio-lenient config parser silently ignores and so
/// leaves the surface at its default-enabled state — is then immediately
/// visible to the operator rather than only discoverable by probing.
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
    config: Config,
    listener: tokio::net::TcpListener,
) -> crate::error::Result<()> {
    let listen = listener.local_addr()?;
    config.ensure_a_feature_is_enabled()?;
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
    if first == "-" && second == "whoami" {
        return private_no_cache(serve_whoami(&identity));
    }
    if first.starts_with('@') {
        let full = format!("{first}/{second}");
        serve_packument(&state, &identity, &headers, &full).await
    } else {
        serve_version_manifest(&state, &identity, &first, &second).await
    }
}

async fn get_three_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    OriginalUri(uri): OriginalUri,
    Path((first, second, third)): Path<(String, String, String)>,
) -> Response {
    if first == "-" && second == "v1" && third == "search" {
        let query = uri.query().unwrap_or("");
        return serve_search(&state, &identity, query).await;
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
    if !scope.starts_with('@') {
        return not_found();
    }
    let full = format!("{scope}/{name}");
    serve_tarball(&state, &identity, &full, &filename).await
}

/// 4-segment GET:
/// * `/-/package/{pkg}/dist-tags` — packument's `dist-tags` object.
/// * `/-/npm/v1/user` — caller's profile (`npm profile get`).
/// * `/-/npm/v1/tokens` — list bearer tokens for the caller
///   (`npm token list`).
async fn get_four_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((a, b, c, d)): Path<(String, String, String, String)>,
) -> Response {
    if a == "-" && b == "package" && d == "dist-tags" {
        return get_dist_tags(&state, &identity, &c).await;
    }
    if a == "-" && b == "npm" && c == "v1" && d == "user" {
        return private_no_cache(serve_profile(&identity));
    }
    if a == "-" && b == "npm" && c == "v1" && d == "tokens" {
        return private_no_cache(list_tokens(&state, &identity).await);
    }
    not_found()
}

/// 5-segment GET: rare for the npm spec — just here as a not-found
/// catchall so the route compiles and DELETE/PUT can sit on the
/// same path.
async fn get_five_segments(
    State(_state): State<AppState>,
    Path((_, _, _, _, _)): Path<(String, String, String, String, String)>,
) -> Response {
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
    publish_package(&state, &identity, &name, body).await
}

/// `PUT /{first}/{second}` — publish a scoped package
/// (`/@scope/name`). The `/-/package/{pkg}` shape never lands here
/// because that's at least 4 segments.
async fn put_two_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((first, second)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Response {
    if first.starts_with('@') {
        let full = format!("{first}/{second}");
        return publish_package(&state, &identity, &full, body).await;
    }
    not_found()
}

/// `PUT /-/user/org.couchdb.user:{name}` — adduser / login.
/// `PUT /{pkg}/-rev/{rev}` — packument update (partial unpublish).
async fn put_three_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((first, second, third)): Path<(String, String, String)>,
    body: axum::body::Bytes,
) -> Response {
    if first == "-"
        && second == "user"
        && let Some(name) = third.strip_prefix("org.couchdb.user:")
    {
        // adduser/login authenticates from the request body, not the
        // caller's existing identity.
        return add_user(&state, name, &body).await;
    }
    if second == "-rev" {
        // `third` is the opaque revision token the client sent back.
        // We don't track revisions, so it's only used for routing —
        // the body is the full mutated packument.
        let _ = third;
        return update_packument(&state, &identity, &first, &body).await;
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
        return delete_package(&state, &identity, &first).await;
    }
    not_found()
}

/// `PUT /-/package/{pkg}/dist-tags/{tag}` — add/update a dist-tag.
async fn put_five_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((a, b, c, d, e)): Path<(String, String, String, String, String)>,
    body: axum::body::Bytes,
) -> Response {
    if a == "-" && b == "package" && d == "dist-tags" {
        return set_dist_tag(&state, &identity, &c, &e, &body).await;
    }
    not_found()
}

/// `DELETE /-/user/token/{tok}` — npm logout. `{tok}` is the raw
/// bearer token sent verbatim. We hash it and remove the matching
/// row from the token store.
async fn delete_four_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((a, b, c, d)): Path<(String, String, String, String)>,
) -> Response {
    if a == "-" && b == "user" && c == "token" {
        return private_no_cache(logout(&state, &identity, &d).await);
    }
    not_found()
}

/// 5-segment DELETE:
/// * `/-/package/{pkg}/dist-tags/{tag}` — remove a dist-tag.
/// * `/{pkg}/-/{filename}/-rev/{rev}` — remove an unscoped tarball
///   (one step of `pnpm unpublish <pkg>@<version>`).
async fn delete_five_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((a, b, c, d, e)): Path<(String, String, String, String, String)>,
) -> Response {
    if a == "-" && b == "package" && d == "dist-tags" {
        return remove_dist_tag(&state, &identity, &c, &e).await;
    }
    if b == "-" && d == "-rev" {
        let _ = e; // revision token is unused
        return delete_tarball(&state, &identity, &a, &c).await;
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
/// * `/-/npm/v1/tokens/token/{key}` — revoke a bearer token by its
///   listing-side `key` (`npm token revoke`).
async fn delete_six_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((a, b, c, d, e, f)): Path<(String, String, String, String, String, String)>,
) -> Response {
    if a == "-" && b == "npm" && c == "v1" && d == "tokens" && e == "token" {
        return private_no_cache(revoke_token_by_key(&state, &identity, &f).await);
    }
    if a.starts_with('@') && c == "-" && e == "-rev" {
        let _ = f; // revision token is unused
        let full = format!("{a}/{b}");
        return delete_tarball(&state, &identity, &full, &d).await;
    }
    not_found()
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
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };
    if let Err(err) = authorize(state, identity, name.as_str(), Action::Access) {
        return error_response(&err);
    }
    match load_packument_bytes(state, &name).await {
        PackumentLoad::Ok(bytes) => {
            let abbreviated = wants_abbreviated(headers);
            match packument_response(
                &name,
                &bytes,
                &state.inner.config,
                state.inner.osv_index.as_ref(),
                abbreviated,
            ) {
                Ok(response) => response,
                Err(err) => error_response(&err),
            }
        }
        PackumentLoad::NotFound => not_found(),
        PackumentLoad::Err(err) => error_response(&err),
    }
}

async fn serve_version_manifest(
    state: &AppState,
    identity: &Identity,
    raw_name: &str,
    version_or_tag: &str,
) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };
    if let Err(err) = authorize(state, identity, name.as_str(), Action::Access) {
        return error_response(&err);
    }
    let bytes = match load_packument_bytes(state, &name).await {
        PackumentLoad::Ok(bytes) => bytes,
        PackumentLoad::NotFound => return not_found(),
        PackumentLoad::Err(err) => return error_response(&err),
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
    let Some(manifest) =
        extract_version_manifest(&packument, &name, version_or_tag, &state.inner.config.public_url)
    else {
        return not_found();
    };
    match serde_json::to_vec(&manifest) {
        Ok(body) => packument_bytes_response(body, "application/json"),
        Err(err) => error_response(&RegistryError::Json(err)),
    }
}

async fn serve_tarball(
    state: &AppState,
    identity: &Identity,
    raw_name: &str,
    filename: &str,
) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };
    // `name_version` is the version segment carried by the filename. It is
    // canonical for hosted tarballs (the publish handler enforces it) and a
    // best-effort screen here; the authoritative version a proxied tarball
    // resolves to is the `version` matched below, which may differ for a
    // non-canonical upstream tarball name.
    let (filename, name_version) = match name.parse_tarball_name(filename) {
        Ok(parsed) => parsed,
        Err(err) => return error_response(&err),
    };
    if let Err(err) = authorize(state, identity, name.as_str(), Action::Access) {
        return error_response(&err);
    }
    if let Err(err) = ensure_osv_allowed(state, &name, &name_version) {
        return error_response(&err);
    }

    // The hosted store is authoritative. A genuine fault here (not a
    // plain miss, which surfaces as `Ok(None)`) must fail closed rather
    // than fall through to upstream and serve bytes of a different
    // provenance for the same package name.
    match state.inner.storage.open_hosted_tarball(&name, &filename).await {
        Ok(Some((body, len))) => return tarball_response(body, len),
        Ok(None) => {}
        Err(err) => {
            tracing::warn!(?err, package = %name.as_str(), %filename, "hosted tarball open failed");
            return error_response(&err);
        }
    }

    let packument = match load_packument_bytes(state, &name).await {
        PackumentLoad::Ok(bytes) => bytes,
        PackumentLoad::NotFound => return not_found(),
        PackumentLoad::Err(err) => return error_response(&err),
    };
    let TarballDist { version, integrity } =
        match expected_tarball_dist(&packument, &name, &filename) {
            Ok(Some(dist)) => dist,
            Ok(None) => return not_found(),
            Err(err) => return error_response(&err),
        };
    // Re-screen when the resolved version differs from the filename's: a
    // non-canonical tarball name slips past the screen above, so this is
    // where OSV sees the version such a tarball really belongs to.
    if version != name_version
        && let Err(err) = ensure_osv_allowed(state, &name, &version)
    {
        return error_response(&err);
    }

    let upstream = resolve_upstream(state, &name);
    let should_read_cache = upstream.as_ref().is_none_or(|upstream| upstream.caches());
    if should_read_cache {
        match state.inner.storage.open_cached_tarball(&name, &filename).await {
            Ok(Some((file, len))) => {
                let expected = cached_tarball_integrity(&integrity, len);
                if state
                    .inner
                    .storage
                    .read_cached_tarball_integrity(&name, &filename)
                    .await
                    .is_some_and(|cached| cached == expected)
                {
                    return tarball_response(streaming::stream_file(file), Some(len));
                }
                match streaming::verify_file(file, &integrity).await {
                    Ok(file) => {
                        record_cached_tarball_integrity(state, &name, &filename, expected).await;
                        return tarball_response(streaming::stream_file(file), Some(len));
                    }
                    Err(err) => {
                        let err = tarball_stream_error(err, &name, &filename);
                        tracing::warn!(?err, package = %name.as_str(), %filename, "cached tarball failed verification");
                        discard_cached_tarball(state, &name, &filename).await;
                    }
                }
            }
            Ok(None) => {}
            Err(err) => {
                tracing::warn!(?err, package = %name.as_str(), %filename, "tarball cache open failed");
            }
        }
    }

    let Some(upstream) = upstream else {
        return not_found();
    };

    let response = match upstream.fetch_tarball_response(&name, &filename).await {
        Ok(FetchOutcome::Ok(response)) => response,
        Ok(FetchOutcome::NotFound) => return not_found(),
        Err(err) => return error_response(&err),
    };

    let write = match state.inner.storage.open_cached_tarball_tmp(&name, &filename).await {
        Ok(w) => w,
        Err(err) => return error_response(&err),
    };

    if upstream.caches() {
        let len = match streaming::download_verified_to_cache(
            response,
            write,
            &integrity,
            MAX_TARBALL_BYTES,
        )
        .await
        {
            Ok(len) => len,
            Err(err) => return error_response(&tarball_stream_error(err, &name, &filename)),
        };
        record_cached_tarball_integrity(
            state,
            &name,
            &filename,
            cached_tarball_integrity(&integrity, len),
        )
        .await;

        match state.inner.storage.open_cached_tarball(&name, &filename).await {
            Ok(Some((file, len))) => tarball_response(streaming::stream_file(file), Some(len)),
            Ok(None) => error_response(&tarball_integrity_error(
                &name,
                &filename,
                "verified cache entry disappeared before it could be served".to_string(),
            )),
            Err(err) => error_response(&err),
        }
    } else {
        match streaming::download_verified_to_temp(response, write, &integrity, MAX_TARBALL_BYTES)
            .await
        {
            Ok((file, len, tmp_path)) => {
                tarball_response(streaming::stream_file_and_remove(file, tmp_path), Some(len))
            }
            Err(err) => error_response(&tarball_stream_error(err, &name, &filename)),
        }
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

fn expected_tarball_dist(
    packument: &[u8],
    name: &PackageName,
    filename: &str,
) -> Result<Option<TarballDist>, RegistryError> {
    let packument: Value = serde_json::from_slice(packument)?;
    let Some(versions) = packument.get("versions").and_then(Value::as_object) else {
        return Ok(None);
    };
    let mut matches = versions.iter().filter(|(_, manifest)| {
        manifest
            .get("dist")
            .and_then(|dist| dist.get("tarball"))
            .and_then(Value::as_str)
            .and_then(tarball_basename)
            .is_some_and(|basename| basename == filename)
    });
    let Some((version, manifest)) = matches.next() else {
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
    let declared = manifest
        .get("dist")
        .and_then(|dist| dist.get("integrity"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            tarball_integrity_error(
                name,
                filename,
                format!("packument has no dist.integrity for version {version:?}"),
            )
        })?;
    let integrity = streaming::parse_integrity(declared).map_err(|err| {
        tarball_integrity_error(name, filename, format!("malformed dist.integrity: {err}"))
    })?;
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

async fn discard_cached_tarball(state: &AppState, name: &PackageName, filename: &str) {
    if let Err(err) = state.inner.storage.remove_cached_tarball(name, filename).await {
        tracing::warn!(?err, package = %name.as_str(), %filename, "invalid tarball cache removal failed");
    }
}

fn cached_tarball_integrity(integrity: &Integrity, len: u64) -> CachedTarballIntegrity {
    CachedTarballIntegrity { integrity: integrity.to_string(), len }
}

async fn record_cached_tarball_integrity(
    state: &AppState,
    name: &PackageName,
    filename: &str,
    integrity: CachedTarballIntegrity,
) {
    if let Err(err) =
        state.inner.storage.write_cached_tarball_integrity(name, filename, &integrity).await
    {
        tracing::warn!(?err, package = %name.as_str(), %filename, "tarball cache integrity marker write failed");
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

#[cfg(test)]
mod tests;

/// Require that an endpoint's caller is authenticated, returning their
/// username or the 401 error to send back. The identity was already
/// resolved by the [`authenticate`] middleware (which is also where an
/// auth-backend outage surfaces as a 5xx), so this is a pure check.
/// `resource` names what the 401 is about.
fn require_caller(identity: &Identity, resource: &str) -> Result<String, RegistryError> {
    match identity {
        Identity::User { username } => Ok(username.clone()),
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
    identify(authorization, state.inner.auth.users.as_ref(), state.inner.auth.tokens.as_ref()).await
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

/// `PUT /:pkg` — publish a new version (or republish). Body is the
/// full packument with `_attachments` carrying the tarball bytes
/// base64-encoded.
async fn publish_package(
    state: &AppState,
    identity: &Identity,
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

    let validated = match validate_publish_doc(state, identity, name, incoming).await {
        Ok(validated) => validated,
        Err(err) => return error_response(&err),
    };

    // Serialize the read-merge-write against other writers of this same
    // package on this instance, so a concurrent publish can't read the
    // same `existing`, merge a different version, and overwrite ours.
    // Held until this function returns, past the packument write below.
    let _packument_guard = state.inner.package_locks.lock(validated.name.as_str()).await;

    let staged = match stage_publish(state, validated, &now_iso()).await {
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
        match validate_publish_doc(&state, &identity, name, doc).await {
            Ok(doc) => validated.push(doc),
            Err(err) => return error_response(&err),
        }
    }

    // Hold every affected package's lock across the whole
    // stage-and-commit, so concurrent writers of any package in the
    // batch serialize with us just like with a single publish.
    let names: Vec<&str> = validated.iter().map(|doc| doc.name.as_str()).collect();
    let _guards = state.inner.package_locks.lock_many(&names).await;

    let now = now_iso();
    let mut staged: Vec<StagedPublish> = Vec::with_capacity(validated.len());
    for doc in validated {
        match stage_publish(&state, doc, &now).await {
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
    /// One `(attachment, canonical disk filename, dist)` triple per
    /// attachment.
    prepared: Vec<(PendingAttachment, String, Value)>,
}

async fn validate_publish_doc(
    state: &AppState,
    identity: &Identity,
    name: PackageName,
    mut incoming: Value,
) -> Result<ValidatedPublish, RegistryError> {
    authorize(state, identity, name.as_str(), Action::Publish)?;

    let attachments = extract_attachments(&mut incoming)?;

    // Resolve each attachment's canonical disk filename + matching
    // `versions[v].dist` block. Attachment names that don't match the
    // package (`bar-1.0.0.tgz` for `foo`) or that try to escape the
    // package dir (`../../etc/passwd.tgz`) are rejected here, before
    // any I/O. The canonical name is what we actually persist — for
    // scoped libnpmpublish bodies the wire form is `@scope/name-version.tgz`
    // but on disk it lives at `<root>/@scope/name/name-version.tgz`,
    // matching what `serve_tarball` expects.
    let mut prepared: Vec<(PendingAttachment, String, Value)> =
        Vec::with_capacity(attachments.len());
    for attachment in attachments {
        let (canonical, version) = name.parse_tarball_name(&attachment.filename)?;
        let dist = incoming
            .get("versions")
            .and_then(|versions| versions.get(&version))
            .and_then(|manifest| manifest.get("dist"))
            .cloned()
            .unwrap_or(Value::Null);
        prepared.push((attachment, canonical, dist));
    }
    Ok(ValidatedPublish { name, incoming, prepared })
}

/// A publish whose packument is merged and whose tarballs are fully
/// written to tmp slots — everything verified, nothing visible to
/// readers yet. [`commit_publishes`] makes it visible.
struct StagedPublish {
    name: PackageName,
    merged_bytes: Vec<u8>,
    slots: Vec<crate::storage::TarballSlot>,
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
) -> Result<StagedPublish, RegistryError> {
    let ValidatedPublish { name, incoming, prepared } = doc;

    // Seed the merge from whatever the upstream knows about the
    // package, not just from a cold cache. Without this, a publish
    // of a brand-new version of an upstream-only package would
    // start from `None` and the newly-written local packument
    // would mask every upstream version + dist-tag on subsequent
    // reads. `update_dist_tag` already does the same fallback —
    // we just mirror it here.
    let existing_bytes = match state.inner.storage.read_hosted_packument(&name).await? {
        Some(bytes) => Some(bytes),
        None => match load_packument_bytes(state, &name).await {
            PackumentLoad::Ok(bytes) => Some(bytes),
            PackumentLoad::NotFound => None,
            PackumentLoad::Err(err) => return Err(err),
        },
    };
    let existing: Option<Value> = match existing_bytes.as_deref().map(serde_json::from_slice) {
        Some(Ok(v)) => Some(v),
        Some(Err(err)) => return Err(RegistryError::Json(err)),
        None => None,
    };
    let merged = merge_manifest(existing.as_ref(), &incoming, now_iso);
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
    for (attachment, canonical, dist) in prepared {
        let slot = match state.inner.storage.reserve_hosted_tarball(&name, &canonical).await {
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
    Ok(StagedPublish { name, merged_bytes, slots: written_slots })
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
            for slot in stage.slots {
                state.inner.storage.finalize_tarball_slot(slot).await?;
            }
            state.inner.storage.write_hosted_packument(&stage.name, &stage.merged_bytes).await?;
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
/// Results are filtered by the per-package access policy: a package
/// the caller can't read (e.g. anonymous + `@private/*` or
/// `@pnpm.e2e/needs-auth` with the default rules) is dropped from
/// `objects` before the response is built. Without this the search
/// endpoint would happily enumerate protected packages that the
/// packument and tarball GETs correctly hide behind 401.
async fn serve_search(state: &AppState, identity: &Identity, query_string: &str) -> Response {
    let Some(text) = crate::search::parse_query(query_string) else {
        let body = json!({ "objects": [], "total": 0, "time": now_iso() });
        let bytes = serde_json::to_vec(&body).expect("static-shape JSON serializes");
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(bytes))
            .expect("static-shape response always builds");
    };
    let size = crate::search::parse_size(query_string, 20);
    let mut body = match crate::search::run_local_search(&state.inner.storage, &text, size).await {
        Ok(body) => body,
        Err(err) => return error_response(&err),
    };

    // Augment with an upstream packument lookup for the exact query
    // name. Without this, freshly-prepared registry-mock storage
    // (which ships only scoped packages) returns nothing for queries
    // like `is-positive` until something else proxies that package
    // first. Verdaccio's search does an equivalent merge with
    // upstream results.
    augment_search_with_upstream(state, &text, &mut body).await;

    if let Some(objects) = body.get_mut("objects").and_then(Value::as_array_mut) {
        // The caller was resolved once by the middleware; authorize each
        // candidate synchronously against it inside the filter.
        objects.retain(|entry| {
            let Some(name) =
                entry.get("package").and_then(|pkg| pkg.get("name")).and_then(Value::as_str)
            else {
                // Malformed entry — be conservative and drop it.
                return false;
            };
            authorize(state, identity, name, Action::Access).is_ok()
        });
        let visible = objects.len();
        // Surface the post-filter count so clients can't infer the
        // existence of hidden packages from a mismatched `total`.
        body["total"] = json!(visible);
    }
    let bytes = serde_json::to_vec(&body).expect("search response serializes");
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .expect("static-shape response always builds")
}

/// Inject an exact-name upstream match into a local-search result.
///
/// Verdaccio's search proxies to its uplinks; npm's `/-/v1/search`
/// is too fuzzy to mirror directly (a guaranteed-not-to-exist query
/// returns 1.7M results), so instead we treat the query as a literal
/// package name. If it parses as one, isn't already in the local
/// results, and the upstream returns a real packument for it, we
/// prepend the resulting entry. The fetch also caches the packument
/// on disk, so subsequent searches find it without another upstream
/// hit.
async fn augment_search_with_upstream(state: &AppState, query: &str, body: &mut Value) {
    if state.inner.upstreams.is_empty() {
        return;
    }
    let Ok(name) = PackageName::parse(query) else {
        return;
    };
    let already_present = body.get("objects").and_then(Value::as_array).is_some_and(|objects| {
        objects.iter().any(|object| {
            object.get("package").and_then(|pkg| pkg.get("name")).and_then(Value::as_str)
                == Some(name.as_str())
        })
    });
    if already_present {
        return;
    }
    // `load_packument_bytes` fetches from upstream and writes the
    // result into the cache, so the next search picks it up locally
    // without another network round trip.
    let PackumentLoad::Ok(bytes) = load_packument_bytes(state, &name).await else {
        return;
    };
    let Ok(packument) = serde_json::from_slice::<Value>(&bytes) else {
        return;
    };
    let Some(entry) = crate::search::build_search_entry(name.as_str(), &packument) else {
        return;
    };
    if let Some(objects) = body.get_mut("objects").and_then(Value::as_array_mut) {
        objects.insert(0, entry);
        let new_total = objects.len();
        body["total"] = json!(new_total);
    }
}

/// `PUT /:pkg/-rev/:rev` — overwrite the on-disk packument with the
/// client-supplied body. pnpm uses this in the partial-unpublish
/// flow: it fetches the packument, removes the unpublished version
/// from `versions` / `dist-tags`, then PUTs the result back. We
/// trust the body verbatim — the same trust verdaccio extends — and
/// strip any `_attachments` so we don't persist base64 payloads
/// alongside the manifest.
async fn update_packument(
    state: &AppState,
    identity: &Identity,
    raw_name: &str,
    body: &[u8],
) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };
    for action in [Action::Publish, Action::Unpublish] {
        if let Err(err) = authorize(state, identity, name.as_str(), action) {
            return error_response(&err);
        }
    }
    let mut packument: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(err) => return error_response(&RegistryError::Json(err)),
    };
    if let Some(obj) = packument.as_object_mut() {
        obj.remove("_attachments");
        obj.remove("_rev");
        obj.remove("_revisions");
    }
    let bytes = match serde_json::to_vec_pretty(&packument) {
        Ok(b) => b,
        Err(err) => return error_response(&RegistryError::Json(err)),
    };
    // Serialize the write against this instance's other same-package
    // packument writers (publish / dist-tag), so the client-supplied
    // rewrite can't interleave with a concurrent merge.
    let _packument_guard = state.inner.package_locks.lock(name.as_str()).await;
    if let Err(err) = state.inner.storage.write_hosted_packument(&name, &bytes).await {
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

/// `DELETE /:pkg/-rev/:rev` — remove the entire package directory,
/// packument and all tarballs. Used by `pnpm unpublish --force`.
async fn delete_package(state: &AppState, identity: &Identity, raw_name: &str) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };
    if let Err(err) = authorize(state, identity, name.as_str(), Action::Unpublish) {
        return error_response(&err);
    }
    // Serialize against same-package publishers so a delete can't race a
    // stage-and-commit and remove the package mid-write.
    let _packument_guard = state.inner.package_locks.lock(name.as_str()).await;
    if let Err(err) = state.inner.storage.remove_package(&name).await {
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
    if let Err(err) = authorize(state, identity, name.as_str(), Action::Unpublish) {
        return error_response(&err);
    }
    // Serialize against same-package publishers so a delete can't race a
    // stage-and-commit and remove a tarball mid-write.
    let _packument_guard = state.inner.package_locks.lock(name.as_str()).await;
    if let Err(err) = state.inner.storage.remove_tarball(&name, &canonical).await {
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

/// `GET /-/package/:pkg/dist-tags` — return the packument's
/// `dist-tags` object.
async fn get_dist_tags(state: &AppState, identity: &Identity, raw_name: &str) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };
    if let Err(err) = authorize(state, identity, name.as_str(), Action::Access) {
        return error_response(&err);
    }
    let bytes = match load_packument_bytes(state, &name).await {
        PackumentLoad::Ok(bytes) => bytes,
        PackumentLoad::NotFound => return not_found(),
        PackumentLoad::Err(err) => return error_response(&err),
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

/// `PUT /-/package/:pkg/dist-tags/:tag` — set a dist-tag. Body is
/// a JSON-encoded version string (e.g. `"1.0.0"`).
async fn set_dist_tag(
    state: &AppState,
    identity: &Identity,
    raw_name: &str,
    tag: &str,
    body: &[u8],
) -> Response {
    update_dist_tag(state, identity, raw_name, tag, |tags| {
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
    raw_name: &str,
    tag: &str,
) -> Response {
    update_dist_tag(state, identity, raw_name, tag, |tags| {
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
    raw_name: &str,
    tag: &str,
    mutate: Mutate,
) -> Response
where
    Mutate: FnOnce(&mut serde_json::Map<String, Value>) -> Result<(), RegistryError>,
{
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };
    if let Err(err) = authorize(state, identity, name.as_str(), Action::Publish) {
        return error_response(&err);
    }

    // Serialize the read-modify-write against other same-package writers
    // on this instance (held until this function returns).
    let _packument_guard = state.inner.package_locks.lock(name.as_str()).await;

    // Start from the authoritative packument if we have one. A
    // dist-tag change is an authoritative override, so it is written
    // back to the hosted store (below) regardless of whether the
    // package originated locally or from upstream.
    let mut packument: Value = match state.inner.storage.read_hosted_packument(&name).await {
        Ok(Some(bytes)) => match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(err) => return error_response(&RegistryError::Json(err)),
        },
        Ok(None) => {
            // Nothing published yet — pull the current packument
            // (cache or upstream) so a first dist-tag change against
            // a proxied package starts from its real version list.
            match load_packument_bytes(state, &name).await {
                PackumentLoad::Ok(bytes) => match serde_json::from_slice(&bytes) {
                    Ok(v) => v,
                    Err(err) => return error_response(&RegistryError::Json(err)),
                },
                PackumentLoad::NotFound => return not_found(),
                PackumentLoad::Err(err) => return error_response(&err),
            }
        }
        Err(err) => return error_response(&err),
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
    let _ = tag; // tag name is used by the mutate closure
    // Refresh `time.modified` so clients that rely on it for
    // freshness (pacquet's pick_package, npm's abbreviated-packument
    // staleness check) don't see the post-mutation packument as
    // older than its dist-tag change.
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
    if let Err(err) = state.inner.storage.write_hosted_packument(&name, &new_bytes).await {
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

// --------------------------------------------------------------------
// Helpers.
// --------------------------------------------------------------------

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
/// a `Forbidden` error); an unknown bearer token, a wrong Basic password,
/// and a missing header all resolve to [`Identity::Anonymous`]. `Err` is a
/// backing-store failure, surfaced as a 5xx so an outage isn't mistaken
/// for "not authenticated".
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
        return Ok(Identity::User { username: record.username });
    }
    // Not a bearer token: Basic (or no credentials), which carries no
    // token-level restriction. `identify` does the decode + password
    // verification.
    let username =
        identify(header, state.inner.auth.users.as_ref(), state.inner.auth.tokens.as_ref()).await?;
    Ok(username.map_or(Identity::Anonymous, |username| Identity::User { username }))
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

/// Check an already-resolved `identity` against the per-package rule.
/// Returns `Ok(())` when the call is allowed; otherwise the appropriate
/// `Unauthenticated` / `Forbidden` error. The identity is resolved once by
/// [`authenticate`], so every handler — including the search endpoint that
/// filters many packages — authorizes synchronously against it.
fn authorize(
    state: &AppState,
    identity: &Identity,
    package: &str,
    action: Action,
) -> Result<(), RegistryError> {
    let effective = state.inner.config.policies.for_package(package);
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
        Identity::User { username } => Err(RegistryError::Forbidden {
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

/// Resolve which prebuilt [`Upstream`] should serve `package`, by
/// walking the verdaccio-style `packages` rules in declared order and
/// looking up the resolved uplink name in [`AppInner::upstreams`].
/// Returns `None` when no rule with a `proxy:` field matches the
/// package, leaving the request to fall through to a not-found.
fn resolve_upstream<'a>(state: &'a AppState, package: &PackageName) -> Option<&'a Upstream> {
    let (uplink_name, _) = state.inner.config.resolve_uplink(package.as_str())?;
    state.inner.upstreams.get(uplink_name)
}

/// Result of loading the packument for a package — either bytes (raw,
/// from cache or upstream), a definite not-found, or a real error.
enum PackumentLoad {
    Ok(Vec<u8>),
    NotFound,
    Err(RegistryError),
}

/// Pull the on-disk packument bytes, hitting the upstream and updating
/// the cache when configured. The same logic backs both the packument
/// and the version-manifest endpoints.
async fn load_packument_bytes(state: &AppState, name: &PackageName) -> PackumentLoad {
    // A hosted packument — published here or static-served — is
    // authoritative: serve it as-is and never overwrite it with an
    // upstream refresh, so hosted versions can't be masked or lost.
    match state.inner.storage.read_hosted_packument(name).await {
        Ok(Some(bytes)) => {
            record_cache_status("hosted");
            return PackumentLoad::Ok(bytes);
        }
        Ok(None) => {}
        Err(err) => {
            tracing::warn!(?err, package = %name.as_str(), "published packument read failed");
        }
    }

    let Some(upstream) = resolve_upstream(state, name) else {
        // Nothing published and no upstream to proxy. The only thing
        // left is a leftover cache entry (e.g. a `proxy:` rule was
        // removed after the package was mirrored).
        return match state.inner.storage.read_cached_packument(name).await {
            Ok(Some(bytes)) => {
                // Served regardless of age — there's no upstream left to
                // revalidate against — so this is not a fresh `hit`.
                record_cache_status("orphaned");
                PackumentLoad::Ok(bytes)
            }
            Ok(None) => PackumentLoad::NotFound,
            Err(err) => PackumentLoad::Err(err),
        };
    };

    // Freshness window for the proxy cache: a cached packument younger
    // than `ttl` is served straight from disk; older than `ttl` it's
    // "stale" and revalidated against the upstream below. Lower = newer
    // versions surface sooner but more upstream traffic; higher = the
    // reverse. The conditional GET on the stale path keeps a high `ttl`
    // cheap (a `304` refreshes the entry without re-downloading it).
    //
    // The uplink's per-uplink `maxage` (verdaccio) wins when set;
    // otherwise the global `packument_ttl` (the `--packument-ttl-secs`
    // flag) applies.
    let ttl = upstream.maxage().unwrap_or(state.inner.config.packument_ttl);
    // A fresh entry serves immediately (and moves its bytes out — a
    // packument can be multiple MB). A stale entry yields only its
    // validators; its body stays on disk until a `304`/error path below
    // actually needs it, so the common stale→`200` refresh never reads it.
    let validators = match state.inner.storage.read_cached_packument_entry(name, ttl).await {
        Ok(Some(CachedPackument::Fresh(bytes))) => {
            record_cache_status("hit");
            return PackumentLoad::Ok(bytes);
        }
        Ok(Some(CachedPackument::Stale(validators))) => validators,
        Ok(None) => CacheValidators::default(),
        Err(err) => {
            tracing::warn!(?err, package = %name.as_str(), "cache read failed");
            CacheValidators::default()
        }
    };

    // Revalidate conditionally when we hold a stale copy: the upstream
    // can answer `304` and save us re-downloading an unchanged packument.
    match upstream.fetch_packument(name, &validators).await {
        Ok(PackumentFetch::Modified(fetched)) => {
            store_fetched_packument(state, name, fetched).await
        }
        // `304` confirmed our stale copy is current: read it now (deferred
        // until here), re-write it to bump the cache mtime so it's fresh
        // again until the next TTL window, and serve it.
        Ok(PackumentFetch::NotModified) => {
            match state.inner.storage.read_cached_packument(name).await {
                Ok(Some(bytes)) => {
                    if let Err(err) =
                        state.inner.storage.write_cached_packument(name, &bytes, &validators).await
                    {
                        tracing::warn!(?err, package = %name.as_str(), "packument cache refresh failed");
                    }
                    record_cache_status("revalidated");
                    PackumentLoad::Ok(bytes)
                }
                // The body vanished between the freshness check and this read
                // (cache wiped concurrently). The upstream just confirmed the
                // package exists, so re-fetch it unconditionally and self-heal
                // rather than 404-ing a present package.
                Ok(None) => match upstream.fetch_packument(name, &CacheValidators::default()).await
                {
                    Ok(PackumentFetch::Modified(fetched)) => {
                        store_fetched_packument(state, name, fetched).await
                    }
                    Ok(_) => PackumentLoad::NotFound,
                    Err(err) => PackumentLoad::Err(err),
                },
                Err(err) => PackumentLoad::Err(err),
            }
        }
        Ok(PackumentFetch::NotFound) => PackumentLoad::NotFound,
        Err(err) => {
            tracing::warn!(?err, package = %name.as_str(), "upstream packument fetch failed");
            match state.inner.storage.read_cached_packument(name).await {
                Ok(Some(bytes)) => {
                    record_cache_status("stale");
                    PackumentLoad::Ok(bytes)
                }
                // No cache to fall back on: surface the upstream failure.
                Ok(None) => PackumentLoad::Err(err),
                // The cache itself is unreadable: surface that I/O error
                // rather than the upstream one — it's the more actionable
                // failure when both go wrong.
                Err(cache_err) => PackumentLoad::Err(cache_err),
            }
        }
    }
}

/// Persist a freshly fetched packument to the proxy cache and return it,
/// tagging the access record as a `miss`. A cache-write failure is logged
/// but not fatal — the fetched bytes are still served.
async fn store_fetched_packument(
    state: &AppState,
    name: &PackageName,
    fetched: FetchedPackument,
) -> PackumentLoad {
    if let Err(err) =
        state.inner.storage.write_cached_packument(name, &fetched.bytes, &fetched.validators).await
    {
        tracing::warn!(?err, package = %name.as_str(), "packument cache write failed");
    }
    record_cache_status("miss");
    PackumentLoad::Ok(fetched.bytes)
}

/// Tag the current `pnpr::access` request span with how a packument
/// request was served against the proxy cache, surfacing as a `cache=…`
/// field on that request's access-log record:
///
/// * `hit` — served from a fresh cache entry (within `packument_ttl`)
///   without contacting the upstream.
/// * `revalidated` — entry was stale; the upstream answered `304 Not
///   Modified`, so the cached body was reused.
/// * `miss` — fetched a fresh body from the upstream.
/// * `stale` — upstream was unreachable; a stale cached body was served
///   as a fallback.
/// * `orphaned` — a leftover mirror served with no upstream left to
///   revalidate against (its `proxy:` rule was removed after the package
///   was mirrored). Served regardless of age, so distinct from `hit`.
/// * `hosted` — served from the authoritative hosted store (a published
///   or static package), bypassing the proxy cache entirely.
///
/// A no-op when called outside a request span (e.g. unit tests), so the
/// field is simply absent on those records.
fn record_cache_status(status: &'static str) {
    Span::current().record("cache", status);
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
    config: &Config,
    osv_index: Option<&Arc<crate::resolver::OsvIndex>>,
    abbreviated: bool,
) -> Result<Response, RegistryError> {
    let mut doc: Value = serde_json::from_slice(bytes)?;
    filter_osv_vulnerable_versions(&mut doc, name, osv_index);
    rewrite_tarball_urls(&mut doc, name, &config.public_url);
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

async fn serve_resolve(State(state): State<AppState>, body: axum::body::Bytes) -> Response {
    // pnpr resolves but serves no file content, so there is no per-package
    // read gate here: the client fetches every tarball directly from the
    // registry with its own credentials, and resolution uses the client's
    // forwarded credentials for private packages.
    let runtime = crate::resolver::Resolver::get_or_init(
        &state.inner.resolver,
        &state.inner.config,
        state.inner.osv_index.clone(),
    );
    crate::resolver::handle_resolve(runtime, body).await
}

async fn serve_verify_lockfile(State(state): State<AppState>, body: axum::body::Bytes) -> Response {
    let runtime = crate::resolver::Resolver::get_or_init(
        &state.inner.resolver,
        &state.inner.config,
        state.inner.osv_index.clone(),
    );
    crate::resolver::handle_verify_lockfile(runtime, body).await
}
