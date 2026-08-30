mod authentication;
mod package_mutation;
mod publishing;
mod routing;
mod staged;

#[cfg(test)]
mod tests;

use self::{
    authentication::{Action, AuthedCaller, authenticate, authorize},
    package_mutation::{
        delete_package, delete_tarball, get_dist_tags, remove_dist_tag, set_dist_tag,
        update_packument,
    },
    publishing::{
        PublishTarget, commit_publishes, publish_package, resolve_publish_target,
        serve_batch_publish, stage_publish, validate_publish_doc,
    },
    routing::router_with_auth_and_osv,
};

use axum::{
    Router,
    body::Body,
    extract::{
        FromRequestParts, Path, RawPathParams, Request, State, connect_info::Connected,
        rejection::RawPathParamsRejection,
    },
    http::{HeaderMap, StatusCode, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
    serve::IncomingStream,
};
use chrono::Utc;
use indexmap::IndexMap;
use pnpm_crypto_hash::{integrity_addressed_tarball_integrity, integrity_addressed_tarball_path};
use pnpm_lockfile::TarballRevision;
use pnpr_auth::{AuthState, UpsertOutcome, identify};
use pnpr_config::{Config, HostedConfig};
use pnpr_error::RegistryError;
use pnpr_package_name::PackageName;
use pnpr_policy::Identity;
use pnpr_registry::{ConcreteKind, Registry, Resolved};
use pnpr_storage::{
    Storage,
    publish::{iso_from_unix_millis, now_iso},
    streaming,
};

use pnpr_upstream::{
    CacheValidators, FetchOutcome, PackumentFetch, Upstream, abbreviate_packument,
    extract_upstream_version_manifest, extract_version_manifest, rewrite_tarball_urls,
    rewrite_upstream_tarball_urls, tarball_basename,
};
use serde::Deserialize;
use serde_json::{Value, json};
use ssri::Integrity;
use std::{collections::HashSet, net::SocketAddr, sync::Arc, time::Duration};

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
/// [`axum::extract::DefaultBodyLimit::max`] on the router rather than on each
/// route, so future write endpoints inherit the same ceiling.
const MAX_PUBLISH_BODY_BYTES: usize = MAX_TARBALL_BYTES as usize;

/// Cap adduser/login bodies far below the publish ceiling. The body is a
/// small couchdb-user JSON document, and login is the one body-accepting
/// endpoint reachable anonymously on every tier — letting it inherit the
/// 100 MiB publish limit would hand unauthenticated callers a cheap
/// buffer-and-parse amplifier.
const MAX_LOGIN_BODY_BYTES: usize = 64 * 1024;

/// The `PoC` accepts blobs inline on artifact publication. Keep the buffered
/// request at the same ceiling as an npm package publish.
const MAX_ARTIFACT_PUBLISH_BODY_BYTES: usize = MAX_PUBLISH_BODY_BYTES;
const MAX_ARTIFACT_RESOLVE_BODY_BYTES: usize = 16 * 1024 * 1024;
const MAX_ARTIFACT_BLOB_BODY_BYTES: usize = 8 * 1024;
#[derive(Clone)]
struct AppState {
    inner: Arc<AppInner>,
}

struct AppInner {
    storage: Storage,
    artifacts: Option<pnpr_shared_artifacts::SharedArtifactStore>,
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
    /// lose each other's changes.
    package_locks: StripedLocks,
    /// Lazily-built engine backing the `/-/pnpr/v0/resolve` endpoint. Built on
    /// first such request so servers that never receive one pay nothing.
    resolver: std::sync::OnceLock<crate::resolver::Resolver>,
    /// Local OSV index, loaded before the server accepts requests when
    /// `osv.enabled` is set and a mounted surface consults it.
    osv_index: Option<Arc<pnpr_osv::OsvIndex>>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct HostedOriginalRef {
    package: String,
    version: String,
}

/// A fixed stripe set bounds lock memory while serializing writers for the
/// same logical resource. Hash collisions only reduce concurrency.
///
/// This guards concurrency **within one instance**. Across replicas
/// sharing one hosted store, the same race needs a conditional write
/// (S3 `If-Match` / `ETag`); that is the cross-replica half tracked in
/// [pnpm/pnpm#12199](https://github.com/pnpm/pnpm/issues/12199).
struct StripedLocks {
    stripes: Box<[tokio::sync::Mutex<()>]>,
}

impl StripedLocks {
    /// Number of stripes. 64 keeps false sharing between distinct resources
    /// rare while staying tiny in memory.
    const STRIPES: usize = 64;

    fn new() -> Self {
        let stripes = (0..Self::STRIPES).map(|_| tokio::sync::Mutex::new(())).collect();
        Self { stripes }
    }

    /// Lock the stripe owning `name`, held until the returned guard is dropped.
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

/// Fallible counterpart to [`router`]: surfaces an invalid config, an
/// unloadable OSV database (when `osv.enabled`), and hosted-store settings
/// that don't build a client as errors instead of panicking, for embedders
/// that build the router directly rather than via [`serve`].
pub fn try_router(config: Config) -> pnpr_error::Result<Router> {
    let max_users = config.auth.htpasswd.max_users;
    try_router_with_auth(config, AuthState::in_memory_with_max_users(max_users))
}

/// Like [`router`] but with a caller-supplied [`AuthState`]. Used
/// by [`serve`] to wire the persistent file-backed stores, and by
/// tests that want to override the bcrypt cost or pre-seed users.
///
/// Panics if the config is invalid, an enabled OSV database can't load, or
/// the hosted object store's settings don't build a client. Call
/// [`try_router_with_auth`] to handle these as recoverable errors.
pub fn router_with_auth(config: Config, auth: AuthState) -> Router {
    try_router_with_auth(config, auth)
        .expect("pnpr config must be valid, and any enabled OSV database and hosted-store client must build, before building the router")
}

/// Fallible counterpart to [`router_with_auth`].
pub fn try_router_with_auth(mut config: Config, auth: AuthState) -> pnpr_error::Result<Router> {
    // Enforce the "at least one surface enabled" invariant for embedders
    // that build and serve the router themselves rather than going through
    // `serve`/`serve_listener`.
    config.ensure_a_feature_is_enabled()?;
    config.ensure_valid_registry_graph()?;
    let osv_index = load_active_osv_index(&config)?;
    router_with_auth_and_osv(config, auth, osv_index)
}

/// Load the OSV index only for surfaces that consult it. An artifacts-only
/// tier skips the database because artifact requests do not use OSV data.
fn load_active_osv_index(config: &Config) -> pnpr_error::Result<Option<Arc<pnpr_osv::OsvIndex>>> {
    if config.resolver.enabled || config.registry.enabled {
        pnpr_osv::load_osv_index(config)
    } else {
        Ok(None)
    }
}

/// Run startup side effects and load the auth backends. The registry
/// needs publish-journal recovery; auth loads on every tier because the
/// account endpoints (which mint and manage tokens) are always served,
/// and every mounted surface consults caller identity.
async fn load_startup_auth(config: &Config) -> pnpr_error::Result<AuthState> {
    if config.registry.enabled {
        pnpr_storage::journal::recover_publish_journal(config).await?;
    }
    AuthState::load(&config.auth, &config.backend).await
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
pub async fn serve(mut config: Config) -> pnpr_error::Result<()> {
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
    // Build the router before taking the port: it can fail, and a failure
    // should not leave a bound socket behind or put a `pnpr listening` line
    // immediately above the error saying it is not.
    let app = router_with_auth_and_osv(config, auth, osv_index)?;
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
        artifacts = config.artifacts.enabled,
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
) -> pnpr_error::Result<()> {
    let listen = listener.local_addr()?;
    config.ensure_a_feature_is_enabled()?;
    config.ensure_valid_registry_graph()?;
    log_enabled_surfaces(&config);
    let osv_index = load_active_osv_index(&config)?;
    // Load the configured auth backends here too — going through `router`
    // would silently fall back to in-memory auth and ignore a persisted
    // htpasswd / SQLite store or a configured `backend:`.
    let auth = load_startup_auth(&config).await?;
    let app = router_with_auth_and_osv(config, auth, osv_index)?;
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
// Account routes — adduser/login, whoami, profile, token list and
// revocation, logout. Mounted on every tier (see the router construction
// in `router_with_auth_and_osv`). Each has a `/~<prefix>/`-addressed twin
// whose `/{prefix}/...` route pattern also matches a non-`~` first
// segment; that shape is not an account URL, so the handler 404s it —
// though route-level layers still run first (an oversized body to a
// non-`~` login path is the body cap's 413, not a 404).
// --------------------------------------------------------------------

/// The registry a `~<name>` path segment addresses, or `None` for a segment
/// that is not one. A bare `~` names no registry, so it reads as "not a
/// registry prefix" rather than as an empty name — every caller then treats
/// it the way it treats `foo`.
pub(super) fn tilde_registry(segment: &str) -> Option<&str> {
    segment.strip_prefix('~').filter(|registry| !registry.is_empty())
}

/// The registry a request addressed through a leading `/~<name>/`, or `None`
/// when it arrived on the path-less base.
///
/// Every route that answers under a registry prefix is registered twice — once
/// bare, once under `/{prefix}` — pointing at the same handler. This extractor
/// is what tells the two apart, so a handler states once that it is
/// prefix-aware instead of needing a near-identical twin.
///
/// A `{prefix}` segment that is present but is not a well-formed `~<name>`
/// rejects with 404: the prefixed registration exists only to serve that
/// shape, and falling through to the base behaviour would let any first
/// segment reach an endpoint the route never meant to expose.
pub(super) struct TargetRegistry(pub(super) Option<String>);

impl<RouterState: Send + Sync> FromRequestParts<RouterState> for TargetRegistry {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &RouterState,
    ) -> Result<Self, Self::Rejection> {
        // `RawPathParams` reports only what the matched route captured, so an
        // absent `prefix` means this is the bare registration rather than a
        // prefixed request that happened to omit the segment.
        let params = RawPathParams::from_request_parts(parts, &()).await.map_err(|err| {
            match err {
                // The client sent a segment that percent-decodes to invalid
                // UTF-8. It cannot be a well-formed `~<name>`, so answer it the
                // same 404 every other malformed prefix gets rather than a 500
                // — a bad URL is not a server fault, and rendering it as one
                // would also let a client fill the error log.
                RawPathParamsRejection::InvalidUtf8InPathParam(_) => RegistryError::NotFound,
                // The matched route registered no path parameters at all, which
                // means the route table and this extractor disagree. Fail closed
                // rather than serve the request as if it named no registry.
                rejection => RegistryError::Internal {
                    reason: format!("path params unavailable: {rejection}"),
                },
            }
            .into_response()
        })?;
        let Some((_, prefix)) = params.iter().find(|(name, _)| *name == "prefix") else {
            return Ok(Self(None));
        };
        tilde_registry(prefix)
            .map(|registry| Self(Some(registry.to_string())))
            .ok_or_else(|| RegistryError::NotFound.into_response())
    }
}

async fn get_whoami(
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(_): TargetRegistry,
) -> Response {
    private_no_cache(serve_whoami(&identity))
}

/// `PUT /-/user/org.couchdb.user:{name}` — adduser / login. Authenticates
/// from the request body, not the caller's existing identity.
async fn put_login(
    State(state): State<AppState>,
    TargetRegistry(_): TargetRegistry,
    Path(path): Path<UserPath>,
    body: axum::body::Bytes,
) -> Response {
    match path.user.strip_prefix("org.couchdb.user:") {
        Some(name) => add_user(&state, name, &body).await,
        None => not_found(),
    }
}

async fn delete_session_token(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(_): TargetRegistry,
    Path(path): Path<TokenPath>,
) -> Response {
    private_no_cache(logout(&state, &identity, &path.token).await)
}

async fn get_profile(
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(_): TargetRegistry,
) -> Response {
    private_no_cache(serve_profile(&identity))
}

async fn get_token_list(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(_): TargetRegistry,
) -> Response {
    private_no_cache(list_tokens(&state, &identity).await)
}

async fn delete_token_by_key(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(_): TargetRegistry,
    Path(path): Path<TokenKeyPath>,
) -> Response {
    private_no_cache(revoke_token_by_key(&state, &identity, &path.key).await)
}

/// The account routes capture their own parameter alongside the optional
/// `{prefix}`, so each needs a named shape rather than a bare `Path<String>`:
/// the prefixed registration captures two segments and a single-value `Path`
/// would refuse to deserialize it.
#[derive(Deserialize)]
struct UserPath {
    user: String,
}

#[derive(Deserialize)]
struct TokenPath {
    token: String,
}

#[derive(Deserialize)]
struct TokenKeyPath {
    key: String,
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
        Err(err) => return err.into_response(),
    };
    let resolved_source = resolve_registry_source(state, registry, name.as_str());
    let bytes = match &resolved_source {
        RegistrySource::Upstream(source) => {
            // The upstream registry's per-package rules gate the read — see
            // `serve_registry_packument`.
            if let Err(err) =
                authorize(state, identity, &resolved_source, name.as_str(), Action::Access)
            {
                return err.into_response();
            }
            match load_upstream_packument_for(state, identity, source, &name).await {
                Ok(Some(bytes)) => bytes,
                Ok(None) => return not_found(),
                Err(err) => return err.into_response(),
            }
        }
        RegistrySource::Hosted(source) => {
            // The hosted gate answers a denial itself — a not-found mask or
            // an explicit-rule 401/403 — see `serve_registry_packument`.
            let org = match hosted_read_namespace(state, identity, source, name.as_str()) {
                Ok(org) => org,
                Err(err) => return err.into_response(),
            };
            match state.inner.storage.for_hosted(&org).read_hosted_packument(&name).await {
                Ok(Some(bytes)) => bytes,
                Ok(None) => return not_found(),
                Err(err) => return err.into_response(),
            }
        }
        RegistrySource::Unclaimed | RegistrySource::NotFound => return not_found(),
    };
    let packument: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(err) => return RegistryError::Json(err).into_response(),
    };
    if let Some(osv_index) = state.inner.osv_index.as_ref() {
        let resolved = resolve_version_or_tag(&packument, version_or_tag);
        if is_osv_vulnerable_packument_version(&packument, name.as_str(), resolved, osv_index) {
            return not_found();
        }
    }
    let revision_registry = match &resolved_source {
        RegistrySource::Upstream(source) => revision_source_registry(state, registry, source),
        RegistrySource::Hosted(_) | RegistrySource::Unclaimed | RegistrySource::NotFound => None,
    };
    let manifest = match revision_registry {
        Some(source_registry) => extract_upstream_version_manifest(
            &packument,
            &name,
            version_or_tag,
            source_registry,
            tarball_base,
        ),
        None => extract_version_manifest(&packument, &name, version_or_tag, tarball_base),
    };
    let Some(manifest) = manifest else {
        return not_found();
    };
    match serde_json::to_vec(&manifest) {
        Ok(body) => packument_bytes_response(body, "application/json", None),
        Err(err) => RegistryError::Json(err).into_response(),
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
) -> Result<&'a Upstream, RegistryError> {
    let Some(config) = state.inner.config.upstreams.get(upstream) else {
        return Err(RegistryError::NotFound);
    };
    // A private upstream registry gates by its `access:` list; a public registry
    // (no access) is reachable by anyone at its `/~<name>/` URL, its upstream
    // credential (if any) staying server-side either way.
    if let Some(access) = config.access.as_ref()
        && !access.allows(identity)
    {
        let user = require_caller(identity, "upstream access")
            .unwrap_or_else(|_| "<anonymous>".to_string());
        return Err(RegistryError::Forbidden {
            user,
            action: "access",
            resource: format!("upstream {upstream:?}"),
        });
    }
    state.inner.upstreams.get(upstream).ok_or_else(|| RegistryError::NotFound)
}

fn authorized_revision_upstream<'a>(
    state: &'a AppState,
    identity: &Identity,
    registry: &str,
) -> Result<&'a Upstream, RegistryError> {
    if !matches!(state.inner.config.registries.get(registry), Some(Registry::Upstream { .. })) {
        return Err(RegistryError::NotFound);
    }
    let Some(config) = state.inner.config.upstreams.get(registry) else {
        return Err(RegistryError::NotFound);
    };
    if config.rules.refines_access() {
        return Err(RegistryError::NotFound);
    }
    authorized_upstream(state, identity, registry)
}

