pub(super) use super::{
    Action, AppInner, AppState, AuthedCaller, CanonicalPackageName, MAX_PUBLISH_BODY_BYTES,
    MAX_TEAM_BODY_BYTES, RegistrySource, TargetRegistry, addressed_registry, authenticate,
    authorize_package, batch, caller_scoped, cargo, check_publish_tombstone, delete_package,
    delete_tarball, get_dist_tags, get_org_teams, get_team_members,
    health::{serve_live, serve_ping, serve_ready},
    hosted_storage, not_found, oci, pnpr_protocols_disabled, private_no_cache, publish_package,
    pypi, remove_dist_tag, resolve_write_target, serve_batch_publish, serve_metrics,
    serve_org_packages, serve_package_tarball, serve_packument, serve_pnpr_handshake,
    serve_revision_tarball, serve_search, serve_tarball, serve_version_manifest, set_dist_tag,
    split_package_identifier, staged,
    team_access::{grant_team_access, list_team_packages, revoke_team_access},
    team_mutations::{TeamScope, add_team_member, create_team, destroy_team, remove_team_member},
    update_packument,
};

mod account_handlers;
use account_handlers::account_routes;

mod protocol_routes;
use protocol_routes::{artifacts_routes, pipeline_routes, resolver_routes};

mod team_handlers;
use team_handlers::team_routes;

mod npm_handlers;
use npm_handlers::{
    delete_package_dist_tag, get_package_dist_tags, get_packument,
    get_packument_or_version_manifest, get_revision_tarball, get_scoped_tarball,
    get_scoped_version_manifest, get_search, get_tarball, put_package, put_package_dist_tag,
    put_packument_revision, put_scoped_package, put_scoped_packument_revision, unpublish_package,
    unpublish_scoped_package, unpublish_scoped_tarball, unpublish_tarball,
};

mod observability;
use observability::with_observability_layers;

use std::sync::Arc;

