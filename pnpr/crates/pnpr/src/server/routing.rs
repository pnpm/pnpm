use std::{sync::Arc, time::Duration};

use axum::{
    Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, RawQuery, Request, State},
    http::{HeaderMap, HeaderValue, Method, header},
    middleware,
    response::Response,
    routing::{any, delete, get, post, put},
};
use indexmap::IndexMap;
use tower_http::{
    compression::{
        CompressionLayer,
        predicate::{DefaultPredicate, NotForContentType, Predicate as _},
    },
    cors::{AllowOrigin, CorsLayer},
    trace::TraceLayer,
};
use tracing::Span;

use pnpr_auth::AuthState;
use pnpr_config::Config;
use pnpr_registry::Ecosystem;
use pnpr_storage::Storage;
use pnpr_upstream::Upstream;
use serde::Deserialize;

use super::{
    AppInner, AppState, AuthedCaller, MAX_ARTIFACT_BLOB_BODY_BYTES,
    MAX_ARTIFACT_PUBLISH_BODY_BYTES, MAX_ARTIFACT_RESOLVE_BODY_BYTES, MAX_LOGIN_BODY_BYTES,
    MAX_PUBLISH_BODY_BYTES, StripedLocks, TargetRegistry, addressed_registry, authenticate, batch,
    caller_scoped, cargo, compute_upstream_cache_namespace, delete_package, delete_session_token,
    delete_tarball, delete_token_by_key, get_dist_tags, get_org_teams, get_profile,
    get_team_members, get_token_list, get_whoami, loggable_uri, not_found, pnpr_protocols_disabled,
    private_no_cache, publish_package, put_login, pypi, reject_team_mutation, remove_dist_tag,
    require_artifact_caller, require_resolver_caller, serve_artifact_blob, serve_batch_publish,
    serve_org_packages, serve_packument, serve_ping, serve_pnpr_handshake, serve_publish_artifact,
    serve_resolve, serve_resolve_artifacts, serve_revision_tarball, serve_search, serve_tarball,
    serve_verify_lockfile, serve_version_manifest, set_dist_tag, staged, update_packument,
};