fn revision_registry_is_private(state: &AppState, registry: &str) -> bool {
    state.inner.config.upstreams.get(registry).is_some_and(|config| config.access.is_some())
}

fn revision_source_registry<'a>(
    state: &'a AppState,
    addressed_registry: &str,
    source: &str,
) -> Option<&'a str> {
    if addressed_registry != source {
        return None;
    }
    let config = state.inner.config.upstreams.get(source)?;
    (!config.rules.refines_access()).then_some(config.url.as_str())
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
        let epoch = pnpr_route::credential_digest(&format!(
            "{url}\0{}",
            pnpr_route::headers_credential_digest(&upstream_config.headers),
        ));
        let digest =
            pnpr_route::upstream_cache_digest(upstream, epoch, &config.resolution_cache_secret);
        return format!("~upstreams/{digest}");
    }
    // Public registry: a stable, secret-free namespace keyed by the registry name
    // and its origin URL (hashed so a path-unsafe value can't escape the
    // cache root).
    format!("~public/{}", pnpr_route::credential_digest(&format!("{upstream}\0{url}")))
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
        Err(err) => {
            return recover_stale_upstream_packument(state, namespace, upstream, name, err).await;
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

/// Serve a stale cache entry only when an upstream fetch failed transiently.
/// Authoritative client errors and cache-disabled upstreams preserve the fetch
/// error, while transport, server, and open-circuit failures may use bytes from
/// the same upstream namespace.
async fn recover_stale_upstream_packument(
    state: &AppState,
    namespace: &str,
    upstream: &Upstream,
    name: &PackageName,
    err: RegistryError,
) -> Result<Option<Vec<u8>>, RegistryError> {
    if !err.is_transient_upstream_error() || !upstream.caches() {
        return Err(err);
    }
    let Some(bytes) = state.inner.storage.read_upstream_packument_any(namespace, name).await?
    else {
        return Err(err);
    };
    // The upstream error may embed credentials in its request URL, so only its
    // credential-redacted rendering is safe to log.
    tracing::warn!(
        error = %err.log_message(),
        package = %name.as_str(),
        "upstream packument refetch failed; serving stale cache",
    );
    Ok(Some(bytes))
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
) -> Result<Option<Vec<u8>>, RegistryError> {
    let namespace = upstream_cache_namespace(state, upstream);
    let upstream = authorized_upstream(state, identity, upstream)?;
    let ttl = upstream.maxage().unwrap_or(state.inner.config.packument_ttl);
    load_upstream_packument(state, &namespace, upstream, name, ttl).await
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
) -> Result<Option<Vec<u8>>, RegistryError> {
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
            authorize(state, identity, &resolved_source, name.as_str(), Action::Access)?;
            load_upstream_packument_for(state, identity, source, name).await
        }
        RegistrySource::Hosted(source) => {
            let org = match hosted_gate(state, identity, source, name.as_str()) {
                HostedGate::Allowed(org) => org,
                HostedGate::MaskNotFound => return Ok(None),
                HostedGate::Denied(err) => return Err(err),
            };
            state.inner.storage.for_hosted(&org).read_hosted_packument(name).await
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
    revision_registry: Option<&str>,
) -> Response {
    let bytes = match load_upstream_packument_for(state, identity, upstream, name).await {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return not_found(),
        Err(err) => return err.into_response(),
    };
    match packument_response(
        name,
        &bytes,
        tarball_base,
        revision_registry,
        state.inner.osv_index.as_ref(),
        wants_abbreviated(headers),
    ) {
        Ok(response) => response,
        Err(err) => err.into_response(),
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
        Err(err) => return err.into_response(),
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
            if !pnpr_package_name::is_safe_path_segment(filename) {
                return err.into_response();
            }
            (filename.to_string(), None)
        }
    };
    let namespace = upstream_cache_namespace(state, upstream);
    let upstream = match authorized_upstream(state, identity, upstream) {
        Ok(upstream) => upstream,
        Err(err) => return err.into_response(),
    };
    // Pre-check OSV on the filename-derived version (when the name is
    // canonical) to fail fast; the authoritative check against the
    // packument-resolved version runs below either way.
    if let Some(version) = &parsed_version
        && let Err(err) = ensure_osv_allowed(state, &name, version)
    {
        return err.into_response();
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
        Err(err) => return err.into_response(),
    };
    let TarballDist { version, integrity } =
        match expected_tarball_dist(&packument, &name, &filename) {
            Ok(Some(dist)) => dist,
            Ok(None) => return not_found(),
            Err(err) => return err.into_response(),
        };
    if parsed_version.as_deref() != Some(version.as_str())
        && let Err(err) = ensure_osv_allowed(state, &name, &version)
    {
        return err.into_response();
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
        Err(err) => return err.into_response(),
    };
    let write =
        match state.inner.storage.open_upstream_tarball_tmp(&namespace, &name, &filename).await {
            Ok(write) => write,
            Err(err) => return err.into_response(),
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
            Err(err) => tarball_stream_error(err, &name, &filename).into_response(),
        };
    }
    // Stream the download to the client while teeing it into the namespaced
    // cache; the entry is promoted only on an SRI match (see
    // `stream_verified_to_cache`). No `Content-Length` is set: the upstream's
    // is attacker-controlled and unverifiable before streaming, so the body is
    // chunked and the client reads to EOF (then re-verifies the integrity).
    match streaming::stream_verified_to_cache(response, write, &integrity, MAX_TARBALL_BYTES) {
        Ok(body) => tarball_response(body, None),
        Err(err) => tarball_stream_error(err, &name, &filename).into_response(),
    }
}

