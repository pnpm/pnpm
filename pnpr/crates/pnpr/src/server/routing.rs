mod team_handlers;
use team_handlers::{
    delete_team, delete_team_user, get_org_package_list, get_team_users, get_teams, put_team,
    put_team_user,
};

mod npm_handlers;
use npm_handlers::{
    delete_package_dist_tag, get_package_dist_tags, get_packument,
    get_packument_or_version_manifest, get_revision_tarball, get_scoped_tarball,
    get_scoped_version_manifest, get_search, get_tarball, put_package, put_package_dist_tag,
    put_packument_revision, put_scoped_package, unpublish_package, unpublish_scoped_tarball,
    unpublish_tarball,
};

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
    MAX_PIPELINE_RUN_BODY_BYTES, MAX_PUBLISH_BODY_BYTES, StripedLocks, TargetRegistry,
    addressed_registry, authenticate, batch, caller_scoped, cargo, compiler_cache,
    compute_upstream_cache_namespace, delete_package, delete_session_token, delete_tarball,
    delete_token_by_key, get_dist_tags, get_org_teams, get_profile, get_team_members,
    get_token_list, get_whoami, loggable_uri, not_found, oci, pnpr_protocols_disabled,
    private_no_cache, publish_package, put_login, pypi, reject_team_mutation, remove_dist_tag,
    require_artifact_caller, require_pipeline_caller, require_resolver_caller, serve_artifact_blob,
    serve_batch_publish, serve_get_pipeline_run, serve_list_pipeline_runs, serve_org_packages,
    serve_packument, serve_ping, serve_pipeline_ui, serve_pnpr_handshake, serve_publish_artifact,
    serve_publish_pipeline_run, serve_resolve, serve_resolve_artifacts, serve_revision_tarball,
    serve_search, serve_tarball, serve_verify_lockfile, serve_version_manifest, set_dist_tag,
    staged, update_packument,
};

pub(super) fn router_with_auth_and_osv(
    config: Config,
    auth: AuthState,
    osv_index: Option<Arc<pnpr_osv::OsvIndex>>,
) -> pnpr_error::Result<Router> {
    let cors_origins = cors_origins(&config)?;
    let storage =
        Storage::new(&config.hosted_store, config.storage.clone(), config.cache_storage.clone())?;
    let surfaces = EnabledSurfaces {
        resolver: config.resolver.enabled,
        registry: config.registry.enabled,
        artifacts: config.artifacts.enabled,
        pipeline: config.pipeline.enabled,
    };
    let pipeline_runs =
        surfaces.pipeline.then(|| pnpr_pipeline_runs::PipelineRunStore::new(storage.clone()));
    let artifacts = artifact_store(&config, surfaces.artifacts)?;
    // Only the registry routes consult the upstreams, so a resolver-only
    // server builds none — skipping a `ThrottledClient` allocation per
    // configured upstream.
    let upstreams = upstream_clients(&config, surfaces.registry);
    let upstream_cache_namespaces = config
        .upstreams
        .keys()
        .map(|name| (name.clone(), compute_upstream_cache_namespace(&config, name)))
        .collect();
    super::oidc::validate_workloads(&config)?;
    let oidc = pnpr_auth::oidc::OidcState::new(&config.auth.oidc, &config.public_url)?;
    let state = AppState {
        inner: Arc::new(AppInner {
            storage,
            artifacts,
            compiler_cache_uploads: tokio::sync::Semaphore::new(2),
            pipeline_runs,
            upstreams,
            upstream_cache_namespaces,
            config,
            auth,
            oidc,
            package_locks: StripedLocks::new(),
            referrer_migration_locks: StripedLocks::new(),
            resolver: std::sync::OnceLock::new(),
            osv_index,
        }),
    };
    finish_router(state, surfaces, cors_origins)
}

fn finish_router(
    state: AppState,
    surfaces: EnabledSurfaces,
    cors_origins: Vec<HeaderValue>,
) -> pnpr_error::Result<Router> {
    let router = surface_routes(&state, surfaces);
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
    Ok(with_observability_layers(router).with_state(state))
}

fn cors_origins(config: &Config) -> pnpr_error::Result<Vec<HeaderValue>> {
    config
        .cors
        .allowed_origins()
        .iter()
        .map(|origin| {
            HeaderValue::from_str(origin).map_err(|_| pnpr_error::RegistryError::InvalidConfig {
                reason: format!("CORS allowed origin {origin:?} is not a valid HTTP header value"),
            })
        })
        .collect()
}

/// Only the registry routes consult the upstreams, so a resolver-only
/// server builds none, skipping a `ThrottledClient` allocation per
/// configured upstream.
fn upstream_clients(config: &Config, registry_enabled: bool) -> IndexMap<String, Upstream> {
    if !registry_enabled {
        return IndexMap::new();
    }
    config
        .upstreams
        .iter()
        .map(|(name, upstream)| {
            let client = Upstream::new(name, upstream);
            let client = if config.registries.ecosystem(name) == Some(Ecosystem::Npm) {
                client
            } else {
                client.with_fetch_guard(super::ecosystem::upstream_fetch_guard(config, upstream))
            };
            (name.clone(), client)
        })
        .collect()
}

/// The compression and access-log layers every response passes through.
fn with_observability_layers(router: Router<AppState>) -> Router<AppState> {
    router
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
}

/// Which of the configurable surfaces this server mounts.
#[derive(Clone, Copy)]
struct EnabledSurfaces {
    resolver: bool,
    registry: bool,
    artifacts: bool,
    pipeline: bool,
}

