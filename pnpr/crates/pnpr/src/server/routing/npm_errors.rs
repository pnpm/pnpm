use axum::{
    Json,
    body::Body,
    extract::Request,
    http::{HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use pnpr_error::PublicErrorMessage;
use serde_json::json;

use super::super::authentication::InvalidToken;

pub(in crate::server) async fn render(request: Request, next: Next) -> Response {
    if !is_npm_path(request.uri().path()) {
        return next.run(request).await;
    }
    let is_head = request.method() == Method::HEAD;
    let mut response = next.run(request).await;
    let invalid_token = response
        .extensions()
        .get::<InvalidToken>()
        .is_some();
    let message = if invalid_token {
        "Invalid authentication token".to_string()
    } else if let Some(PublicErrorMessage(message)) = response.extensions_mut().remove() {
        message
    } else {
        return response;
    };
    if response.status() == StatusCode::UNAUTHORIZED {
        let challenge = if invalid_token { r#"Bearer error="invalid_token""# } else { "Bearer" };
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static(challenge));
    }
    let json = json_error(response.status(), &message);
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response.headers_mut().remove(header::CONTENT_LENGTH);
    *response.body_mut() = if is_head { Body::empty() } else { json.into_body() };
    response
}

pub(in crate::server) fn json_error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message, "reason": message }))).into_response()
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