async fn serve_revision_tarball(
    state: &AppState,
    identity: &Identity,
    registry: &str,
    digest: &str,
) -> Response {
    let Some(integrity) = integrity_addressed_tarball_integrity(digest) else {
        return not_found();
    };
    if matches!(state.inner.config.registries.get(registry), Some(Registry::Upstream { .. })) {
        let response =
            serve_upstream_revision_tarball(state, identity, registry, digest, &integrity).await;
        return if revision_registry_is_private(state, registry) {
            private_no_cache(response)
        } else {
            response
        };
    }
    serve_hosted_revision_tarball(state, identity, registry, digest, &integrity).await
}

async fn serve_upstream_revision_tarball(
    state: &AppState,
    identity: &Identity,
    registry: &str,
    digest: &str,
    integrity: &Integrity,
) -> Response {
    let upstream = match authorized_revision_upstream(state, identity, registry) {
        Ok(upstream) => upstream,
        Err(err) => return err.into_response(),
    };
    let namespace = upstream_cache_namespace(state, registry);
    if upstream.caches() {
        match state.inner.storage.open_upstream_revision_tarball(&namespace, digest).await {
            Ok(Some((file, len))) => {
                return revision_tarball_response(
                    streaming::stream_file(file),
                    Some(len),
                    digest,
                    integrity,
                );
            }
            Ok(None) => {}
            Err(err) => {
                tracing::warn!(?err, %registry, %digest, "revision tarball cache open failed");
            }
        }
    }
    let response = match upstream.fetch_revision_tarball_response(digest).await {
        Ok(FetchOutcome::Ok(response)) => response,
        Ok(FetchOutcome::NotFound) => return not_found(),
        Err(err) => return err.into_response(),
    };
    let write =
        match state.inner.storage.open_upstream_revision_tarball_tmp(&namespace, digest).await {
            Ok(write) => write,
            Err(err) => return err.into_response(),
        };
    if !upstream.caches() {
        return match streaming::download_verified_to_temp(
            response,
            write,
            integrity,
            MAX_TARBALL_BYTES,
        )
        .await
        {
            Ok((file, len, tmp_path)) => revision_tarball_response(
                streaming::stream_file_and_remove(file, tmp_path),
                Some(len),
                digest,
                integrity,
            ),
            Err(err) => {
                tarball_stream_error_for_package(err, "registry revision", digest).into_response()
            }
        };
    }
    match streaming::stream_verified_to_cache(response, write, integrity, MAX_TARBALL_BYTES) {
        Ok(body) => revision_tarball_response(body, None, digest, integrity),
        Err(err) => {
            tarball_stream_error_for_package(err, "registry revision", digest).into_response()
        }
    }
}

