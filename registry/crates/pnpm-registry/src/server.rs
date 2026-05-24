use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::{Value, json};
use tower_http::trace::TraceLayer;

use crate::auth::{TokenStore, UpsertOutcome, UserStore, identify};
use crate::cache::Cache;
use crate::config::Config;
use crate::error::RegistryError;
use crate::package_name::PackageName;
use crate::policy::{AccessRule, PackagePolicies};
use crate::publish::{extract_attachments, merge_manifest, now_iso};
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
    cache: Cache,
    upstream: Option<Upstream>,
    config: Config,
    users: UserStore,
    tokens: TokenStore,
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
    let state = AppState {
        inner: Arc::new(AppInner {
            cache,
            upstream,
            config,
            users: UserStore::new(),
            tokens: TokenStore::new(),
        }),
    };
    Router::new()
        .route("/{name}", get(get_packument_unscoped).put(put_one_segment))
        .route("/{first}/{second}", get(get_two_segments).put(put_two_segments))
        .route("/{first}/{second}/{third}", get(get_three_segments).put(put_three_segments))
        .route("/{scope}/{name}/-/{filename}", get(get_tarball_scoped))
        .route("/{a}/{b}/{c}/{d}", get(get_four_segments).delete(delete_four_segments))
        .route(
            "/{a}/{b}/{c}/{d}/{e}",
            get(get_five_segments).put(put_five_segments).delete(delete_five_segments),
        )
        .layer(DefaultBodyLimit::max(MAX_PUBLISH_BODY_BYTES))
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
    Path((first, second, third)): Path<(String, String, String)>,
) -> Response {
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

/// 4-segment GET: `/-/package/{pkg}/dist-tags`. Returns the
/// packument's `dist-tags` object.
async fn get_four_segments(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((a, b, c, d)): Path<(String, String, String, String)>,
) -> Response {
    if a == "-" && b == "package" && d == "dist-tags" {
        return get_dist_tags(&state, &headers, &c).await;
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
async fn put_three_segments(
    State(state): State<AppState>,
    Path((first, second, third)): Path<(String, String, String)>,
    body: axum::body::Bytes,
) -> Response {
    if first == "-"
        && second == "user"
        && let Some(name) = third.strip_prefix("org.couchdb.user:")
    {
        return add_user(&state, name, &body).await;
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

/// `DELETE /{a}/{b}/{c}/{d}` — not a real npm shape; sits here so
/// the route is symmetric with PUT/GET. Returns 404.
async fn delete_four_segments(
    State(_state): State<AppState>,
    Path(_): Path<(String, String, String, String)>,
) -> Response {
    not_found()
}

/// `DELETE /-/package/{pkg}/dist-tags/{tag}` — remove a dist-tag.
async fn delete_five_segments(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((a, b, c, d, e)): Path<(String, String, String, String, String)>,
) -> Response {
    if a == "-" && b == "package" && d == "dist-tags" {
        return remove_dist_tag(&state, &headers, &c, &e).await;
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
    if let Err(err) = enforce_access(state, headers, name.as_str(), Action::Access) {
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
    if let Err(err) = enforce_access(state, headers, name.as_str(), Action::Access) {
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
    if let Err(err) = enforce_access(state, headers, name.as_str(), Action::Access) {
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
    let password = match body.get("password").and_then(Value::as_str) {
        Some(p) => p,
        None => {
            return error_response(&RegistryError::BadRequest {
                reason: "missing password".to_string(),
            });
        }
    };

    let outcome = match state.inner.users.add_or_login(name, password) {
        Ok(o) => o,
        Err(err) => return error_response(&err),
    };
    let token = state.inner.tokens.issue(name);
    let ok_msg = match outcome {
        UpsertOutcome::Created => format!("user '{name}' created"),
        UpsertOutcome::LoggedIn => format!("you are authenticated as '{name}'"),
    };
    let body = json!({ "ok": ok_msg, "token": token, "id": format!("org.couchdb.user:{name}") });
    let bytes = serde_json::to_vec(&body).expect("static-shape JSON serializes");
    Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(bytes))
        .expect("static-shape response always builds")
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
    if let Err(err) = enforce_access(state, headers, name.as_str(), Action::Publish) {
        return error_response(&err);
    }

    let mut incoming: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(err) => return error_response(&RegistryError::Json(err)),
    };
    let attachments = match extract_attachments(&mut incoming) {
        Ok(a) => a,
        Err(err) => return error_response(&err),
    };

    // Validate attachment filenames against the package name so a
    // crafted payload can't write `../../etc/passwd.tgz`.
    for attachment in &attachments {
        if let Err(err) = name.validate_tarball_name(&attachment.filename) {
            return error_response(&err);
        }
    }

    let existing_bytes = match state.inner.cache.read_packument_any_age(&name).await {
        Ok(b) => b,
        Err(err) => return error_response(&err),
    };
    let existing: Option<Value> = match existing_bytes.as_deref().map(serde_json::from_slice) {
        Some(Ok(v)) => Some(v),
        Some(Err(err)) => return error_response(&RegistryError::Json(err)),
        None => None,
    };
    let merged = merge_manifest(existing.as_ref(), &incoming, &now_iso());
    let merged_bytes = match serde_json::to_vec_pretty(&merged) {
        Ok(b) => b,
        Err(err) => return error_response(&RegistryError::Json(err)),
    };

    // Write attachments first; if the packument write succeeds but
    // we never wrote the tarball, the registry will 404 anyway.
    // Doing tarballs first avoids the symmetric race where the
    // packument advertises a tarball that isn't on disk yet.
    for attachment in attachments {
        let tmp = match state.inner.cache.open_tarball_tmp(&name, &attachment.filename).await {
            Ok(t) => t,
            Err(err) => return error_response(&err),
        };
        if let Err(err) = write_tarball_bytes(tmp, &attachment.bytes).await {
            return error_response(&err);
        }
    }

    if let Err(err) = state.inner.cache.write_packument(&name, &merged_bytes).await {
        return error_response(&err);
    }

    let body = json!({ "ok": true, "success": true });
    let bytes = serde_json::to_vec(&body).expect("static-shape JSON serializes");
    Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .expect("static-shape response always builds")
}

async fn write_tarball_bytes(
    tmp: crate::cache::TarballWrite,
    bytes: &[u8],
) -> Result<(), RegistryError> {
    use tokio::io::AsyncWriteExt;
    let mut tmp = tmp;
    tmp.file.write_all(bytes).await?;
    tmp.finalize().await
}

/// `GET /-/package/:pkg/dist-tags` — return the packument's
/// `dist-tags` object.
async fn get_dist_tags(state: &AppState, headers: &HeaderMap, raw_name: &str) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return error_response(&err),
    };
    if let Err(err) = enforce_access(state, headers, name.as_str(), Action::Access) {
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
    if let Err(err) = enforce_access(state, headers, name.as_str(), Action::Publish) {
        return error_response(&err);
    }

    // Read whatever is on disk; we need the current packument even
    // in proxy mode so the cached copy on disk gets the new tag.
    // In static mode that's the only source.
    let mut packument: Value = match state.inner.cache.read_packument_any_age(&name).await {
        Ok(Some(bytes)) => match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(err) => return error_response(&RegistryError::Json(err)),
        },
        Ok(None) => {
            // No cached packument — try to pull one from upstream
            // so first-time dist-tag changes work against a fresh
            // proxy cache.
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

    let packument_obj = match packument.as_object_mut() {
        Some(obj) => obj,
        None => {
            return error_response(&RegistryError::BadRequest {
                reason: "stored packument is not an object".to_string(),
            });
        }
    };
    let tags_entry = packument_obj
        .entry("dist-tags".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    let tags = match tags_entry.as_object_mut() {
        Some(t) => t,
        None => {
            return error_response(&RegistryError::BadRequest {
                reason: "stored dist-tags is not an object".to_string(),
            });
        }
    };
    if let Err(err) = mutate(tags) {
        return error_response(&err);
    }
    let _ = tag; // tag name is used by the mutate closure
    let new_bytes = match serde_json::to_vec_pretty(&packument) {
        Ok(b) => b,
        Err(err) => return error_response(&RegistryError::Json(err)),
    };
    if let Err(err) = state.inner.cache.write_packument(&name, &new_bytes).await {
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

/// Resolve the caller and check the per-package rule. Returns
/// `Ok(())` when the call is allowed; otherwise the appropriate
/// `Unauthenticated` / `Forbidden` error.
fn enforce_access(
    state: &AppState,
    headers: &HeaderMap,
    package: &str,
    action: Action,
) -> Result<(), RegistryError> {
    let policies: &PackagePolicies = &state.inner.config.policies;
    let effective = policies.for_package(package);
    let rule = match action {
        Action::Access => effective.access,
        Action::Publish => effective.publish,
    };
    let authenticated = identify(
        headers.get(header::AUTHORIZATION).and_then(|value| value.to_str().ok()),
        &state.inner.users,
        &state.inner.tokens,
    );
    match (rule, authenticated, action) {
        (AccessRule::All, _, Action::Access) => Ok(()),
        (AccessRule::All, _, _) => Ok(()),
        (AccessRule::Authenticated, Some(_), _) => Ok(()),
        (AccessRule::Authenticated, None, _) => {
            Err(RegistryError::Unauthenticated { resource: format!("package {package:?}") })
        }
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
