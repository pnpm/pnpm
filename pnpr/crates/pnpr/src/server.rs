pub(crate) use tcp_listener::PeerAddr;

pub(crate) use self::striped_locks::StripedLocks;

mod tcp_listener;
use tcp_listener::NodelayTcpListener;

mod pnpr_protocol;
use pnpr_protocol::{
    pnpr_protocols_disabled, serve_artifact_blob, serve_pnpr_handshake, serve_publish_artifact,
    serve_resolve, serve_resolve_artifacts, serve_verify_lockfile,
};

mod pipeline_runs;
use pipeline_runs::{
    serve_get_pipeline_run, serve_list_pipeline_runs, serve_pipeline_ui, serve_publish_pipeline_run,
};

mod organizations;
use organizations::{get_org_teams, get_team_members, reject_team_mutation, serve_org_packages};

mod package_search;
use package_search::{
    DiscoverySource, SearchPage, discovery_sources, hosted_search_names, serve_search,
};

mod package_responses;
use package_responses::{
    DistBlock, TarballDist, ensure_osv_allowed, expected_tarball_dist,
    filter_osv_vulnerable_dist_tags, is_osv_vulnerable_packument_version, packument_bytes_response,
    packument_response, resolve_version_or_tag, revision_tarball_response, tarball_integrity_error,
    tarball_response, tarball_stream_error, tarball_stream_error_for_package, wants_abbreviated,
};

mod package_reads;
use package_reads::{
    HostedGate, RegistrySource, default_registry_target, hosted_gate, hosted_read_namespace,
    resolve_ecosystem_source, resolve_registry_source, resolves_to_private_source, serve_packument,
    serve_tarball, serve_version_manifest,
};

mod revision_refs;
use revision_refs::{
    RevisionScan, RevisionSource, hosted_revision_refs, hosted_revision_sources,
    serve_private_revision_refs, serve_revision_refs,
};

mod revision_tarballs;
use revision_tarballs::{
    declared_tarball_integrity, hosted_original_is_current, open_hosted_revision_tarball,
    serve_revision_tarball,
};

mod upstream_tarballs;
use upstream_tarballs::{cached_upstream_tarball, serve_tarball_via_upstream};

mod upstream_packuments;
use upstream_packuments::{
    load_packument_for_read, load_upstream_packument, read_source_packument,
    serve_packument_via_upstream,
};

mod request_access;
use request_access::{
    authorized_revision_upstream, authorized_upstream, compute_upstream_cache_namespace,
    require_artifact_caller, require_caller, require_pipeline_caller, require_resolver_caller,
    revision_registry_is_private, revision_source_registry, single_authorization_header,
    upstream_cache_namespace,
};

mod user_accounts;
use user_accounts::{
    delete_session_token, delete_token_by_key, get_profile, get_token_list, get_whoami, put_login,
};

mod authentication;
mod batch;
mod cargo;
mod compiler_cache;
mod documents;
mod ecosystem;
mod oci;
mod oidc;
mod package_mutation;
mod publishing;
mod pypi;
mod registry_directory;
mod routing;
mod staged;
mod striped_locks;

#[cfg(test)]
mod tests;