async fn serve_hosted_revision_tarball(
    state: &AppState,
    identity: &Identity,
    registry: &str,
    digest: &str,
    integrity: &Integrity,
) -> Response {
    let sources = hosted_revision_sources(state, registry);
    if sources.is_empty() {
        return not_found();
    }

    let mut private_refs = Vec::new();
    let mut policy_error = None;
    for source in sources {
        let Some(hosted) = state.inner.config.hosted.get(&source) else {
            continue;
        };
        let storage = state.inner.storage.for_hosted(&hosted.org);
        let refs = match hosted_revision_refs(&storage, digest).await {
            Ok(refs) => refs,
            Err(err) => return private_no_cache(err.into_response()),
        };
        for original in refs {
            let package = match PackageName::parse(&original.package) {
                Ok(package) => package,
                Err(err) => return private_no_cache(err.into_response()),
            };
            let filename = package.tarball_name_for_version(&original.version);
            if let Err(err) = package.canonicalize_tarball_name(&filename) {
                return private_no_cache(err.into_response());
            }
            if !matches!(
                resolve_registry_source(state, registry, package.as_str()),
                RegistrySource::Hosted(resolved) if resolved == source,
            ) || !matches!(
                hosted_gate(state, identity, &source, package.as_str()),
                HostedGate::Allowed(_),
            ) {
                continue;
            }
            match hosted_original_is_current(&storage, &package, &original.version, digest).await {
                Ok(true) => {}
                Ok(false) => continue,
                Err(err) => return private_no_cache(err.into_response()),
            }
            if let Err(err) = ensure_osv_allowed(state, &package, &original.version) {
                policy_error.get_or_insert(err);
                continue;
            }
            if matches!(
                hosted_gate(state, &Identity::Anonymous, &source, package.as_str()),
                HostedGate::Allowed(_),
            ) {
                let response = open_hosted_revision_tarball(
                    &storage,
                    &package,
                    &original.version,
                    digest,
                    integrity,
                )
                .await;
                if response.status() != StatusCode::NOT_FOUND {
                    return response;
                }
                continue;
            }
            private_refs.push((storage.clone(), package, original.version));
        }
    }

    for (storage, package, version) in private_refs {
        let response =
            open_hosted_revision_tarball(&storage, &package, &version, digest, integrity).await;
        if response.status() != StatusCode::NOT_FOUND {
            return response;
        }
    }
    if let Some(err) = policy_error {
        return private_no_cache(err.into_response());
    }
    private_no_cache(not_found())
}

