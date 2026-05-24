use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::Value;
use tower_http::trace::TraceLayer;

use crate::cache::Cache;
use crate::config::Config;
use crate::error::RegistryError;
use crate::package_name::PackageName;
use crate::streaming;
use crate::upstream::{
    FetchOutcome, Upstream, abbreviate_packument, extract_version_manifest, rewrite_tarball_urls,
};

/// MIME the npm registry uses for the abbreviated install-v1 form.
/// Matches what pacquet (and pnpm/npm/yarn) send in `Accept` when
/// resolving for an install — see pacquet's
/// `resolving-npm-resolver::ACCEPT_ABBREVIATED_DOC`. Returning the
/// full document instead bloats the wire by 2–10× on packuments with
/// long version histories.
const ABBREVIATED_CONTENT_TYPE: &str = "application/vnd.npm.install-v1+json";

#[derive(Clone)]
struct AppState {
    inner: Arc<AppInner>,
}

struct AppInner {
    cache: Cache,
    upstream: Option<Upstream>,
    config: Config,
}

/// Build the axum [`Router`] for the registry. Exposed for tests and
/// for callers that want to drive the app without binding a TCP socket.
///
/// The 2- and 3-segment routes do dispatch inside the handler rather
/// than registering overlapping parametric routes — matchit can't
/// disambiguate `/{scope}/{name}` from `/{name}/{version}` at the
/// router level, so we take both via one handler that branches on
/// the `@` prefix and the literal-`-` segment.
pub fn router(config: Config) -> Router {
    let cache = Cache::new(config.storage.clone());
    let upstream = config.upstream.as_ref().map(|base| Upstream::new(base.clone()));
    let state = AppState { inner: Arc::new(AppInner { cache, upstream, config }) };
    Router::new()
        .route("/{name}", get(get_packument_unscoped))
        .route("/{first}/{second}", get(get_two_segments))
        .route("/{first}/{second}/{third}", get(get_three_segments))
        .route("/{scope}/{name}/-/{filename}", get(get_tarball_scoped))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Bind to `config.listen` and serve forever.
pub async fn serve(config: Config) -> crate::error::Result<()> {
    let listen = config.listen;
    let app = router(config);
    let listener = NodelayTcpListener(tokio::net::TcpListener::bind(listen).await?);
    tracing::info!(%listen, "pnpm-registry listening");
    axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await?;
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

async fn get_packument_unscoped(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    serve_packument(&state, &name, wants_abbreviated(&headers)).await
}

/// `/{a}/{b}` — scoped packument when `a` starts with `@`, otherwise
/// the unscoped version manifest endpoint (`/{name}/{version-or-tag}`).
async fn get_two_segments(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((first, second)): Path<(String, String)>,
) -> Response {
    if first.starts_with('@') {
        let full = format!("{first}/{second}");
        serve_packument(&state, &full, wants_abbreviated(&headers)).await
    } else {
        serve_version_manifest(&state, &first, &second).await
    }
}

/// `/{a}/{b}/{c}` — unscoped tarball when middle is literal `-`,
/// otherwise the scoped version manifest endpoint
/// (`/{scope}/{name}/{version-or-tag}`).
async fn get_three_segments(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((first, second, third)): Path<(String, String, String)>,
) -> Response {
    if second == "-" {
        serve_tarball(&state, &first, &third).await
    } else if first.starts_with('@') {
        let _ = headers; // version-manifest endpoint always returns the full version object
        let full = format!("{first}/{second}");
        serve_version_manifest(&state, &full, &third).await
    } else {
        not_found()
    }
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

async fn serve_packument(state: &AppState, raw_name: &str, abbreviated: bool) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };
    match load_packument_bytes(state, &name).await {
        PackumentLoad::Ok(bytes) => {
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
    raw_name: &str,
    version_or_tag: &str,
) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };
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

async fn serve_tarball(state: &AppState, raw_name: &str, filename: &str) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };
    if let Err(err) = name.validate_tarball_name(filename) {
        return error_response(&err);
    }

    match state.inner.cache.open_tarball(&name, filename).await {
        Ok(Some((file, len))) => return tarball_response(streaming::stream_file(file), Some(len)),
        Ok(None) => {}
        Err(err) => {
            tracing::warn!(?err, package = %name.as_str(), %filename, "tarball cache open failed")
        }
    }

    let Some(upstream) = state.inner.upstream.as_ref() else {
        return not_found();
    };

    let response = match upstream.fetch_tarball_response(&name, filename).await {
        Ok(FetchOutcome::Ok(response)) => response,
        Ok(FetchOutcome::NotFound) => return not_found(),
        Err(err) => return error_response(&err),
    };
    let upstream_len = response.content_length();

    let write = match state.inner.cache.open_tarball_tmp(&name, filename).await {
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
    let Some(upstream) = state.inner.upstream.as_ref() else {
        return match state.inner.cache.read_packument_any_age(name).await {
            Ok(Some(bytes)) => PackumentLoad::Ok(bytes),
            Ok(None) => PackumentLoad::NotFound,
            Err(err) => PackumentLoad::Err(err),
        };
    };

    let ttl = state.inner.config.packument_ttl;
    match state.inner.cache.read_fresh_packument(name, ttl).await {
        Ok(Some(bytes)) => return PackumentLoad::Ok(bytes),
        Ok(None) => {}
        Err(err) => tracing::warn!(?err, package = %name.as_str(), "cache read failed"),
    }

    match upstream.fetch_packument(name).await {
        Ok(FetchOutcome::Ok(bytes)) => {
            if let Err(err) = state.inner.cache.write_packument(name, &bytes).await {
                tracing::warn!(?err, package = %name.as_str(), "packument cache write failed");
            }
            PackumentLoad::Ok(bytes)
        }
        Ok(FetchOutcome::NotFound) => PackumentLoad::NotFound,
        Err(err) => {
            tracing::warn!(?err, package = %name.as_str(), "upstream packument fetch failed");
            match state.inner.cache.read_packument_any_age(name).await {
                Ok(Some(bytes)) => PackumentLoad::Ok(bytes),
                Ok(None) => PackumentLoad::Err(err),
                Err(cache_err) => PackumentLoad::Err(cache_err),
            }
        }
    }
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
        let trimmed = abbreviate_packument(&doc);
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
    tracing::error!(%err, %status, "request failed");
    (status, err.to_string()).into_response()
}
