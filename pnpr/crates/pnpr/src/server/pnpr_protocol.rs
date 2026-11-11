use super::{
    AppState, AuthedCaller, Body, CallerOwners, Ecosystem, Response, State, StatusCode, header,
    private_no_cache,
};
use axum::{extract::Path, http::HeaderMap, response::IntoResponse};
use pnpm_shared_artifact_protocol::OwnerScope;
use pnpr_error::RegistryError;

/// Configuration-derived protocol capabilities.
pub(super) async fn serve_pnpr_handshake(State(state): State<AppState>) -> Response {
    (
        StatusCode::OK,
        axum::Json(serde_json::json!({ "pnpr": { "protocols": build_protocols_payload(&state.inner.config) } })),
    )
        .into_response()
}

pub(super) async fn serve_npm_handshake(State(state): State<AppState>) -> Response {
    let mut protocols = build_protocols_payload(&state.inner.config);
    for name in ["resolver", "artifacts", "compilerCache", "pipeline"] {
        protocols[name]["versions"] = serde_json::json!([]);
        protocols[name]["features"] = serde_json::json!([]);
    }
    protocols["resolver"]["ecosystems"] = serde_json::json!([]);
    axum::Json(serde_json::json!({ "pnpr": { "protocols": protocols } })).into_response()
}

fn protocol(enabled: bool, version: u32) -> serde_json::Value {
    let versions: Vec<u32> = enabled
        .then_some(version)
        .into_iter()
        .collect();
    serde_json::json!({ "versions": versions, "features": [], "experimental": false })
}

fn build_protocols_payload(config: &crate::Config) -> serde_json::Value {
    let resolver_enabled = config.features.resolver.enabled;
    let mut resolver = protocol(resolver_enabled, 0);
    resolver["features"] = serde_json::json!(if resolver_enabled {
        vec![
            "fixLockfile",
            "verifyLockfile",
            "projectTransforms",
            "policyEcho",
            "errorCodes",
            "acceptedLockfileFormats",
        ]
    } else {
        Vec::new()
    });
    resolver["ecosystems"] = serde_json::json!(if resolver_enabled {
        crate::resolver::resolved_ecosystems().map(Ecosystem::as_str).collect::<Vec<_>>()
    } else {
        Vec::new()
    });
    serde_json::json!({
        "resolver": resolver,
        "artifacts": experimental_protocol(config.features.artifacts.enabled),
        "compilerCache": experimental_protocol(config.features.artifacts.enabled),
        "pipeline": experimental_protocol(config.features.pipeline.enabled),
        "publish": protocol(config.features.registry.enabled, 0),
        "pnpmPublish": protocol(config.features.registry.enabled, 1),
        "records": records_protocol(config),
    })
}

fn experimental_protocol(enabled: bool) -> serde_json::Value {
    let mut capability = protocol(enabled, 0);
    capability["experimental"] = serde_json::json!(true);
    capability
}

fn records_protocol(config: &crate::Config) -> serde_json::Value {
    let records_enabled = config.features.registry.enabled
        && config.release_records.log_signing_key.is_some();
    let mut records = protocol(records_enabled, 1);
    records["manifestProfiles"] = serde_json::json!(if records_enabled {
        vec![pnpm_package_manifest::INSTALL_MANIFEST_PROFILE]
    } else {
        Vec::new()
    });
    records
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
        &state,
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
        &state,
    );
    crate::resolver::handle_verify_lockfile(runtime, identity, body).await
}

pub(super) async fn serve_publish_artifact(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    body: axum::body::Bytes,
) -> Response {
    let caller = match CallerOwners::new(&state, &identity, "shared artifact publication") {
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
    let caller = match CallerOwners::new(&state, &identity, "shared artifact lookup") {
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

type BlobPath = Path<(String, String)>;

/// `GET /-/pnpr/v0/artifacts/blobs/{organization}/{integrity}`: one stored
/// blob. A blob the caller may not read answers as if it were not stored.
pub(super) async fn serve_artifact_blob(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((organization, integrity)): BlobPath,
) -> Response {
    let caller = match CallerOwners::new(&state, &identity, "shared artifact blob") {
        Ok(caller) => caller,
        Err(err) => return private_no_cache(err.into_response()),
    };
    match state.inner.builds.artifacts
        .as_ref()
        .expect("artifact routes require an artifact store")
        .read_blob(&caller, &OwnerScope::organization(organization), &integrity)
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

/// `PUT /-/pnpr/v0/artifacts/blobs/{organization}/{integrity}`: store one
/// blob ahead of the publication that names it, streamed from the body.
/// `Content-Length` is required.
pub(super) async fn serve_upload_artifact_blob(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((organization, integrity)): BlobPath,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let caller = match CallerOwners::new(&state, &identity, "shared artifact blob upload") {
        Ok(caller) => caller,
        Err(err) => return private_no_cache(err.into_response()),
    };
    let result = async {
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
            .store_blob(
                &caller,
                &OwnerScope::organization(organization),
                &integrity,
                size,
                body.into_data_stream(),
            )
            .await
    }
    .await;
    private_no_cache(match result {
        Ok(true) => StatusCode::CREATED.into_response(),
        Ok(false) => StatusCode::OK.into_response(),
        Err(err) => err.into_response(),
    })
}

/// `HEAD /-/pnpr/v0/artifacts/blobs/{organization}/{integrity}`: whether the
/// blob is stored, so a publisher uploads only what is missing.
pub(super) async fn serve_artifact_blob_size(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((organization, integrity)): BlobPath,
) -> Response {
    let caller = match CallerOwners::new(&state, &identity, "shared artifact blob") {
        Ok(caller) => caller,
        Err(err) => return private_no_cache(err.into_response()),
    };
    let result = state.inner.builds.artifacts
        .as_ref()
        .expect("artifact routes require an artifact store")
        .blob_size(&caller, &OwnerScope::organization(organization), &integrity)
        .await;
    private_no_cache(match result {
        Ok(Some(size)) => ([(header::CONTENT_LENGTH, size.to_string())], ()).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(err) => err.into_response(),
    })
}