fn hosted_revision_sources(state: &AppState, registry: &str) -> Vec<String> {
    match state.inner.config.registries.get(registry) {
        Some(Registry::Hosted { .. }) => vec![registry.to_string()],
        Some(Registry::Router { sources }) => sources
            .iter()
            .filter(|source| {
                matches!(state.inner.config.registries.get(source), Some(Registry::Hosted { .. }))
            })
            .cloned()
            .collect(),
        Some(Registry::Upstream { .. }) | None => Vec::new(),
    }
}

async fn hosted_revision_refs(
    storage: &Storage,
    digest: &str,
) -> Result<Vec<HostedOriginalRef>, RegistryError> {
    storage
        .read_hosted_revision_refs(digest)
        .await?
        .into_iter()
        .map(|bytes| serde_json::from_slice(&bytes).map_err(RegistryError::Json))
        .collect()
}

async fn hosted_original_is_current(
    storage: &Storage,
    package: &PackageName,
    version: &str,
    digest: &str,
) -> Result<bool, RegistryError> {
    let Some(bytes) = storage.read_hosted_packument(package).await? else {
        return Ok(false);
    };
    let packument = serde_json::from_slice::<HostedRevisionPackument>(&bytes)?;
    Ok(packument
        .versions
        .get(version)
        .and_then(|manifest| manifest.dist.as_ref())
        .and_then(original_integrity)
        .and_then(|integrity| integrity_addressed_tarball_path(&integrity))
        .is_some_and(|path| path == format!("-/tarballs/sha512/{digest}")))
}

async fn open_hosted_revision_tarball(
    storage: &Storage,
    package: &PackageName,
    version: &str,
    digest: &str,
    integrity: &Integrity,
) -> Response {
    let filename = package.tarball_name_for_version(version);
    match storage.open_hosted_tarball(package, &filename).await {
        Ok(Some((body, len))) => revision_tarball_response(body, len, digest, integrity),
        Ok(None) => not_found(),
        Err(err) => err.into_response(),
    }
}

#[derive(serde::Deserialize)]
struct HostedRevisionPackument {
    #[serde(default)]
    versions: IndexMap<String, HostedRevisionManifest>,
}

#[derive(serde::Deserialize)]
struct HostedRevisionManifest {
    #[serde(default)]
    dist: Option<HostedRevisionDist>,
}

#[derive(serde::Deserialize)]
struct HostedRevisionDist {
    #[serde(default)]
    integrity: Option<String>,
    #[serde(default)]
    revision: RevisionField,
    #[serde(default)]
    revisions: Vec<HostedRevisionRecord>,
}

#[derive(serde::Deserialize)]
struct HostedRevisionRecord {
    #[serde(default)]
    revision: Value,
    #[serde(default)]
    integrity: Option<String>,
}

#[derive(Default)]
enum RevisionField {
    #[default]
    Missing,
    Present(Value),
}