use axum::{
    Router,
    extract::{DefaultBodyLimit, Path, RawQuery, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{any, delete, get, post, put},
};
use tower_http::cors::{AllowOrigin, CorsLayer};

use pnpr_auth::AuthState;
use pnpr_config::Config;
use pnpr_registry::Ecosystem;
use serde::Deserialize;

pub(super) fn router_with_auth_and_osv(
    config: Config,
    auth: AuthState,
    osv_index: Option<Arc<pnpr_osv::OsvIndex>>,
) -> pnpr_error::Result<Router> {
    let cors_origins = cors_origins(&config)?;
    let surfaces = EnabledSurfaces {
        resolver: config.features.resolver.enabled,
        registry: config.features.registry.enabled,
        artifacts: config.features.artifacts.enabled,
        pipeline: config.features.pipeline.enabled,
    };
    let ui_dir = if surfaces.registry { super::web_ui::locate(&config.http.ui)? } else { None };
    let state = AppState { inner: Arc::new(AppInner::new(config, auth, osv_index)?) };
    finish_router(state, surfaces, cors_origins, ui_dir.as_deref())
}

fn finish_router(
    state: AppState,
    surfaces: EnabledSurfaces,
    cors_origins: Vec<HeaderValue>,
    ui_dir: Option<&std::path::Path>,
) -> pnpr_error::Result<Router> {
    let mut router = surface_routes(&state, surfaces, ui_dir.is_some());
    if let Some(dir) = ui_dir {
        tracing::info!(dir = %dir.display(), "serving the web UI at /-/ui/");
        router = router.merge(super::web_ui::routes(dir));
    }
    let router = router
        .layer(DefaultBodyLimit::max(MAX_PUBLISH_BODY_BYTES))
        // Authenticate once, ahead of every handler: resolve the caller,
        // enforce bearer-token read-only / CIDR restrictions (so a
        // restricted token is rejected before a write handler buffers its
        // up-to-100-MiB body), and stash the identity for handlers to read.
        // Inside the trace layer below, so a rejection is still one record.
        .layer(axum::middleware::from_fn_with_state(state.clone(), authenticate));
    let mut router = apply_request_timeout(router, state.inner.config.http.request_timeout);
    if !cors_origins.is_empty() {
        router = router.layer(
            CorsLayer::new()
                .allow_origin(AllowOrigin::list(cors_origins))
                .allow_methods([
                    Method::GET,
                    Method::HEAD,
                    Method::OPTIONS,
                    Method::PUT,
                    Method::POST,
                    Method::DELETE,
                ])
                .allow_headers([
                    header::AUTHORIZATION,
                    header::ACCEPT,
                    header::CONTENT_TYPE,
                    header::IF_MATCH,
                ])
                .expose_headers([header::ETAG]),
        );
    }
    Ok(with_observability_layers(router, state.clone()).with_state(state))
}

fn apply_request_timeout(
    router: Router<AppState>,
    timeout_duration: Option<std::time::Duration>,
) -> Router<AppState> {
    let Some(timeout_duration) = timeout_duration else {
        return router;
    };
    router.layer(axum::middleware::from_fn(
        move |request: axum::extract::Request, next: axum::middleware::Next| async move {
            match tokio::time::timeout(timeout_duration, next.run(request)).await {
                Ok(response) => response,
                Err(_) => (
                    StatusCode::GATEWAY_TIMEOUT,
                    [(header::CONTENT_TYPE, "application/json")],
                    r#"{"error":"request timed out"}"#,
                )
                    .into_response(),
            }
        },
    ))
}

fn cors_origins(config: &Config) -> pnpr_error::Result<Vec<HeaderValue>> {
    config.http.cors
        .allowed_origins()
        .iter()
        .map(|origin| {
            HeaderValue::from_str(origin)
                .map_err(|_| pnpr_error::RegistryError::InvalidConfig {
                    reason: format!(
                        "CORS allowed origin {origin:?} is not a valid HTTP header value",
                    ),
                })
        })
        .collect()
}

/// Which of the configurable surfaces this server mounts.
#[derive(Clone, Copy)]
struct EnabledSurfaces {
    resolver: bool,
    registry: bool,
    artifacts: bool,
    pipeline: bool,
}

fn surface_routes(
    state: &AppState,
    surfaces: EnabledSurfaces,
    ui_served: bool,
) -> Router<AppState> {
    // Health and probe endpoints (`/-/ping`, `/-/ready`, `/-/live`) are always served.
    // The configurable surfaces are mounted only when their feature is enabled,
    // so resolver, registry, and artifacts can be deployed independently.
    // The config guarantees at least one is enabled.
    let mut router: Router<AppState> = Router::new();
    for base in ["", "/~{registry}"] {
        let path = |tail: &str| format!("{base}{tail}");
        router = router
            .route(&path("/-/ping"), get(serve_ping))
            .route(&path("/-/ready"), get(serve_ready))
            .route(&path("/-/live"), get(serve_live))
            .route(&path("/-/metrics"), get(serve_metrics))
            .route(&path("/metrics"), get(serve_metrics));
    }
    router = router
        .merge(super::oidc::routes(ui_served))
        .merge(account_routes())
        .merge(super::user_admin::user_admin_routes())
        .merge(super::rule_admin::rule_admin_routes());
    if state.inner.config.identity.auth.scim.is_some() {
        router = router.merge(super::scim::scim_routes());
    }
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
    let registries = &state.inner.config.routing.registries;
    let npm = npm_registry_routes();
    // One publish transaction for packages of any ecosystem. It answers here
    // rather than inside the npm surface.
    let mut router = router
        .route("/-/pnpr/v0/publish", put(batch::serve_ecosystem_publish))
        .route("/-/pnpr/v0/registries", get(super::registry_directory::serve));
    if registries.has_ecosystem(Ecosystem::Npm) {
        router = router.nest(&Ecosystem::Npm.base_path(), account_routes().merge(npm));
    }
    if registries.has_ecosystem(Ecosystem::Cargo) {
        router = router.merge(cargo::routes());
    }
    if registries.has_ecosystem(Ecosystem::Pypi) {
        router = router.merge(pypi::routes());
    }
    if registries.has_ecosystem(Ecosystem::Oci) {
        router = router.merge(oci::routes());
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
        let with_teams = team_routes(staged_routes(ping_route(router, base), base), base);
        router = super::security_audit::routes(with_teams, base)
            .route(&path("/-/v1/search"), get(get_search))
            .route(&path("/-/npm/v1/keys"), get(super::attestations::get_keys))
            .route(
                &path("/-/npm/v1/attestations/{*spec}"),
                get(super::attestations::get_attestations),
            )
            .route(&path("/-/tarballs/sha512/{digest}"), get(get_revision_tarball))
            .route(&path("/-/package/{name}/dist-tags"), get(get_package_dist_tags))
            .route(
                &path("/-/package/{name}/dist-tags/{tag}"),
                put(put_package_dist_tag).delete(delete_package_dist_tag),
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
            .route(
                &path("/{scope}/{name}/-rev/{rev}"),
                put(put_scoped_packument_revision).delete(unpublish_scoped_package),
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

/// `npm ping` asks the registry URL, not the server root.
fn ping_route(router: Router<AppState>, base: &str) -> Router<AppState> {
    router.route(&format!("{base}/-/ping"), get(serve_ping))
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
