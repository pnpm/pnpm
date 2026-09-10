use super::{
    AppState, AuthedCaller, Body, Ecosystem, Response, State, StatusCode, header, private_no_cache,
    require_caller,
};
use axum::response::IntoResponse;

/// `GET /-/pnpr` — capability handshake for the pnpr resolver
/// protocol. A plain npm registry has no such route and 404s, so a
/// client can fail fast against a misconfigured server. `versions`
/// lists the `/-/pnpr/vN/resolve` protocol versions this server speaks;
/// `fixLockfile` narrows that list to versions that honor repair requests;
/// `ecosystems` names the package ecosystems `/-/pnpr/v0/resolve` accepts
/// in its request body, so a client meeting a server that does not serve
/// one of them resolves it locally rather than failing; `publish` lists the
/// `/-/pnpr/vN/publish` protocol versions, the cross-ecosystem publish
/// transaction.
pub(super) async fn serve_pnpr_handshake(State(state): State<AppState>) -> Response {
    let resolver_enabled = state.inner.config.resolver.enabled;
    let versions = resolver_enabled.then_some(0).into_iter().collect::<Vec<_>>();
    let fix_lockfile = versions.clone();
    let ecosystems: Vec<&str> = if resolver_enabled {
        crate::resolver::resolved_ecosystems().map(Ecosystem::as_str).collect()
    } else {
        Vec::new()
    };
    let artifacts =
        state.inner.config.artifacts.enabled.then_some(0).into_iter().collect::<Vec<_>>();
    let publish = state.inner.config.registry.enabled.then_some(0).into_iter().collect::<Vec<_>>();
    let pipeline = state.inner.config.pipeline.enabled.then_some(0).into_iter().collect::<Vec<_>>();
    (
        StatusCode::OK,
        axum::Json(serde_json::json!({
            "pnpr": {
                "versions": versions,
                "artifacts": artifacts,
                "pipeline": pipeline,
                "fixLockfile": fix_lockfile,
                "ecosystems": ecosystems,
                "publish": publish,
            }
        })),
    )
        .into_response()
}

/// 404 stub mounted on the capability handshake when neither pnpr protocol is
/// enabled. It prevents the registry catch-all from proxying the probe
/// upstream.
pub(super) async fn pnpr_protocols_disabled() -> Response {
    StatusCode::NOT_FOUND.into_response()
}

pub(super) async fn serve_resolve(
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

pub(super) async fn serve_verify_lockfile(
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

pub(super) async fn serve_publish_artifact(
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

pub(super) async fn serve_resolve_artifacts(
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

pub(super) async fn serve_artifact_blob(
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