pub(super) fn router_with_auth_and_osv(
    config: Config,
    auth: AuthState,
    osv_index: Option<Arc<pnpr_osv::OsvIndex>>,
) -> pnpr_error::Result<Router> {
    let cors_origins = config
        .cors
        .allowed_origins()
        .iter()
        .map(|origin| {
            HeaderValue::from_str(origin).map_err(|_| pnpr_error::RegistryError::InvalidConfig {
                reason: format!("CORS allowed origin {origin:?} is not a valid HTTP header value"),
            })
        })
        .collect::<pnpr_error::Result<Vec<_>>>()?;
    let storage =
        Storage::new(&config.hosted_store, config.storage.clone(), config.cache_storage.clone())?;
    let registry_enabled = config.registry.enabled;
    let resolver_enabled = config.resolver.enabled;
    let artifacts_enabled = config.artifacts.enabled;
    let artifacts = artifacts_enabled
        .then(|| {
            pnpr_shared_artifacts::SharedArtifactStore::new(
                &config.hosted_store,
                &config.cache_storage,
            )
        })
        .transpose()?;
    // Only the registry routes consult the upstreams, so a resolver-only
    // server builds none — skipping a `ThrottledClient` allocation per
    // configured upstream.
    let upstreams: IndexMap<String, Upstream> = if registry_enabled {
        config
            .upstreams
            .iter()
            .map(|(name, upstream)| {
                let client = Upstream::new(name, upstream);
                let client = if config.registries.ecosystem(name) == Some(Ecosystem::Npm) {
                    client
                } else {
                    client
                        .with_fetch_guard(super::ecosystem::upstream_fetch_guard(&config, upstream))
                };
                (name.clone(), client)
            })
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
            artifacts,
            upstreams,
            upstream_cache_namespaces,
            config,
            auth,
            package_locks: StripedLocks::new(),
            resolver: std::sync::OnceLock::new(),
            osv_index,
        }),
    };
    // `/-/ping` is a health check and is always served. The two
    // configurable surfaces are mounted only when their feature is enabled,
    // so resolver, registry, and artifacts can be deployed independently.
    // The config guarantees at least one is enabled.
    let mut router = Router::new().route("/-/ping", get(serve_ping));
    let account = account_routes();
    router = router.merge(account.clone());
    // The install-accelerator and shared-artifact surfaces live under the
    // reserved `/-/pnpr` namespace. The handshake advertises each protocol
    // independently, so either surface can be mounted on its own.
    //
    // When both protocol surfaces are disabled, only `/-/pnpr` gets a 404
    // stub: it is the capability-probe path and overlaps the registry catch-all
    // (`/-/pnpr` matches `/{first}/{second}`), so without the stub a probe
    // would be proxied upstream, giving a confusing 502 where a client
    // expects the "no pnpr protocols here" 404. The `/-/pnpr/v0/*` endpoints
    // carry no capability probe, so they are left unmounted rather than
    // stubbed.
    if resolver_enabled || artifacts_enabled || registry_enabled {
        router = router.route("/-/pnpr", get(serve_pnpr_handshake));
    } else {
        router = router.route("/-/pnpr", any(pnpr_protocols_disabled));
    }
    if resolver_enabled {
        router = router
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
    }
    if artifacts_enabled {
        router = router
            .route(
                "/-/pnpr/v0/artifacts",
                put(serve_publish_artifact)
                    .route_layer(DefaultBodyLimit::max(MAX_ARTIFACT_PUBLISH_BODY_BYTES))
                    .route_layer(middleware::from_fn_with_state(
                        state.clone(),
                        require_artifact_caller,
                    )),
            )
            .route(
                "/-/pnpr/v0/artifacts/resolve",
                post(serve_resolve_artifacts)
                    .route_layer(DefaultBodyLimit::max(MAX_ARTIFACT_RESOLVE_BODY_BYTES))
                    .route_layer(middleware::from_fn_with_state(
                        state.clone(),
                        require_artifact_caller,
                    )),
            )
            .route(
                "/-/pnpr/v0/artifacts/blob",
                post(serve_artifact_blob)
                    .route_layer(DefaultBodyLimit::max(MAX_ARTIFACT_BLOB_BODY_BYTES))
                    .route_layer(middleware::from_fn_with_state(
                        state.clone(),
                        require_artifact_caller,
                    )),
            );
    }
    if registry_enabled {
        let npm = npm_registry_routes();
        router = router
            // One publish transaction for packages of any ecosystem. It
            // answers here rather than inside the npm surface.
            .route("/-/pnpr/v0/publish", put(batch::serve_ecosystem_publish));
        if state.inner.config.registries.is_only_ecosystem(Ecosystem::Npm) {
            router = router.merge(npm);
        } else {
            router = router.nest("/npm", account.merge(npm));
        }
        if state.inner.config.registries.has_ecosystem(Ecosystem::Cargo) {
            router = router.merge(cargo::routes(
                !state.inner.config.registries.is_only_ecosystem(Ecosystem::Cargo),
            ));
        }
        if state.inner.config.registries.has_ecosystem(Ecosystem::Pypi) {
            router = router.merge(pypi::routes(
                !state.inner.config.registries.is_only_ecosystem(Ecosystem::Pypi),
            ));
        }
    }
    let mut router = router
        .layer(DefaultBodyLimit::max(MAX_PUBLISH_BODY_BYTES))
        // Authenticate once, ahead of every handler: resolve the caller,
        // enforce bearer-token read-only / CIDR restrictions (so a
        // restricted token is rejected before a write handler buffers its
        // up-to-100-MiB body), and stash the identity for handlers to read.
        // Inside the trace layer below, so a rejection is still one record.
        .layer(axum::middleware::from_fn_with_state(state.clone(), authenticate));
    if !cors_origins.is_empty() {
        router = router.layer(
            CorsLayer::new()
                .allow_origin(AllowOrigin::list(cors_origins))
                .allow_methods([Method::GET, Method::HEAD, Method::OPTIONS])
                .allow_headers([header::AUTHORIZATION, header::ACCEPT, header::CONTENT_TYPE]),
        );
    }
    let router = router
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
        );
    Ok(router.with_state(state))
}

