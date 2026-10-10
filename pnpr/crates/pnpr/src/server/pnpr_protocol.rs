use super::{
    AppState, AuthedCaller, Body, CallerOrgs, Ecosystem, Response, State, StatusCode, header,
    private_no_cache,
};
use axum::{
    http::{HeaderMap, Uri},
    response::IntoResponse,
};
use pnpm_shared_artifact_protocol::OwnerScope;
use pnpr_error::RegistryError;

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
    let resolver_enabled = state.inner.config.features.resolver.enabled;
    let versions = resolver_enabled
        .then_some(0)
        .into_iter()
        .collect::<Vec<_>>();
    let fix_lockfile = versions.clone();
    let ecosystems: Vec<&str> = if resolver_enabled {
        crate::resolver::resolved_ecosystems().map(Ecosystem::as_str).collect()
    } else {
        Vec::new()
    };
    let artifacts = state.inner.config.features.artifacts.enabled
        .then_some(0)
        .into_iter()
        .collect::<Vec<_>>();
    let publish = state.inner.config.features.registry.enabled
        .then_some(0)
        .into_iter()
        .collect::<Vec<_>>();
    let pipeline = state.inner.config.features.pipeline.enabled
        .then_some(0)
        .into_iter()
        .collect::<Vec<_>>();
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
    let caller = match CallerOrgs::new(&state, &identity, "shared artifact publication") {
        Ok(caller) => caller,
        Err(err) => return private_no_cache(err.into_response()),
    };
    let request = match pnpr_shared_artifacts::parse_publish(&body) {
        Ok(request) => request,
        Err(err) => return private_no_cache(err.into_response()),
    };
    private_no_cache(
        match state.inner.builds.artifacts
            .as_ref()
            .expect("artifact routes require an artifact store")
            .publish(&caller, request)
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
    let caller = match CallerOrgs::new(&state, &identity, "shared artifact lookup") {
        Ok(caller) => caller,
        Err(err) => return private_no_cache(err.into_response()),
    };
    private_no_cache(
        match state.inner.builds.artifacts
            .as_ref()
            .expect("artifact routes require an artifact store")
            .resolve(&caller, &body)
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
    let caller = match CallerOrgs::new(&state, &identity, "shared artifact blob") {
        Ok(caller) => caller,
        Err(err) => return private_no_cache(err.into_response()),
    };
    match state.inner.builds.artifacts
        .as_ref()
        .expect("artifact routes require an artifact store")
        .read_blob(&caller, &body)
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

/// `PUT /-/pnpr/v0/artifacts/blob?<owner>&integrity=<sha512-...>` — store one
/// blob ahead of the publication that names it, streamed from the body. The
/// owner is `organization=<name>` or `publisher=<package>`, and
/// `Content-Length` is required.
pub(super) async fn serve_upload_artifact_blob(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    uri: Uri,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let caller = match CallerOrgs::new(&state, &identity, "shared artifact blob upload") {
        Ok(caller) => caller,
        Err(err) => return private_no_cache(err.into_response()),
    };
    let result = async {
        let (owner, integrity) = blob_target(&uri)?;
        let size = headers
            .get(header::CONTENT_LENGTH)
            .and_then(|length| {
                length
                    .to_str()
                    .ok()?
                    .parse::<u64>()
                    .ok()
            })
            .ok_or_else(|| RegistryError::BadRequest {
                reason: "a blob upload needs a Content-Length".to_string(),
            })?;
        state.inner.builds.artifacts
            .as_ref()
            .expect("artifact routes require an artifact store")
            .store_blob(&caller, &owner, &integrity, size, body.into_data_stream())
            .await
    }
    .await;
    private_no_cache(match result {
        Ok(true) => StatusCode::CREATED.into_response(),
        Ok(false) => StatusCode::OK.into_response(),
        Err(err) => err.into_response(),
    })
}

/// `HEAD /-/pnpr/v0/artifacts/blob?<owner>&integrity=<sha512-...>` — whether
/// the blob is stored, so a publisher uploads only what is missing.
pub(super) async fn serve_artifact_blob_size(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    uri: Uri,
) -> Response {
    let caller = match CallerOrgs::new(&state, &identity, "shared artifact blob") {
        Ok(caller) => caller,
        Err(err) => return private_no_cache(err.into_response()),
    };
    let result = async {
        let (owner, integrity) = blob_target(&uri)?;
        state.inner.builds.artifacts
            .as_ref()
            .expect("artifact routes require an artifact store")
            .blob_size(&caller, &owner, &integrity)
            .await
    }
    .await;
    private_no_cache(match result {
        Ok(Some(size)) => ([(header::CONTENT_LENGTH, size.to_string())], ()).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(err) => err.into_response(),
    })
}

/// The owner and integrity a blob request's query string names.
fn blob_target(uri: &Uri) -> Result<(OwnerScope, String), RegistryError> {
    let mut owners = Vec::new();
    let mut integrity = None;
    for (key, value) in url::form_urlencoded::parse(
        uri.query()
            .unwrap_or_default()
            .as_bytes(),
    ) {
        match key.as_ref() {
            "organization" => owners.push(OwnerScope::organization(value)),
            "publisher" => owners.push(OwnerScope::Publisher { package: value.into_owned() }),
            "integrity" => integrity = Some(value.into_owned()),
            _ => {}
        }
    }
    let invalid = || RegistryError::BadRequest {
        reason: "a blob request names one owner (organization or publisher) and an integrity"
            .to_string(),
    };
    let [owner] = <[OwnerScope; 1]>::try_from(owners).map_err(|_| invalid())?;
    Ok((owner, integrity.ok_or_else(invalid)?))
}
