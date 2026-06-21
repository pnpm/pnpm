use crate::{
    auth::{AuthState, TokenBackend, UpsertOutcome, UserBackend, identify},
    config::Config,
    error::RegistryError,
    journal::JournaledPublish,
    package_name::PackageName,
    policy::Identity,
    publish::{
        PendingAttachment, extract_attachments, iso_from_unix_millis, merge_manifest, now_iso,
        stream_decode_verify_and_write,
    },
    storage::{CachedPackument, Storage},
    streaming,
    upstream::{
        CacheValidators, FetchOutcome, FetchedPackument, PackumentFetch, Upstream,
        abbreviate_packument, extract_version_manifest, rewrite_tarball_urls,
    },
};
use axum::{
    Router,
    body::Body,
    extract::{DefaultBodyLimit, OriginalUri, Path, Request, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use chrono::Utc;
use indexmap::IndexMap;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
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

/// Cap publish bodies at 100 MiB. The default axum body limit is
/// 2 MiB, far too small for a real package — npm itself caps publish
/// at 100 MiB and verdaccio inherits that limit. We apply it via
/// [`DefaultBodyLimit::max`] on the router rather than on each
/// route, so future write endpoints inherit the same ceiling.
const MAX_PUBLISH_BODY_BYTES: usize = 100 * 1024 * 1024;

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
    /// Lazily-built engine backing the `/v1/resolve` endpoint. Built on
    /// first such request so servers that never receive one pay nothing.
    resolver: std::sync::OnceLock<crate::resolver::Resolver>,
    /// Local OSV index, loaded before the server accepts requests when
    /// `osv.enabled` is set.
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
    router_with_auth(config, AuthState::in_memory())
}

/// Fallible counterpart to [`router`]: surfaces a missing/invalid OSV
/// database (when `osv.enabled`) as an error instead of panicking, for
/// embedders that build the router directly rather than via [`serve`].
pub fn try_router(config: Config) -> crate::error::Result<Router> {
    try_router_with_auth(config, AuthState::in_memory())
}

/// Like [`router`] but with a caller-supplied [`AuthState`]. Used
/// by [`serve`] to wire the persistent file-backed stores, and by
/// tests that want to override the bcrypt cost or pre-seed users.
///
/// Panics if `osv.enabled` is set but the database can't load; call
/// [`try_router_with_auth`] to handle that as a recoverable error.
pub fn router_with_auth(config: Config, auth: AuthState) -> Router {
    try_router_with_auth(config, auth)
        .expect("enabled OSV database must load before building pnpr router")
}

/// Fallible counterpart to [`router_with_auth`].
pub fn try_router_with_auth(config: Config, auth: AuthState) -> crate::error::Result<Router> {
    let osv_index = crate::resolver::load_osv_index(&config)?;
    Ok(router_with_auth_and_osv(config, auth, osv_index))
}

fn router_with_auth_and_osv(
    config: Config,
    auth: AuthState,
    osv_index: Option<Arc<crate::resolver::OsvIndex>>,
) -> Router {
    let storage =
        Storage::new(&config.hosted_store, config.storage.clone(), config.cache_storage.clone());
    let upstreams: IndexMap<String, Upstream> = config
        .uplinks
        .iter()
        .map(|(name, uplink)| (name.clone(), Upstream::new(name, uplink)))
        .collect();
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
    Router::new()
        .route("/-/ping", get(serve_ping))
        // pnpr resolver: opt-in, versioned endpoints layered on the
        // registry core. Non-pnpm clients never touch these. `/-/pnpr`
        // is the capability handshake (404 on a plain registry).
        .route("/-/pnpr", get(serve_pnpr_handshake))
        .route("/v1/resolve", post(serve_resolve))
        .route("/v1/verify-lockfile", post(serve_verify_lockfile))
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
        .route("/{a}/{b}/{c}/{d}/{e}/{f}", delete(delete_six_segments))
        .layer(DefaultBodyLimit::max(MAX_PUBLISH_BODY_BYTES))
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

/// Bind to `config.listen` and serve forever. Loads the configured
/// htpasswd users and token database before binding the socket so
/// a startup-time auth error surfaces before we accept any client
/// connections.
pub async fn serve(config: Config) -> crate::error::Result<()> {
    crate::journal::recover_publish_journal(&config).await?;
    let osv_index = crate::resolver::load_osv_index(&config)?;
    let auth = AuthState::load(&config.auth, &config.backend).await?;
    let listen = config.listen;
    let app = router_with_auth_and_osv(config, auth, osv_index);
    let listener = NodelayTcpListener(tokio::net::TcpListener::bind(listen).await?);
    tracing::info!(%listen, "pnpr listening");
    axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await?;
    Ok(())
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
    crate::journal::recover_publish_journal(&config).await?;
    let osv_index = crate::resolver::load_osv_index(&config)?;
    // Load the configured auth backends here too — going through
    // `router` would silently fall back to in-memory auth and ignore a
    // persisted htpasswd / SQLite store or a configured `backend:`.
    let auth = AuthState::load(&config.auth, &config.backend).await?;
    let app = router_with_auth_and_osv(config, auth, osv_index);
    tracing::info!(%listen, "pnpr listening");
    axum::serve(NodelayTcpListener(listener), app)
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
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    serve_packument(&state, &headers, &name).await
}

async fn get_two_segments(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((first, second)): Path<(String, String)>,
) -> Response {
    if first == "-" && second == "whoami" {
        return private_no_cache(serve_whoami(&state, &headers).await);
    }
    if first.starts_with('@') {
        let full = format!("{first}/{second}");
        serve_packument(&state, &headers, &full).await
    } else {
        serve_version_manifest(&state, &headers, &first, &second).await
    }
}

async fn get_three_segments(
    State(state): State<AppState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    Path((first, second, third)): Path<(String, String, String)>,
) -> Response {
    if first == "-" && second == "v1" && third == "search" {
        let query = uri.query().unwrap_or("");
        return serve_search(&state, &headers, query).await;
    }
    if second == "-" {
        serve_tarball(&state, &headers, &first, &third).await
    } else if first.starts_with('@') {
        let full = format!("{first}/{second}");
        serve_version_manifest(&state, &headers, &full, &third).await
    } else {
        not_found()
    }
}

async fn get_tarball_scoped(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((scope, name, filename)): Path<(String, String, String)>,
) -> Response {
    if !scope.starts_with('@') {
        return not_found();
    }
    let full = format!("{scope}/{name}");
    serve_tarball(&state, &headers, &full, &filename).await
}

/// 4-segment GET:
/// * `/-/package/{pkg}/dist-tags` — packument's `dist-tags` object.
/// * `/-/npm/v1/user` — caller's profile (`npm profile get`).
/// * `/-/npm/v1/tokens` — list bearer tokens for the caller
///   (`npm token list`).
async fn get_four_segments(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((a, b, c, d)): Path<(String, String, String, String)>,
) -> Response {
    if a == "-" && b == "package" && d == "dist-tags" {
        return get_dist_tags(&state, &headers, &c).await;
    }
    if a == "-" && b == "npm" && c == "v1" && d == "user" {
        return private_no_cache(serve_profile(&state, &headers).await);
    }
    if a == "-" && b == "npm" && c == "v1" && d == "tokens" {
        return private_no_cache(list_tokens(&state, &headers).await);
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
    headers: HeaderMap,
    Path(name): Path<String>,
    body: axum::body::Bytes,
) -> Response {
    publish_package(&state, &headers, &name, body).await
}

/// `PUT /{first}/{second}` — publish a scoped package
/// (`/@scope/name`). The `/-/package/{pkg}` shape never lands here
/// because that's at least 4 segments.
async fn put_two_segments(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((first, second)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Response {
    if first.starts_with('@') {
        let full = format!("{first}/{second}");
        return publish_package(&state, &headers, &full, body).await;
    }
    not_found()
}

/// `PUT /-/user/org.couchdb.user:{name}` — adduser / login.
/// `PUT /{pkg}/-rev/{rev}` — packument update (partial unpublish).
async fn put_three_segments(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((first, second, third)): Path<(String, String, String)>,
    body: axum::body::Bytes,
) -> Response {
    if first == "-"
        && second == "user"
        && let Some(name) = third.strip_prefix("org.couchdb.user:")
    {
        return add_user(&state, name, &body).await;
    }
    if second == "-rev" {
        // `third` is the opaque revision token the client sent back.
        // We don't track revisions, so it's only used for routing —
        // the body is the full mutated packument.
        let _ = third;
        return update_packument(&state, &headers, &first, &body).await;
    }
    not_found()
}

/// `DELETE /{pkg}/-rev/{rev}` — remove the entire package
/// (`pnpm unpublish --force`). For scoped packages the URL is
/// `/@scope%2Fname/-rev/{rev}` and arrives as a single segment after
/// axum's percent-decoding.
async fn delete_three_segments(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((first, second, third)): Path<(String, String, String)>,
) -> Response {
    if second == "-rev" {
        let _ = third;
        return delete_package(&state, &headers, &first).await;
    }
    not_found()
}

/// `PUT /-/package/{pkg}/dist-tags/{tag}` — add/update a dist-tag.
async fn put_five_segments(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((a, b, c, d, e)): Path<(String, String, String, String, String)>,
    body: axum::body::Bytes,
) -> Response {
    if a == "-" && b == "package" && d == "dist-tags" {
        return set_dist_tag(&state, &headers, &c, &e, &body).await;
    }
    not_found()
}

/// `DELETE /-/user/token/{tok}` — npm logout. `{tok}` is the raw
/// bearer token sent verbatim. We hash it and remove the matching
/// row from the token store.
async fn delete_four_segments(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((a, b, c, d)): Path<(String, String, String, String)>,
) -> Response {
    if a == "-" && b == "user" && c == "token" {
        return private_no_cache(logout(&state, &headers, &d).await);
    }
    not_found()
}

/// 5-segment DELETE:
/// * `/-/package/{pkg}/dist-tags/{tag}` — remove a dist-tag.
/// * `/{pkg}/-/{filename}/-rev/{rev}` — remove an unscoped tarball
///   (one step of `pnpm unpublish <pkg>@<version>`).
async fn delete_five_segments(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((a, b, c, d, e)): Path<(String, String, String, String, String)>,
) -> Response {
    if a == "-" && b == "package" && d == "dist-tags" {
        return remove_dist_tag(&state, &headers, &c, &e).await;
    }
    if b == "-" && d == "-rev" {
        let _ = e; // revision token is unused
        return delete_tarball(&state, &headers, &a, &c).await;
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
    headers: HeaderMap,
    Path((a, b, c, d, e, f)): Path<(String, String, String, String, String, String)>,
) -> Response {
    if a == "-" && b == "npm" && c == "v1" && d == "tokens" && e == "token" {
        return private_no_cache(revoke_token_by_key(&state, &headers, &f).await);
    }
    if a.starts_with('@') && c == "-" && e == "-rev" {
        let _ = f; // revision token is unused
        let full = format!("{a}/{b}");
        return delete_tarball(&state, &headers, &full, &d).await;
    }
    not_found()
}

// --------------------------------------------------------------------
// Handler bodies.
// --------------------------------------------------------------------

async fn serve_packument(state: &AppState, headers: &HeaderMap, raw_name: &str) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };
    if let Err(err) = enforce_access(state, headers, name.as_str(), Action::Access).await {
        return error_response(&err);
    }
    match load_packument_bytes(state, &name).await {
        PackumentLoad::Ok(bytes) => {
            let abbreviated = wants_abbreviated(headers);
            match packument_response(&name, &bytes, &state.inner.config, abbreviated) {
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
    headers: &HeaderMap,
    raw_name: &str,
    version_or_tag: &str,
) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };
    if let Err(err) = enforce_access(state, headers, name.as_str(), Action::Access).await {
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
    headers: &HeaderMap,
    raw_name: &str,
    filename: &str,
) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };
    if let Err(err) = name.validate_tarball_name(filename) {
        return error_response(&err);
    }
    if let Err(err) = enforce_access(state, headers, name.as_str(), Action::Access).await {
        return error_response(&err);
    }

    match state.inner.storage.open_tarball(&name, filename).await {
        Ok(Some((body, len))) => return tarball_response(body, len),
        Ok(None) => {}
        Err(err) => {
            tracing::warn!(?err, package = %name.as_str(), %filename, "tarball cache open failed");
        }
    }

    let Some(upstream) = resolve_upstream(state, &name) else {
        return not_found();
    };

    let response = match upstream.fetch_tarball_response(&name, filename).await {
        Ok(FetchOutcome::Ok(response)) => response,
        Ok(FetchOutcome::NotFound) => return not_found(),
        Err(err) => return error_response(&err),
    };
    let upstream_len = response.content_length();

    // `cache: false` uplinks (verdaccio) are mirror-less: stream the
    // tarball straight to the client without writing it to disk.
    if !upstream.caches() {
        return tarball_response(Body::from_stream(response.bytes_stream()), upstream_len);
    }

    let write = match state.inner.storage.open_cached_tarball_tmp(&name, filename).await {
        Ok(w) => w,
        Err(err) => {
            tracing::warn!(?err, package = %name.as_str(), %filename, "tarball cache tmp-open failed; streaming without cache");
            let body = Body::from_stream(response.bytes_stream());
            return tarball_response(body, upstream_len);
        }
    };

    let body = streaming::tee_to_cache(response, write);
    tarball_response(body, upstream_len)
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
async fn serve_whoami(state: &AppState, headers: &HeaderMap) -> Response {
    let username = match require_caller(state, headers, "user identity").await {
        Ok(username) => username,
        Err(response) => return response,
    };
    json_response(StatusCode::OK, &json!({ "username": username }))
}

/// `GET /-/npm/v1/user` — return the profile of the authenticated
/// caller. `npm profile get` reads this. pnpr doesn't track email,
/// 2FA, or anything beyond the username; the absent fields surface
/// as their zero-value defaults so the npm CLI's table renderer
/// doesn't choke on a missing key.
async fn serve_profile(state: &AppState, headers: &HeaderMap) -> Response {
    let username = match require_caller(state, headers, "user profile").await {
        Ok(username) => username,
        Err(response) => return response,
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
async fn list_tokens(state: &AppState, headers: &HeaderMap) -> Response {
    let username = match require_caller(state, headers, "token list").await {
        Ok(username) => username,
        Err(response) => return response,
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
async fn revoke_token_by_key(state: &AppState, headers: &HeaderMap, key: &str) -> Response {
    let username = match require_caller(state, headers, "token revocation").await {
        Ok(username) => username,
        Err(response) => return response,
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
async fn logout(state: &AppState, headers: &HeaderMap, raw_token: &str) -> Response {
    let username = match require_caller(state, headers, "logout").await {
        Ok(username) => username,
        Err(response) => return response,
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

async fn caller_username(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<String>, RegistryError> {
    identify(
        headers.get(header::AUTHORIZATION).and_then(|value| value.to_str().ok()),
        state.inner.auth.users.as_ref(),
        state.inner.auth.tokens.as_ref(),
    )
    .await
}

/// Resolve the authenticated caller for an endpoint that requires one,
/// or return the ready-made response to send back: 401 when the request
/// is anonymous, or a 5xx when the auth backend itself failed (so an
/// outage isn't mistaken for "not logged in"). `resource` names what the
/// 401 is about.
async fn require_caller(
    state: &AppState,
    headers: &HeaderMap,
    resource: &str,
) -> Result<String, Response> {
    match caller_username(state, headers).await {
        Ok(Some(username)) => Ok(username),
        Ok(None) => {
            Err(error_response(&RegistryError::Unauthenticated { resource: resource.to_string() }))
        }
        Err(err) => Err(error_response(&err)),
    }
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
    headers: &HeaderMap,
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

    let validated = match validate_publish_doc(state, headers, name, incoming).await {
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
    headers: HeaderMap,
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
        match validate_publish_doc(&state, &headers, name, doc).await {
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
    headers: &HeaderMap,
    name: PackageName,
    mut incoming: Value,
) -> Result<ValidatedPublish, RegistryError> {
    enforce_access(state, headers, name.as_str(), Action::Publish).await?;

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
async fn serve_search(state: &AppState, headers: &HeaderMap, query_string: &str) -> Response {
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
        // The caller is the same across every result, so resolve the
        // identity once (the async backend hit) and authorize each
        // candidate synchronously inside the filter.
        let identity = match resolve_identity(state, headers).await {
            Ok(identity) => identity,
            Err(err) => return error_response(&err),
        };
        objects.retain(|entry| {
            let Some(name) =
                entry.get("package").and_then(|pkg| pkg.get("name")).and_then(Value::as_str)
            else {
                // Malformed entry — be conservative and drop it.
                return false;
            };
            authorize(state, &identity, name, Action::Access).is_ok()
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
    headers: &HeaderMap,
    raw_name: &str,
    body: &[u8],
) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };
    if let Err(err) = enforce_access(state, headers, name.as_str(), Action::Publish).await {
        return error_response(&err);
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
async fn delete_package(state: &AppState, headers: &HeaderMap, raw_name: &str) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };
    if let Err(err) = enforce_access(state, headers, name.as_str(), Action::Publish).await {
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
    headers: &HeaderMap,
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
    if let Err(err) = enforce_access(state, headers, name.as_str(), Action::Publish).await {
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
async fn get_dist_tags(state: &AppState, headers: &HeaderMap, raw_name: &str) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };
    if let Err(err) = enforce_access(state, headers, name.as_str(), Action::Access).await {
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
    let tags = packument.get("dist-tags").cloned().unwrap_or_else(|| json!({}));
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
    headers: &HeaderMap,
    raw_name: &str,
    tag: &str,
    body: &[u8],
) -> Response {
    update_dist_tag(state, headers, raw_name, tag, |tags| {
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
    headers: &HeaderMap,
    raw_name: &str,
    tag: &str,
) -> Response {
    update_dist_tag(state, headers, raw_name, tag, |tags| {
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
    headers: &HeaderMap,
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
    if let Err(err) = enforce_access(state, headers, name.as_str(), Action::Publish).await {
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
}

impl Action {
    fn label(self) -> &'static str {
        match self {
            Action::Access => "access",
            Action::Publish => "publish",
        }
    }
}

/// Resolve the caller behind a request by inspecting its
/// `Authorization` header against the auth backends. The backend
/// lookup is async (a networked record store hits the database here),
/// so this is the one async step the access checks fan out from.
async fn resolve_identity(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Identity, RegistryError> {
    let username = identify(
        headers.get(header::AUTHORIZATION).and_then(|value| value.to_str().ok()),
        state.inner.auth.users.as_ref(),
        state.inner.auth.tokens.as_ref(),
    )
    .await?;
    Ok(match username {
        Some(username) => Identity::User { username },
        None => Identity::Anonymous,
    })
}

/// Check an already-resolved `identity` against the per-package rule.
/// Returns `Ok(())` when the call is allowed; otherwise the
/// appropriate `Unauthenticated` / `Forbidden` error. Split from
/// [`resolve_identity`] so a caller that filters many packages (the
/// search endpoint) resolves the identity once and authorizes each
/// candidate synchronously.
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

/// Resolve the caller and check the per-package rule in one step.
async fn enforce_access(
    state: &AppState,
    headers: &HeaderMap,
    package: &str,
    action: Action,
) -> Result<(), RegistryError> {
    let identity = resolve_identity(state, headers).await?;
    authorize(state, &identity, package, action)
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
    abbreviated: bool,
) -> Result<Response, RegistryError> {
    let mut doc: Value = serde_json::from_slice(bytes)?;
    rewrite_tarball_urls(&mut doc, name, &config.public_url);
    let (body, content_type) = if abbreviated {
        let trimmed = abbreviate_packument(&doc, Utc::now());
        (serde_json::to_vec(&trimmed)?, ABBREVIATED_CONTENT_TYPE)
    } else {
        (serde_json::to_vec(&doc)?, "application/json")
    };
    Ok(packument_bytes_response(body, content_type))
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
/// lists the `/vN/resolve` protocol versions this server speaks.
async fn serve_pnpr_handshake() -> Response {
    (StatusCode::OK, axum::Json(serde_json::json!({ "pnpr": { "versions": [1] } }))).into_response()
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