/// The account endpoints — adduser/login, whoami, profile, token
/// listing/revocation, logout. They are pnpr account management, not
/// npm-registry functionality: they mint and manage the tokens every
/// authenticated surface demands, so they ride every tier alongside
/// `/-/ping`. A resolver- or artifacts-only tier can then issue its own
/// credentials (`pnpm login --registry https://<resolver-host>/`) instead of
/// depending on a registry-serving replica that shares the auth backend.
///
/// Each endpoint also answers under any `/~<name>/`, so a client whose
/// registry URL is a registry endpoint can log in against it. The identity
/// endpoints are global and consult no registry state; a registry-table lookup
/// would gate nothing while turning the 401-vs-404 split into an existence
/// oracle for private registry names that the content handlers carefully mask.
fn account_routes() -> Router<AppState> {
    let mut router = Router::new();
    for base in ["", "/~{registry}"] {
        let path = |tail: &str| format!("{base}{tail}");
        router = router
            .route(&path("/-/whoami"), get(get_whoami))
            .route(
                &path("/-/user/{user}"),
                put(put_login).route_layer(DefaultBodyLimit::max(MAX_LOGIN_BODY_BYTES)),
            )
            .route(&path("/-/user/token/{token}"), delete(delete_session_token))
            .route(&path("/-/npm/v1/user"), get(get_profile))
            .route(&path("/-/npm/v1/tokens"), get(get_token_list))
            .route(&path("/-/npm/v1/tokens/token/{key}"), delete(delete_token_by_key));
    }
    router
}

/// The npm-registry surface: every packument/tarball read, publish,
/// unpublish, dist-tag, and search. Mounted only when the registry feature is
/// on (registries declared and not `--disable-registry`), so resolver- and
/// artifacts-only tiers expose no registry surface at all.
///
/// Every route is registered twice: bare, addressing the default target
/// through the path-less base, and under `/~<name>/`, addressing one named
/// registry. [`TargetRegistry`] is what tells a handler which of the two it
/// was reached through.
fn npm_registry_routes() -> Router<AppState> {
    // Batch publish: one request carrying many packages' publish documents.
    // Not part of the standard npm registry API — `pnpm publish --batch` opts
    // into it explicitly, and only against the path-less base.
    let mut router = Router::new().route("/-/pnpm/v1/publish", put(serve_batch_publish));
    for base in ["", "/~{registry}"] {
        let path = |tail: &str| format!("{base}{tail}");
        router = router
            // Staged (two-phase) publishing — the `pnpm stage` surface.
            .route(&path("/-/stage"), get(staged::list_staged))
            .route(&path("/-/stage/package/{name}"), post(staged::post_staged_publish))
            .route(&path("/-/stage/{id}"), get(staged::get_staged).delete(staged::reject_staged))
            .route(&path("/-/stage/{id}/approve"), post(staged::approve_staged))
            .route(&path("/-/stage/{id}/tarball"), get(staged::get_staged_tarball))
            .route(&path("/-/v1/search"), get(get_search))
            .route(&path("/-/tarballs/sha512/{digest}"), get(get_revision_tarball))
            .route(&path("/-/package/{name}/dist-tags"), get(get_package_dist_tags))
            .route(
                &path("/-/package/{name}/dist-tags/{tag}"),
                put(put_package_dist_tag).delete(delete_package_dist_tag),
            )
            .route(&path("/-/org/{scope}/team"), get(get_teams).put(put_team))
            .route(&path("/-/org/{scope}/package"), get(get_org_package_list))
            .route(&path("/-/team/{scope}/{team}"), delete(delete_team))
            .route(
                &path("/-/team/{scope}/{team}/user"),
                get(get_team_users).put(put_team_user).delete(delete_team_user),
            )
            // Package addresses. npm spells a scoped name either as two
            // literal segments (`/@scope/name`) or as one percent-encoded
            // segment (`/@scope%2Fname`), so a path's segment count never says
            // on its own which resource it names. The `-` and `-rev` markers
            // do: whatever stands to their left is the package.
            .route(&path("/{name}"), get(get_packument).put(put_package))
            .route(
                &path("/{first}/{second}"),
                get(get_packument_or_version_manifest).put(put_scoped_package),
            )
            .route(&path("/{name}/-/{filename}"), get(get_tarball))
            .route(
                &path("/{name}/-rev/{rev}"),
                put(put_packument_revision).delete(unpublish_package),
            )
            .route(&path("/{scope}/{name}/{version}"), get(get_scoped_version_manifest))
            .route(&path("/{scope}/{name}/-/{filename}"), get(get_scoped_tarball))
            .route(&path("/{name}/-/{filename}/-rev/{rev}"), delete(unpublish_tarball))
            .route(
                &path("/{scope}/{name}/-/{filename}/-rev/{rev}"),
                delete(unpublish_scoped_tarball),
            );
    }
    router
}

// --------------------------------------------------------------------
// Path shapes. Each names only the segments its handlers read: the
// `/~<name>/` registration captures a `registry` segment too, and the
// `-rev` routes capture a revision token pnpr does not track.
// --------------------------------------------------------------------

/// A package addressed by a single segment: an unscoped name, or a scoped name
/// percent-encoded as `@scope%2Fname`.
#[derive(Deserialize)]
struct NamePath {
    name: String,
}

