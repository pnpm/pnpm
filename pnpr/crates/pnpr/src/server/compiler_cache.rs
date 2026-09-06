use axum::{
    body::Bytes,
    extract::{Path, Request, State},
    http::{Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse as _, Response},
};
use pnpr_error::RegistryError;
use pnpr_policy::Identity;
use pnpr_shared_artifacts::CompilerCacheKey;
use serde::Deserialize;

use super::{AppState, AuthedCaller, private_no_cache, require_caller};

pub(super) async fn read(
    State(state): State<AppState>,
    Path((cache, key)): Path<(String, String)>,
) -> Response {
    let result = async {
        let key = CompilerCacheKey::try_from(key)?;
        state
            .inner
            .artifacts
            .as_ref()
            .expect("compiler cache routes require an artifact store")
            .read_compiler_cache(&cache, &key)
            .await
    }
    .await;
    private_no_cache(match result {
        Ok(Some(bytes)) => {
            ([(header::CONTENT_TYPE, "application/octet-stream")], bytes).into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error.into_response(),
    })
}

pub(super) async fn write(
    State(state): State<AppState>,
    Path((cache, key)): Path<(String, String)>,
    bytes: Bytes,
) -> Response {
    let result = async {
        let key = CompilerCacheKey::try_from(key)?;
        state
            .inner
            .artifacts
            .as_ref()
            .expect("compiler cache routes require an artifact store")
            .publish_compiler_cache(&cache, &key, bytes)
            .await
    }
    .await;
    private_no_cache(match result {
        Ok(true) => StatusCode::CREATED.into_response(),
        Ok(false) => StatusCode::OK.into_response(),
        Err(error) => error.into_response(),
    })
}

pub(super) async fn head(
    State(state): State<AppState>,
    Path((cache, key)): Path<(String, String)>,
) -> Response {
    let result = async {
        let key = CompilerCacheKey::try_from(key)?;
        state
            .inner
            .artifacts
            .as_ref()
            .expect("compiler cache routes require an artifact store")
            .compiler_cache_size(&cache, &key)
            .await
    }
    .await;
    private_no_cache(match result {
        Ok(Some(size)) => (
            [
                (header::CONTENT_LENGTH, size.to_string()),
                (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            ],
            (),
        )
            .into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error.into_response(),
    })
}

pub(super) async fn authorize_request(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((cache, key)): Path<(String, String)>,
    request: Request,
    next: Next,
) -> Response {
    let key = if request.method().as_str() == "PROPFIND" {
        key.trim_end_matches('/').to_string()
    } else {
        key
    };
    let authorization = authorize(&state, &identity, &cache, request.method() == Method::PUT)
        .and_then(|()| CompilerCacheKey::try_from(key));
    if let Err(error) = authorization {
        return private_no_cache(error.into_response());
    }
    let _upload = if request.method() == Method::PUT {
        match state.inner.compiler_cache_uploads.try_acquire() {
            Ok(permit) => Some(permit),
            Err(_) => {
                return private_no_cache(
                    (StatusCode::SERVICE_UNAVAILABLE, [(header::RETRY_AFTER, "1")]).into_response(),
                );
            }
        }
    } else {
        None
    };
    private_no_cache(next.run(request).await)
}

#[derive(Deserialize)]
pub(super) struct CollectionPath {
    cache: String,
    key: Option<String>,
}

pub(super) async fn directory(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(path): Path<CollectionPath>,
    method: Method,
) -> Response {
    if let Err(error) = authorize(&state, &identity, &path.cache, false) {
        return private_no_cache(error.into_response());
    }
    if method.as_str() != "PROPFIND" {
        return private_no_cache(StatusCode::METHOD_NOT_ALLOWED.into_response());
    }
    if let Some(key) = &path.key {
        if !key.ends_with('/') {
            return private_no_cache(StatusCode::NOT_FOUND.into_response());
        }
        if let Err(error) = CompilerCacheKey::try_from(key.trim_end_matches('/').to_string()) {
            return private_no_cache(error.into_response());
        }
    }
    // sccache probes parents before PUT. Collections are virtual; only entries
    // occupy storage, so probing one must not create a directory or consume quota.
    let cache: String = url::form_urlencoded::byte_serialize(path.cache.as_bytes()).collect();
    let href = format!("/-/pnpr/v0/compiler-cache/{cache}/{}", path.key.unwrap_or_default());
    private_no_cache((
        StatusCode::MULTI_STATUS,
        [(header::CONTENT_TYPE, "application/xml")],
        format!(r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:"><d:response><d:href>{href}</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype><d:getlastmodified>Thu, 01 Jan 1970 00:00:00 GMT</d:getlastmodified></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>"#),
    ).into_response())
}

fn authorize(
    state: &AppState,
    identity: &Identity,
    cache: &str,
    publish: bool,
) -> Result<(), RegistryError> {
    let username = require_caller(identity, "compiler cache")?;
    let policy = state.inner.config.artifacts.compiler_caches.get(cache);
    if !policy.is_some_and(|policy| policy.access.allows(identity)) {
        return Err(RegistryError::NotFound);
    }
    if publish && !policy.is_some_and(|policy| policy.publish.allows(identity)) {
        return Err(RegistryError::Forbidden {
            user: username,
            action: "publish to compiler cache",
            resource: cache.to_string(),
        });
    }
    Ok(())
}