fn surface_routes(state: &AppState, surfaces: EnabledSurfaces) -> Router<AppState> {
    // `/-/ping` is a health check and is always served. The two
    // configurable surfaces are mounted only when their feature is enabled,
    // so resolver, registry, and artifacts can be deployed independently.
    // The config guarantees at least one is enabled.
    let mut router: Router<AppState> = Router::new()
        .route("/-/ping", get(serve_ping))
        .route("/-/oidc/{provider}/login", get(super::oidc::login))
        .route("/-/oidc/{provider}/callback", get(super::oidc::callback));
    router = router.merge(account_routes());
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
    if surfaces.resolver || surfaces.artifacts || surfaces.registry || surfaces.pipeline {
        router = router.route("/-/pnpr", get(serve_pnpr_handshake));
    } else {
        router = router.route("/-/pnpr", any(pnpr_protocols_disabled));
    }
    if surfaces.resolver {
        router = resolver_routes(state, router);
    }
    if surfaces.artifacts {
        router = artifacts_routes(state, router);
    }
    if surfaces.pipeline {
        router = pipeline_routes(state, router);
    }
    if surfaces.registry {
        router = registry_routes(state, router);
    }

    router
}

/// Mount the npm-compatible registry surface and every other ecosystem the
/// configuration declares.
fn registry_routes(state: &AppState, router: Router<AppState>) -> Router<AppState> {
    let registries = &state.inner.config.registries;
    let npm = npm_registry_routes();
    // One publish transaction for packages of any ecosystem. It answers here
    // rather than inside the npm surface.
    let mut router = router
        .route("/-/pnpr/v0/publish", put(batch::serve_ecosystem_publish))
        .route("/-/pnpr/v0/registries", get(super::registry_directory::serve));
    if registries.is_only_ecosystem(Ecosystem::Npm) {
        router = router.merge(npm);
    } else {
        router = router.nest("/npm", account_routes().merge(npm));
    }
    if registries.has_ecosystem(Ecosystem::Cargo) {
        router = router.merge(cargo::routes(!registries.is_only_ecosystem(Ecosystem::Cargo)));
    }
    if registries.has_ecosystem(Ecosystem::Pypi) {
        router = router.merge(pypi::routes(!registries.is_only_ecosystem(Ecosystem::Pypi)));
    }
    // The image surface keeps `/v2/` at the host root whatever else is served,
    // because a client derives it from the image reference's host and cannot be
    // pointed at a prefix.
    if registries.has_ecosystem(Ecosystem::Oci) {
        router = router.merge(oci::routes(!registries.is_only_ecosystem(Ecosystem::Oci)));
    }
    router
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
        router = staged_routes(router, base)
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

fn resolver_routes(state: &AppState, router: Router<AppState>) -> Router<AppState> {
    router
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
        )
}

fn artifacts_routes(state: &AppState, router: Router<AppState>) -> Router<AppState> {
    compiler_cache_routes(state, router)
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
        )
}

fn pipeline_routes(state: &AppState, router: Router<AppState>) -> Router<AppState> {
    router
        .route(
            "/-/pnpr/v0/pipeline/runs",
            put(serve_publish_pipeline_run)
                .get(serve_list_pipeline_runs)
                .route_layer(DefaultBodyLimit::max(MAX_PIPELINE_RUN_BODY_BYTES))
                .route_layer(middleware::from_fn_with_state(
                    state.clone(),
                    require_pipeline_caller,
                )),
        )
        .route(
            "/-/pnpr/v0/pipeline/runs/{workspace}/{run_id}",
            get(serve_get_pipeline_run).route_layer(middleware::from_fn_with_state(
                state.clone(),
                require_pipeline_caller,
            )),
        )
        // The viewer page is static HTML with no data of its own; the
        // reads it issues are what authenticate.
        .route("/-/pnpr/v0/pipeline", get(serve_pipeline_ui))
}

fn compiler_cache_routes(state: &AppState, router: Router<AppState>) -> Router<AppState> {
    router.route("/-/pnpr/v0/compiler-cache/{cache}/", any(compiler_cache::directory)).route(
        "/-/pnpr/v0/compiler-cache/{cache}/{*key}",
        get(compiler_cache::read)
            .head(compiler_cache::head)
            .put(compiler_cache::write)
            .fallback(compiler_cache::directory)
            .route_layer(DefaultBodyLimit::max(
                pnpr_shared_artifacts::MAX_COMPILER_CACHE_ENTRY_SIZE,
            ))
            .route_layer(middleware::from_fn_with_state(
                state.clone(),
                compiler_cache::authorize_request,
            )),
    )
}

fn staged_routes(router: Router<AppState>, base: &str) -> Router<AppState> {
    let path = |tail: &str| format!("{base}{tail}");
    router
        .route(&path("/-/stage"), get(staged::list_staged))
        .route(&path("/-/stage/package/{name}"), post(staged::post_staged_publish))
        .route(&path("/-/stage/{id}"), get(staged::get_staged).delete(staged::reject_staged))
        .route(&path("/-/stage/{id}/approve"), post(staged::approve_staged))
        .route(&path("/-/stage/{id}/tarball"), get(staged::get_staged_tarball))
}

fn artifact_store(
    config: &Config,
    enabled: bool,
) -> pnpr_error::Result<Option<pnpr_shared_artifacts::SharedArtifactStore>> {
    enabled
        .then(|| {
            pnpr_shared_artifacts::SharedArtifactStore::new(
                &config.hosted_store,
                &config.cache_storage,
            )
        })
        .transpose()
}
