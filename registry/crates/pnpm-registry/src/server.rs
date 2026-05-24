use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use tower_http::trace::TraceLayer;

use crate::cache::Cache;
use crate::config::Config;
use crate::error::RegistryError;
use crate::package_name::PackageName;
use crate::upstream::{FetchOutcome, Upstream};

#[derive(Clone)]
struct AppState {
    inner: Arc<AppInner>,
}

struct AppInner {
    cache: Cache,
    upstream: Upstream,
    config: Config,
}

/// Build the axum [`Router`] for the registry. Exposed for tests and
/// for callers that want to drive the app without binding a TCP socket.
pub fn router(config: Config) -> Router {
    let cache = Cache::new(config.cache_dir.clone());
    let upstream = Upstream::new(config.upstream.clone(), config.public_url.clone());
    let state = AppState { inner: Arc::new(AppInner { cache, upstream, config }) };
    Router::new()
        .route("/{name}", get(get_packument_unscoped))
        .route("/{scope}/{name}", get(get_packument_scoped))
        .route("/{name}/-/{filename}", get(get_tarball_unscoped))
        .route("/{scope}/{name}/-/{filename}", get(get_tarball_scoped))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Bind to `config.listen` and serve forever.
pub async fn serve(config: Config) -> crate::error::Result<()> {
    let listen = config.listen;
    let app = router(config);
    let listener = tokio::net::TcpListener::bind(listen).await?;
    tracing::info!(%listen, "pnpm-registry listening");
    axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown signal received");
}

async fn get_packument_unscoped(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Response {
    serve_packument(&state, &name).await
}

async fn get_packument_scoped(
    State(state): State<AppState>,
    Path((scope, name)): Path<(String, String)>,
) -> Response {
    if !scope.starts_with('@') {
        return not_found();
    }
    let full = format!("{scope}/{name}");
    serve_packument(&state, &full).await
}

async fn get_tarball_unscoped(
    State(state): State<AppState>,
    Path((name, filename)): Path<(String, String)>,
) -> Response {
    serve_tarball(&state, &name, &filename).await
}

async fn get_tarball_scoped(
    State(state): State<AppState>,
    Path((scope, name, filename)): Path<(String, String, String)>,
) -> Response {
    if !scope.starts_with('@') {
        return not_found();
    }
    let full = format!("{scope}/{name}");
    serve_tarball(&state, &full, &filename).await
}

async fn serve_packument(state: &AppState, raw_name: &str) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };

    let ttl = state.inner.config.packument_ttl;
    match state.inner.cache.read_fresh_packument(&name, ttl).await {
        Ok(Some(bytes)) => return packument_response(bytes),
        Ok(None) => {}
        Err(err) => tracing::warn!(?err, package = %name.as_str(), "cache read failed"),
    }

    match state.inner.upstream.fetch_packument(&name).await {
        Ok(FetchOutcome::Ok(bytes)) => {
            if let Err(err) = state.inner.cache.write_packument(&name, &bytes).await {
                tracing::warn!(?err, package = %name.as_str(), "packument cache write failed");
            }
            packument_response(bytes)
        }
        Ok(FetchOutcome::NotFound) => not_found(),
        Err(err) => {
            tracing::warn!(?err, package = %name.as_str(), "upstream packument fetch failed");
            match state.inner.cache.read_packument_any_age(&name).await {
                Ok(Some(bytes)) => packument_response(bytes),
                Ok(None) => error_response(&err),
                Err(cache_err) => error_response(&cache_err),
            }
        }
    }
}

async fn serve_tarball(state: &AppState, raw_name: &str, filename: &str) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };
    if let Err(err) = name.validate_tarball_name(filename) {
        return error_response(&err);
    }

    match state.inner.cache.read_tarball(&name, filename).await {
        Ok(Some(bytes)) => return tarball_response(bytes),
        Ok(None) => {}
        Err(err) => {
            tracing::warn!(?err, package = %name.as_str(), %filename, "tarball cache read failed")
        }
    }

    match state.inner.upstream.fetch_tarball(&name, filename).await {
        Ok(FetchOutcome::Ok(bytes)) => {
            if let Err(err) = state.inner.cache.write_tarball(&name, filename, &bytes).await {
                tracing::warn!(?err, package = %name.as_str(), %filename, "tarball cache write failed");
            }
            tarball_response(bytes)
        }
        Ok(FetchOutcome::NotFound) => not_found(),
        Err(err) => error_response(&err),
    }
}

fn packument_response(bytes: Vec<u8>) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .expect("static-shape response always builds")
}

fn tarball_response(bytes: Vec<u8>) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(Body::from(bytes))
        .expect("static-shape response always builds")
}

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "Not Found").into_response()
}

fn error_response(err: &RegistryError) -> Response {
    let status = err.status_code();
    tracing::error!(%err, %status, "request failed");
    (status, err.to_string()).into_response()
}
