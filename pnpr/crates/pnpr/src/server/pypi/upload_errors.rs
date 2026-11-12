use axum::response::{IntoResponse, Response};
use hyper::ext::ReasonPhrase;
use pnpr_error::RegistryError;

const MAX_REASON_BYTES: usize = 256;

pub(super) fn response(error: RegistryError) -> Response {
    let message = error.public_message();
    let mut response = error.into_response();
    response
        .headers_mut()
        .insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("text/plain; charset=utf-8"),
        );
    *response.body_mut() = axum::body::Body::from(message.clone());
    if !message.is_empty()
        && message.len() <= MAX_REASON_BYTES
        && message
            .bytes()
            .all(|byte| byte.is_ascii_graphic() || byte == b' ')
        && let Ok(reason) = ReasonPhrase::try_from(message)
    {
        response.extensions_mut().insert(reason);
    }
    response
}

#[cfg(test)]
mod tests;