/// npm's overloaded two-segment address. Which package it names, and which
/// resource of it, depends on the first segment's shape and on the method, so
/// neither segment can be given a resource name here.
#[derive(Deserialize)]
struct TwoSegments {
    first: String,
    second: String,
}

#[derive(Deserialize)]
struct ScopedVersionPath {
    scope: String,
    name: String,
    version: String,
}

#[derive(Deserialize)]
struct TarballPath {
    name: String,
    filename: String,
}

#[derive(Deserialize)]
struct ScopedTarballPath {
    scope: String,
    name: String,
    filename: String,
}

#[derive(Deserialize)]
struct DistTagPath {
    name: String,
    tag: String,
}

#[derive(Deserialize)]
struct ScopePath {
    scope: String,
}

#[derive(Deserialize)]
struct TeamPath {
    scope: String,
    team: String,
}

#[derive(Deserialize)]
struct DigestPath {
    digest: String,
}

// --------------------------------------------------------------------
// Package reads — packument, version manifest, tarball.
// --------------------------------------------------------------------

/// `GET {base}/{pkg}`.
async fn get_packument(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    headers: HeaderMap,
    Path(path): Path<NamePath>,
) -> Response {
    serve_packument(&state, &identity, &headers, registry.as_deref(), &path.name).await
}

/// `GET {base}/@{scope}/{pkg}` — a scoped package's packument — or
/// `GET {base}/{pkg}/{version-or-tag}` — a version manifest for a package
/// whose name fits one segment.
async fn get_packument_or_version_manifest(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    headers: HeaderMap,
    Path(path): Path<TwoSegments>,
) -> Response {
    let TwoSegments { first, second } = path;
    if first.starts_with('@') && !first.contains('/') {
        let name = format!("{first}/{second}");
        return serve_packument(&state, &identity, &headers, registry.as_deref(), &name).await;
    }
    serve_version_manifest(&state, &identity, registry.as_deref(), &first, &second).await
}

/// `GET {base}/@{scope}/{pkg}/{version-or-tag}` — a scoped package's version
/// manifest. A first segment that is not a scope names no package.
async fn get_scoped_version_manifest(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<ScopedVersionPath>,
) -> Response {
    let ScopedVersionPath { scope, name, version } = path;
    if !scope.starts_with('@') {
        return not_found();
    }
    let full = format!("{scope}/{name}");
    serve_version_manifest(&state, &identity, registry.as_deref(), &full, &version).await
}

/// `GET {base}/{pkg}/-/{filename}`.
async fn get_tarball(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<TarballPath>,
) -> Response {
    serve_tarball(&state, &identity, registry.as_deref(), &path.name, &path.filename).await
}

/// `GET {base}/@{scope}/{pkg}/-/{filename}`.
async fn get_scoped_tarball(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<ScopedTarballPath>,
) -> Response {
    let ScopedTarballPath { scope, name, filename } = path;
    if !scope.starts_with('@') {
        return not_found();
    }
    let full = format!("{scope}/{name}");
    serve_tarball(&state, &identity, registry.as_deref(), &full, &filename).await
}

/// `GET {base}/-/tarballs/sha512/{digest}` — an integrity-addressed tarball.
async fn get_revision_tarball(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<DigestPath>,
) -> Response {
    let Some(target) = addressed_registry(&state, registry.as_deref()) else {
        return not_found();
    };
    serve_revision_tarball(&state, &identity, &target, &path.digest).await
}

/// `GET {base}/-/v1/search`.
async fn get_search(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    RawQuery(query): RawQuery,
) -> Response {
    let query = query.unwrap_or_default();
    // Results are filtered per caller (registry access plus per-package ACL),
    // so they must never land in a shared HTTP cache.
    private_no_cache(serve_search(&state, &identity, registry.as_deref(), &query).await)
}

// --------------------------------------------------------------------
// Package writes — publish, unpublish, dist-tags.
// --------------------------------------------------------------------

/// `PUT {base}/{pkg}`.
async fn put_package(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<NamePath>,
    body: axum::body::Bytes,
) -> Response {
    publish_package(&state, &identity, registry.as_deref(), &path.name, body).await
}

/// `PUT {base}/@{scope}/{pkg}` — publish a scoped package. A first segment
/// that is not a scope names no publishable package.
async fn put_scoped_package(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<TwoSegments>,
    body: axum::body::Bytes,
) -> Response {
    let TwoSegments { first, second } = path;
    if !first.starts_with('@') {
        return not_found();
    }
    let full = format!("{first}/{second}");
    publish_package(&state, &identity, registry.as_deref(), &full, body).await
}

