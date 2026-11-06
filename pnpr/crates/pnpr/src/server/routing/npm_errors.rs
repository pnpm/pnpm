use axum::{
    Json,
    body::Body,
    extract::Request,
    http::{HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use pnpr_error::PublicErrorMessage;
use pnpr_policy::Identity;
use serde_json::json;

use super::super::{AuthedCaller, authentication::token_credentials};

pub(in crate::server) async fn render(request: Request, next: Next) -> Response {
    if !is_npm_path(request.uri().path()) {
        return next.run(request).await;
    }
    let is_head = request.method() == Method::HEAD;
    let supplied_token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(token_credentials)
        .is_some();
    let mut response = next.run(request).await;
    let Some(PublicErrorMessage(mut message)) = response.extensions_mut().remove() else {
        return response;
    };
    if response.status() == StatusCode::UNAUTHORIZED {
        let invalid_token = supplied_token
            && response
                .extensions()
                .get::<AuthedCaller>()
                .is_some_and(|caller| matches!(caller.0, Identity::Anonymous));
        let challenge = if invalid_token {
            message = "Invalid authentication token".to_string();
            r#"Bearer error="invalid_token""#
        } else {
            "Bearer"
        };
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static(challenge));
    }
    let json = Json(json!({ "error": message, "reason": message })).into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response.headers_mut().remove(header::CONTENT_LENGTH);
    *response.body_mut() = if is_head { Body::empty() } else { json.into_body() };
    response
}

fn is_npm_path(path: &str) -> bool {
    if path == "/npm" || path.starts_with("/npm/") {
        return true;
    }
    let path = path
        .strip_prefix("/~")
        .and_then(|tail| tail.split_once('/').map(|(_, path)| path))
        .unwrap_or(path);
    let path = path.trim_start_matches('/');
    path == "-/whoami"
        || path.starts_with("-/user/")
        || path.starts_with("-/npm/")
        || path == "-/v1/login"
        || path.starts_with("-/v1/login/")
}