impl<'de> serde::Deserialize<'de> for RevisionField {
    fn deserialize<Deserializer>(deserializer: Deserializer) -> Result<Self, Deserializer::Error>
    where
        Deserializer: serde::Deserializer<'de>,
    {
        <Value as serde::Deserialize>::deserialize(deserializer).map(Self::Present)
    }
}

fn original_integrity(dist: &HostedRevisionDist) -> Option<Integrity> {
    let RevisionField::Present(revision) = &dist.revision else {
        return dist.integrity.as_deref()?.parse().ok();
    };
    let selected_revision =
        revision.as_u64().and_then(|revision| TarballRevision::try_from(revision).ok())?.get();
    let selected: Vec<_> = dist
        .revisions
        .iter()
        .filter(|record| record.revision.as_u64() == Some(selected_revision))
        .collect();
    if selected.len() != 1 || selected[0].integrity.as_deref() != dist.integrity.as_deref() {
        return None;
    }
    let originals: Vec<_> =
        dist.revisions.iter().filter(|record| record.revision.as_u64() == Some(0)).collect();
    if originals.len() != 1 {
        return None;
    }
    originals[0].integrity.as_deref()?.parse().ok()
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
// ([`pnpr_registry`]) and serves it there — authoritatively. Every concrete
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
        Err(err) => return err.into_response(),
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
                return err.into_response();
            }
            let revision_registry = revision_source_registry(state, registry, source);
            serve_packument_via_upstream(
                state,
                identity,
                headers,
                source,
                &name,
                tarball_base,
                revision_registry,
            )
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
        Err(err) => return err.into_response(),
    };
    let resolved_source = resolve_registry_source(state, registry, name.as_str());
    match &resolved_source {
        RegistrySource::Upstream(source) => {
            // Per-package rules before serving — see `serve_registry_packument`.
            if let Err(err) =
                authorize(state, identity, &resolved_source, name.as_str(), Action::Access)
            {
                return err.into_response();
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
) -> Result<String, RegistryError> {
    match hosted_gate(state, identity, source, package) {
        HostedGate::Allowed(org) => Ok(org),
        HostedGate::MaskNotFound => Err(RegistryError::NotFound),
        HostedGate::Denied(err) => Err(err),
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
        Err(err) => return err.into_response(),
    };
    // A hosted org has no upstream fallback: a package it does not host is a
    // definitive not-found. Reads come from the org's own storage namespace.
    match state.inner.storage.for_hosted(&org).read_hosted_packument(name).await {
        Ok(Some(bytes)) => match packument_response(
            name,
            &bytes,
            tarball_base,
            None,
            state.inner.osv_index.as_ref(),
            wants_abbreviated(headers),
        ) {
            Ok(response) => response,
            Err(err) => err.into_response(),
        },
        Ok(None) => not_found(),
        Err(err) => err.into_response(),
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
        Err(err) => return err.into_response(),
    };
    let (filename, name_version) = match name.parse_tarball_name(filename) {
        Ok(parsed) => parsed,
        Err(err) => return err.into_response(),
    };
    if let Err(err) = ensure_osv_allowed(state, name, &name_version) {
        return err.into_response();
    }
    match state.inner.storage.for_hosted(&org).open_hosted_tarball(name, &filename).await {
        Ok(Some((body, len))) => tarball_response(body, len),
        Ok(None) => not_found(),
        Err(err) => {
            tracing::warn!(?err, package = %name.as_str(), %filename, "hosted tarball open failed");
            err.into_response()
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
            name.as_str(),
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
            tarball_integrity_error(
                name.as_str(),
                filename,
                format!("malformed dist.integrity: {err}"),
            )
        })?
    } else {
        let shasum = dist.shasum.as_deref().ok_or_else(|| {
            tarball_integrity_error(
                name.as_str(),
                filename,
                format!("packument has no dist.integrity or dist.shasum for {version:?}"),
            )
        })?;
        Integrity::from_hex(shasum, ssri::Algorithm::Sha1).map_err(|err| {
            tarball_integrity_error(
                name.as_str(),
                filename,
                format!("malformed dist.shasum: {err}"),
            )
        })?
    };
    Ok(Some(TarballDist { version: version.clone(), integrity }))
}

fn tarball_stream_error(
    err: streaming::TarballStreamError,
    name: &PackageName,
    filename: &str,
) -> RegistryError {
    tarball_stream_error_for_package(err, name.as_str(), filename)
}

fn tarball_stream_error_for_package(
    err: streaming::TarballStreamError,
    package: &str,
    filename: &str,
) -> RegistryError {
    match err {
        streaming::TarballStreamError::Upstream { url, source } => {
            RegistryError::Upstream { url, source }
        }
        streaming::TarballStreamError::Io(err) => RegistryError::Io(err),
        streaming::TarballStreamError::Integrity(err) => tarball_integrity_error(
            package,
            filename,
            format!("integrity verification failed: {err}"),
        ),
        streaming::TarballStreamError::TooLarge { limit, received } => tarball_integrity_error(
            package,
            filename,
            format!("tarball body exceeds {limit} byte limit (received {received} bytes)"),
        ),
    }
}

fn tarball_integrity_error(package: &str, filename: &str, reason: String) -> RegistryError {
    RegistryError::TarballIntegrity {
        package: package.to_string(),
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
        Err(err) => return RegistryError::Json(err).into_response(),
    };
    let body_name = body.get("name").and_then(Value::as_str).unwrap_or("");
    if body_name != name {
        return RegistryError::BadRequest {
            reason: format!("username in URL ({name:?}) does not match body ({body_name:?})"),
        }
        .into_response();
    }
    let Some(password) = body.get("password").and_then(Value::as_str) else {
        return RegistryError::BadRequest { reason: "missing password".to_string() }
            .into_response();
    };

    let (outcome, username) = match state.inner.auth.users.add_or_login(name, password).await {
        Ok(o) => o,
        Err(err) => return err.into_response(),
    };
    let token = match state.inner.auth.tokens.issue(&username).await {
        Ok(t) => t,
        Err(err) => return err.into_response(),
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
        Err(err) => return err.into_response(),
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
        Err(err) => return err.into_response(),
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
        Err(err) => return err.into_response(),
    };
    let tokens = match state.inner.auth.tokens.list_for_user(&username).await {
        Ok(tokens) => tokens,
        Err(err) => return err.into_response(),
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
        Err(err) => return err.into_response(),
    };
    match state.inner.auth.tokens.find_by_key(key).await {
        Ok(Some(record)) if record.username != username => RegistryError::Forbidden {
            user: username,
            action: "revoke",
            resource: "this token".to_string(),
        }
        .into_response(),
        Ok(Some(_)) => match state.inner.auth.tokens.revoke_by_key(key).await {
            Ok(Some(_)) => json_response(StatusCode::OK, &json!({ "ok": "token revoked" })),
            Ok(None) => not_found(),
            Err(err) => err.into_response(),
        },
        Ok(None) => not_found(),
        Err(err) => err.into_response(),
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
        Err(err) => return err.into_response(),
    };
    let target_owner = match state.inner.auth.tokens.lookup(raw_token).await {
        Ok(Some(owner)) => owner,
        Ok(None) => return not_found(),
        Err(err) => return err.into_response(),
    };
    if target_owner != username {
        return RegistryError::Forbidden {
            user: username,
            action: "revoke",
            resource: "this token".to_string(),
        }
        .into_response();
    }
    match state.inner.auth.tokens.revoke_by_raw(raw_token).await {
        Ok(Some(_)) => json_response(StatusCode::OK, &json!({ "ok": true })),
        Ok(None) => not_found(),
        Err(err) => err.into_response(),
    }
}

fn token_response_object(key: &str, record: &pnpr_auth::TokenRecord) -> Value {
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
    require_protocol_caller(&state, request, next, "dependency resolution").await
}

async fn require_artifact_caller(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    require_protocol_caller(&state, request, next, "shared artifacts").await
}

async fn require_protocol_caller(
    state: &AppState,
    request: Request,
    next: Next,
    resource: &str,
) -> Response {
    match caller_username(state, request.headers()).await {
        Ok(Some(_username)) => next.run(request).await,
        Ok(None) => {
            RegistryError::Unauthenticated { resource: resource.to_string() }.into_response()
        }
        Err(error) => error.into_response(),
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

/// Mark a response as caller-scoped and uncacheable. Authenticated endpoints
/// can return per-user data keyed on the `Authorization` header, so a shared
/// HTTP cache that ignored `Vary` could hand one caller's data to another.
/// Apply this to every response branch so intermediaries cannot cache errors
/// either.
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
    let Some(text) = pnpr_search::parse_query(query_string) else {
        return result(Vec::new());
    };
    let Some(registry) = registry.map(str::to_string).or_else(|| default_registry_target(state))
    else {
        return result(Vec::new());
    };
    let size = pnpr_search::parse_size(query_string, 20);
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
        match pnpr_search::run_local_search(&storage, &text, size - objects.len(), keep).await {
            Ok(mut entries) => objects.append(&mut entries),
            Err(err) => return err.into_response(),
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
) -> Result<&'a HostedConfig, RegistryError> {
    let scope = scope.strip_prefix('@').unwrap_or(scope);
    if scope.is_empty() {
        return Err(RegistryError::NotFound);
    }
    let target = match registry {
        Some(registry) => registry.to_string(),
        None => match default_registry_target(state) {
            Some(target) => target,
            None => return Err(RegistryError::NotFound),
        },
    };
    let probe = format!("@{scope}/-");
    let RegistrySource::Hosted(source) = resolve_registry_source(state, &target, &probe) else {
        return Err(RegistryError::NotFound);
    };
    let Some(hosted) = state.inner.config.hosted.get(&source) else {
        return Err(RegistryError::NotFound);
    };
    if !hosted.rules.default_access().allows(identity) {
        return Err(RegistryError::NotFound);
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
        Err(err) => return err.into_response(),
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
        Err(err) => return err.into_response(),
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
        return response.into_response();
    }
    RegistryError::TeamsConfigManaged { action }.into_response()
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
) -> Result<WriteTarget, RegistryError> {
    match resolve_publish_target(state, identity, registry, name.as_str()) {
        PublishTarget::Hosted { source, org } => Ok(WriteTarget { source, org }),
        PublishTarget::Reject(reason) => Err(RegistryError::BadRequest { reason }),
        PublishTarget::Denied(response) => Err(response),
        PublishTarget::NotFound => Err(RegistryError::NotFound),
    }
}

/// The hosted registry a write resolved to: its name (for the
/// `publish`/`unpublish` rule lookup) and its storage namespace.
struct WriteTarget {
    source: String,
    org: String,
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
    revision_registry: Option<&str>,
    osv_index: Option<&Arc<pnpr_osv::OsvIndex>>,
    abbreviated: bool,
) -> Result<Response, RegistryError> {
    let mut doc: Value = serde_json::from_slice(bytes)?;
    filter_osv_vulnerable_versions(&mut doc, name, osv_index);
    match revision_registry {
        Some(source_registry) => {
            rewrite_upstream_tarball_urls(&mut doc, name, source_registry, tarball_base);
        }
        None => rewrite_tarball_urls(&mut doc, name, tarball_base),
    }
    let last_modified = packument_last_modified(&doc);
    let (body, content_type) = if abbreviated {
        let trimmed = abbreviate_packument(&doc, Utc::now());
        (serde_json::to_vec(&trimmed)?, ABBREVIATED_CONTENT_TYPE)
    } else {
        (serde_json::to_vec(&doc)?, "application/json")
    };
    Ok(packument_bytes_response(body, content_type, last_modified))
}

fn filter_osv_vulnerable_versions(
    packument: &mut Value,
    name: &PackageName,
    osv_index: Option<&Arc<pnpr_osv::OsvIndex>>,
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
    osv_index: Option<&Arc<pnpr_osv::OsvIndex>>,
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
    osv_index: &pnpr_osv::OsvIndex,
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
        advisories: pnpr_osv::format_advisory_ids(&ids),
    })
}

