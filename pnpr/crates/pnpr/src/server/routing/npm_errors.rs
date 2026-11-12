use axum::{
    body::Body,
    extract::Request,
    http::{HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::Response,
};
use pnpr_error::{PublicErrorCode, PublicErrorMessage};

use super::super::authentication::InvalidToken;

pub(in crate::server) async fn render(request: Request, next: Next) -> Response {
    let path = request.uri().path();
    let is_npm = is_npm_path(path);
    let is_pnpr = path.contains("/-/pnpr/") || path.ends_with("/-/pnpr");
    if !is_npm && !is_pnpr {
        return next.run(request).await;
    }
    let is_head = request.method() == Method::HEAD;
    let mut response = next.run(request).await;
    let invalid_token = is_npm
        && response
            .extensions()
            .get::<InvalidToken>()
            .is_some();
    let message = if invalid_token {
        Some("Invalid authentication token".to_string())
    } else {
        error_message(&mut response, is_pnpr).await
    };
    let Some(message) = message else {
        return response;
    };
    if is_npm {
        set_authentication_challenge(&mut response, invalid_token);
    }
    let code = response
        .extensions()
        .get::<PublicErrorCode>()
        .map(|code| code.0.as_str());
    let json = pnpr_error::json_error(response.status(), &message, code);
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response.headers_mut().remove(header::CONTENT_LENGTH);
    *response.body_mut() = if is_head { Body::empty() } else { json.into_body() };
    response
}

async fn error_message(response: &mut Response, is_pnpr: bool) -> Option<String> {
    if let Some(PublicErrorMessage(message)) = response.extensions_mut().remove() {
        return Some(message);
    }
    let is_plain_text = response
        .headers()
        .get(header::CONTENT_TYPE)
        .is_none_or(|value| value.as_bytes().starts_with(b"text/plain"));
    let is_error = response.status().is_client_error() || response.status().is_server_error();
    if is_pnpr && is_error && is_plain_text {
        Some(framework_error_message(response).await)
    } else {
        None
    }
}

fn set_authentication_challenge(response: &mut Response, invalid_token: bool) {
    if response.status() == StatusCode::UNAUTHORIZED {
        let challenge = if invalid_token { r#"Bearer error="invalid_token""# } else { "Bearer" };
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static(challenge));
    }
}

pub(in crate::server) fn json_error(status: StatusCode, message: &str) -> Response {
    pnpr_error::json_error(status, message, None)
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

async fn framework_error_message(response: &mut Response) -> String {
    let fallback = response
        .status()
        .canonical_reason()
        .unwrap_or("Request failed");
    if response.status().is_server_error() {
        return fallback.to_string();
    }
    let body = std::mem::replace(response.body_mut(), Body::empty());
    match axum::body::to_bytes(body, 64 * 1024).await {
        Ok(bytes) if !bytes.is_empty() => String::from_utf8_lossy(&bytes).into_owned(),
        _ => fallback.to_string(),
    }
}