use self::{
    authentication::{Action, AuthedCaller, authenticate, authorize},
    documents::RegistryDocuments,
    ecosystem::{addressed_registry, caller_scoped, registry_endpoint},
    package_mutation::{
        delete_package, delete_tarball, get_dist_tags, remove_dist_tag, set_dist_tag,
        update_packument,
    },
    publishing::{
        PublishTarget, commit_publishes, publish_package, resolve_publish_target_for,
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
use pnpr_package_name::CanonicalPackageName;
use pnpr_policy::Identity;
use pnpr_registry::{ConcreteKind, Ecosystem, Registry, Resolved};
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
use serde_json::{Map, Value, json};
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
/// A pipeline run record is a summary plus a bounded event stream; well
/// under this in practice, with the ceiling guarding against a hostile
/// client rather than a large workspace.
const MAX_PIPELINE_RUN_BODY_BYTES: usize = 4 * 1024 * 1024;
#[derive(Clone)]
struct AppState {
    inner: Arc<AppInner>,
}

struct AppInner {
    storage: Storage,
    artifacts: Option<pnpr_shared_artifacts::SharedArtifactStore>,
    compiler_cache_uploads: tokio::sync::Semaphore,
    pipeline_runs: Option<pnpr_pipeline_runs::PipelineRunStore>,
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
    oidc: pnpr_auth::oidc::OidcState,
    /// Serializes the read-modify-write packument flows per package so
    /// two concurrent writers to the same package on this instance can't
    /// lose each other's changes.
    package_locks: StripedLocks,
    referrer_migration_locks: StripedLocks,
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

/// Bring the publish journal to a consistent state: apply every sealed
/// transaction, discard every unsealed one. [`serve`] and [`serve_listener`]
/// call this before binding; an embedder that builds a router directly should
/// call it itself on startup, before serving requests.
pub async fn recover_publish_journal(config: &Config) -> pnpr_error::Result<()> {
    pnpr_storage::journal::recover_publish_journal(config, &RegistryDocuments).await?;
    let storage =
        Storage::new(&config.hosted_store, config.storage.clone(), config.cache_storage.clone())?;
    for hosted in config.hosted.values() {
        storage.for_hosted(&hosted.org).rebuild_package_index().await?;
    }
    Ok(())
}

/// Reclaim blob uploads abandoned before an unclean shutdown, and any left
/// by a client that never came back. Only image pushes create these, so this
/// is a no-op for a registry serving no image ecosystem.
async fn sweep_abandoned_uploads(config: &Config) -> pnpr_error::Result<()> {
    if !config.registries.has_ecosystem(Ecosystem::Oci) {
        return Ok(());
    }
    let storage =
        Storage::new(&config.hosted_store, config.storage.clone(), config.cache_storage.clone())?;
    // Shared sessions live in their hosted namespace. Local scratch can be
    // shared by those namespaces and is safe to sweep more than once.
    let mut namespaces: Vec<&str> =
        config.hosted.values().map(|hosted| hosted.org.as_str()).collect();
    namespaces.push("");
    namespaces.sort_unstable();
    namespaces.dedup();
    for namespace in namespaces {
        let swept = storage
            .for_hosted(namespace)
            .sweep_blob_uploads(pnpr_storage::upload::UPLOAD_MAX_AGE)
            .await?;
        if swept > 0 {
            tracing::info!(swept, namespace, "reclaimed abandoned blob uploads");
        }
    }
    Ok(())
}

/// Run startup side effects and load the auth backends. The registry
/// needs publish-journal recovery; auth loads on every tier because the
/// account endpoints (which mint and manage tokens) are always served,
/// and every mounted surface consults caller identity.
async fn load_startup_auth(config: &Config) -> pnpr_error::Result<AuthState> {
    if config.registry.enabled {
        recover_publish_journal(config).await?;
        sweep_abandoned_uploads(config).await?;
    }
    AuthState::load(&config.auth, &config.backend).await
}

/// The request URI as recorded in the access log. npm's logout protocol
/// (`DELETE .../-/user/token/{token}`, path-less or under a `/~<prefix>/`)
/// puts the raw bearer token in the URL path, and a reusable credential
/// must never reach a log line, so everything after that marker is
/// redacted. OIDC callback queries are also omitted. Other URIs are logged verbatim; a false positive (a
/// registry path that merely embeds the marker) is redacted too, which
/// only costs log detail on a request no route serves.
fn loggable_uri(uri: &axum::http::Uri) -> String {
    if uri.path().starts_with("/-/oidc/") {
        return uri.path().to_string();
    }
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
        pipeline = config.pipeline.enabled,
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

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown signal received");
}

// --------------------------------------------------------------------
// Account routes — adduser/login, whoami, profile, token list and
// revocation, logout. Mounted on every tier (see the router construction
// in `router_with_auth_and_osv`), each with a `/~<name>/`-addressed twin.
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
/// bare, once under `/~{registry}` — pointing at the same handler. This
/// extractor is what tells the two apart, so a handler states once that it is
/// prefix-aware instead of needing a near-identical twin.
///
/// A bare `~` names no registry, so it rejects with 404 rather than reading as
/// the path-less base.
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
                // UTF-8. It cannot be a registry name, so answer it the same
                // 404 every other malformed prefix gets rather than a 500 — a
                // bad URL is not a server fault, and rendering it as one would
                // also let a client fill the error log.
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
        let Some((_, registry)) = params.iter().find(|(name, _)| *name == "registry") else {
            return Ok(Self(None));
        };
        if !pnpr_package_name::is_safe_path_segment(registry) {
            return Err(RegistryError::NotFound.into_response());
        }
        Ok(Self(Some(registry.to_string())))
    }
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
    name: &CanonicalPackageName,
) -> Result<WriteTarget, RegistryError> {
    resolve_write_target_for(state, identity, registry, Ecosystem::Npm, name)
}

/// [`resolve_write_target`] for one ecosystem's surface.
fn resolve_write_target_for(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    ecosystem: Ecosystem,
    name: &CanonicalPackageName,
) -> Result<WriteTarget, RegistryError> {
    match resolve_publish_target_for(state, identity, registry, ecosystem, name.as_str()) {
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

fn not_found() -> Response {
    RegistryError::NotFound.into_response()
}

async fn serve_ping(State(_state): State<AppState>) -> Response {
    (StatusCode::OK, axum::Json(serde_json::json!({}))).into_response()
}