fn packument_bytes_response(
    bytes: Vec<u8>,
    content_type: &'static str,
    last_modified: Option<String>,
) -> Response {
    let mut builder =
        Response::builder().status(StatusCode::OK).header(header::CONTENT_TYPE, content_type);
    if let Some(last_modified) = last_modified {
        builder = builder.header(header::LAST_MODIFIED, last_modified);
    }
    builder.body(Body::from(bytes)).expect("static-shape response always builds")
}

/// `Last-Modified` value for a served packument: the document's
/// `time.modified` in HTTP-date form. Lets a client's release-age check
/// (pnpm's `minimumReleaseAge`) learn the package-level last-publish
/// upper bound from response headers alone — a `HEAD` costs ~no bytes
/// where the abbreviated body runs to hundreds of KB. Fractional
/// seconds round *up* to the next whole second: the header must stay
/// an upper bound on the publish time, and truncating would understate
/// it by up to 999ms — exactly the window a release-age check guards.
/// `None` when the document carries no parsable `time.modified`; the
/// header is simply omitted then.
fn packument_last_modified(doc: &Value) -> Option<String> {
    let modified = doc.get("time")?.get("modified")?.as_str()?;
    let parsed = chrono::DateTime::parse_from_rfc3339(modified).ok()?;
    let mut whole_seconds = parsed.with_timezone(&Utc);
    if whole_seconds.timestamp_subsec_nanos() > 0 {
        whole_seconds += chrono::Duration::seconds(1);
    }
    Some(whole_seconds.format("%a, %d %b %Y %H:%M:%S GMT").to_string())
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

fn revision_tarball_response(
    body: Body,
    content_length: Option<u64>,
    digest: &str,
    integrity: &Integrity,
) -> Response {
    let mut response = tarball_response(body, content_length);
    let headers = response.headers_mut();
    headers.insert(
        header::CACHE_CONTROL,
        "public, max-age=31536000, immutable".parse().expect("static cache control is valid"),
    );
    headers.insert(
        header::ETAG,
        format!(r#""{digest}""#).parse().expect("canonical base64url digest is a valid ETag"),
    );
    if let [hash] = integrity.hashes.as_slice() {
        headers.insert(
            "content-digest",
            format!("sha-512=:{}:", hash.digest)
                .parse()
                .expect("canonical base64 digest is a valid header value"),
        );
    }
    private_no_cache(response)
}

fn not_found() -> Response {
    RegistryError::NotFound.into_response()
}

async fn serve_ping(State(_state): State<AppState>) -> Response {
    (StatusCode::OK, axum::Json(serde_json::json!({}))).into_response()
}

/// `GET /-/pnpr` — capability handshake for the pnpr resolver
/// protocol. A plain npm registry has no such route and 404s, so a
/// client can fail fast against a misconfigured server. `versions`
/// lists the `/-/pnpr/vN/resolve` protocol versions this server speaks;
/// `fixLockfile` narrows that list to versions that honor repair requests.
async fn serve_pnpr_handshake(State(state): State<AppState>) -> Response {
    let versions = state.inner.config.resolver.enabled.then_some(0).into_iter().collect::<Vec<_>>();
    let fix_lockfile = versions.clone();
    let artifacts =
        state.inner.config.artifacts.enabled.then_some(0).into_iter().collect::<Vec<_>>();
    (
        StatusCode::OK,
        axum::Json(serde_json::json!({
            "pnpr": {
                "versions": versions,
                "artifacts": artifacts,
                "fixLockfile": fix_lockfile,
            }
        })),
    )
        .into_response()
}

/// 404 stub mounted on the capability handshake when neither pnpr protocol is
/// enabled. It prevents the registry catch-all from proxying the probe
/// upstream.
async fn pnpr_protocols_disabled() -> Response {
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

async fn serve_publish_artifact(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    body: axum::body::Bytes,
) -> Response {
    let username = match require_caller(&identity, "shared artifact publication") {
        Ok(username) => username,
        Err(err) => return private_no_cache(err.into_response()),
    };
    let request = match pnpr_shared_artifacts::parse_publish(&body) {
        Ok(request) => request,
        Err(err) => return private_no_cache(err.into_response()),
    };
    private_no_cache(
        match state
            .inner
            .artifacts
            .as_ref()
            .expect("artifact routes require an artifact store")
            .publish(&username, request)
            .await
        {
            Ok(true) => StatusCode::CREATED.into_response(),
            Ok(false) => StatusCode::OK.into_response(),
            Err(err) => err.into_response(),
        },
    )
}

async fn serve_resolve_artifacts(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    body: axum::body::Bytes,
) -> Response {
    let username = match require_caller(&identity, "shared artifact lookup") {
        Ok(username) => username,
        Err(err) => return private_no_cache(err.into_response()),
    };
    private_no_cache(
        match state
            .inner
            .artifacts
            .as_ref()
            .expect("artifact routes require an artifact store")
            .resolve(&username, &body)
            .await
        {
            Ok(response) => (StatusCode::OK, axum::Json(response)).into_response(),
            Err(err) => err.into_response(),
        },
    )
}

async fn serve_artifact_blob(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    body: axum::body::Bytes,
) -> Response {
    let username = match require_caller(&identity, "shared artifact blob") {
        Ok(username) => username,
        Err(err) => return private_no_cache(err.into_response()),
    };
    match state
        .inner
        .artifacts
        .as_ref()
        .expect("artifact routes require an artifact store")
        .read_blob(&username, &body)
        .await
    {
        Ok(Some(blob)) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(header::CONTENT_LENGTH, blob.size.to_string())
            .header(header::CACHE_CONTROL, "private, max-age=31536000, immutable")
            .header(header::VARY, "Authorization")
            .body(Body::from_stream(blob.stream))
            .expect("static artifact blob response always builds"),
        Ok(None) => private_no_cache(StatusCode::NOT_FOUND.into_response()),
        Err(err) => private_no_cache(err.into_response()),
    }
}
