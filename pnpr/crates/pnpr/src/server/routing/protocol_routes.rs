use axum::{
    Router,
    extract::DefaultBodyLimit,
    middleware,
    routing::{any, get, post, put},
};

use crate::server::{
    AppState, MAX_ARTIFACT_BLOB_BODY_BYTES, MAX_ARTIFACT_PUBLISH_BODY_BYTES,
    MAX_ARTIFACT_RESOLVE_BODY_BYTES, MAX_PIPELINE_RUN_BODY_BYTES, compiler_cache,
    require_artifact_caller, require_pipeline_caller, require_resolver_caller, serve_artifact_blob,
    serve_get_pipeline_run, serve_list_pipeline_runs, serve_pipeline_ui, serve_publish_artifact,
    serve_publish_pipeline_run, serve_resolve, serve_resolve_artifacts, serve_verify_lockfile,
    staged_archives::{serve_head_blob, serve_put_blob},
};

pub(super) fn resolver_routes(state: &AppState, router: Router<AppState>) -> Router<AppState> {
    router
        .route(
            "/-/pnpr/v0/resolve",
            post(serve_resolve)
                .route_layer(middleware::from_fn_with_state(
                    state.clone(),
                    require_resolver_caller,
                )),
        )
        .route(
            "/-/pnpr/v0/verify-lockfile",
            post(serve_verify_lockfile)
                .route_layer(middleware::from_fn_with_state(
                    state.clone(),
                    require_resolver_caller,
                )),
        )
}

pub(super) fn artifacts_routes(state: &AppState, router: Router<AppState>) -> Router<AppState> {
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
        // The limit bounds the lookup a `POST` sends, and the layers apply to
        // it alone. A `PUT` streams its body past the limit, and the store
        // holds that to the declared size. A `PUT` or `HEAD` naming no owner
        // stages a release archive instead, which the registry surface
        // serves with the registry's own credentials; both handlers identify
        // their caller themselves.
        .route(
            "/-/pnpr/v0/artifacts/blob",
            post(serve_artifact_blob)
                .route_layer(DefaultBodyLimit::max(MAX_ARTIFACT_BLOB_BODY_BYTES))
                .route_layer(middleware::from_fn_with_state(state.clone(), require_artifact_caller))
                .put(serve_put_blob)
                .head(serve_head_blob),
        )
}

pub(super) fn pipeline_routes(state: &AppState, router: Router<AppState>) -> Router<AppState> {
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
            "/-/pnpr/v0/pipeline/runs/{org}/{workspace}/{run_id}",
            get(serve_get_pipeline_run)
                .route_layer(middleware::from_fn_with_state(
                    state.clone(),
                    require_pipeline_caller,
                )),
        )
        // The viewer page is static HTML with no data of its own; the
        // reads it issues are what authenticate.
        .route("/-/pnpr/v0/pipeline", get(serve_pipeline_ui))
}

fn compiler_cache_routes(state: &AppState, router: Router<AppState>) -> Router<AppState> {
    router
        .route("/-/pnpr/v0/compiler-cache/{cache}/", any(compiler_cache::directory))
        .route(
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

/// Release archive staging on a server without the artifact surface, which
/// otherwise serves the route (see [`artifacts_routes`]).
pub(super) fn staged_archive_routes(router: Router<AppState>) -> Router<AppState> {
    staged_archive_routes_under(router, "")
}

/// Release archive staging under `base`. A staged archive belongs to its
/// principal whichever base it is uploaded through.
pub(super) fn staged_archive_routes_under(
    router: Router<AppState>,
    base: &str,
) -> Router<AppState> {
    router.route(
        &format!("{base}/-/pnpr/v0/artifacts/blob"),
        put(serve_put_blob).head(serve_head_blob),
    )
}