/// `PUT {base}/{pkg}/-rev/{rev}` — the full mutated packument, which is how
/// npm spells a partial unpublish.
async fn put_packument_revision(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<NamePath>,
    body: axum::body::Bytes,
) -> Response {
    update_packument(&state, &identity, registry.as_deref(), &path.name, &body).await
}

/// `DELETE {base}/{pkg}/-rev/{rev}` — remove a whole package
/// (`pnpm unpublish --force`).
async fn unpublish_package(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<NamePath>,
) -> Response {
    delete_package(&state, &identity, registry.as_deref(), &path.name).await
}

/// `DELETE {base}/{pkg}/-/{filename}/-rev/{rev}` — remove one version's
/// tarball, a step of `pnpm unpublish <pkg>@<version>`.
async fn unpublish_tarball(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<TarballPath>,
) -> Response {
    delete_tarball(&state, &identity, registry.as_deref(), &path.name, &path.filename).await
}

/// `DELETE {base}/@{scope}/{pkg}/-/{filename}/-rev/{rev}` — remove one scoped
/// version's tarball. The unpublish flow reconstructs this URL from the
/// packument's `dist.tarball`, which spells a scoped name with a literal
/// slash.
async fn unpublish_scoped_tarball(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<ScopedTarballPath>,
) -> Response {
    let ScopedTarballPath { scope, name, filename } = path;
    if !scope.starts_with('@') {
        return not_found();
    }
    let full = format!("{scope}/{name}");
    delete_tarball(&state, &identity, registry.as_deref(), &full, &filename).await
}

/// `GET {base}/-/package/{pkg}/dist-tags`.
async fn get_package_dist_tags(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<NamePath>,
) -> Response {
    let response = get_dist_tags(&state, &identity, registry.as_deref(), &path.name).await;
    caller_scoped(&state, Ecosystem::Npm, registry.as_deref(), Some(&path.name), response)
}

/// `PUT {base}/-/package/{pkg}/dist-tags/{tag}`.
async fn put_package_dist_tag(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<DistTagPath>,
    body: axum::body::Bytes,
) -> Response {
    set_dist_tag(&state, &identity, registry.as_deref(), &path.name, &path.tag, &body).await
}

/// `DELETE {base}/-/package/{pkg}/dist-tags/{tag}`.
async fn delete_package_dist_tag(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<DistTagPath>,
) -> Response {
    remove_dist_tag(&state, &identity, registry.as_deref(), &path.name, &path.tag).await
}

// --------------------------------------------------------------------
// Orgs and teams. Membership is config-managed, so every mutation is
// rejected with an explanation rather than silently ignored.
// --------------------------------------------------------------------

/// `GET {base}/-/org/{scope}/team` — the teams of the registry claiming
/// `scope`.
async fn get_teams(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<ScopePath>,
) -> Response {
    private_no_cache(get_org_teams(&state, &identity, registry.as_deref(), &path.scope))
}

/// `GET {base}/-/org/{scope}/package` — the packages of the registry claiming
/// `scope`.
async fn get_org_package_list(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<ScopePath>,
) -> Response {
    private_no_cache(serve_org_packages(&state, &identity, registry.as_deref(), &path.scope).await)
}

/// `PUT {base}/-/org/{scope}/team`.
async fn put_team(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<ScopePath>,
) -> Response {
    reject_team_mutation(&state, &identity, registry.as_deref(), &path.scope, "create a team")
}

/// `DELETE {base}/-/team/{scope}/{team}`.
async fn delete_team(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<ScopePath>,
) -> Response {
    reject_team_mutation(&state, &identity, registry.as_deref(), &path.scope, "destroy a team")
}

/// `GET {base}/-/team/{scope}/{team}/user`.
async fn get_team_users(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<TeamPath>,
) -> Response {
    private_no_cache(get_team_members(
        &state,
        &identity,
        registry.as_deref(),
        &path.scope,
        &path.team,
    ))
}

/// `PUT {base}/-/team/{scope}/{team}/user`.
async fn put_team_user(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<ScopePath>,
) -> Response {
    reject_team_mutation(&state, &identity, registry.as_deref(), &path.scope, "add a team member")
}

/// `DELETE {base}/-/team/{scope}/{team}/user`.
async fn delete_team_user(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<ScopePath>,
) -> Response {
    reject_team_mutation(
        &state,
        &identity,
        registry.as_deref(),
        &path.scope,
        "remove a team member",
    )
}
