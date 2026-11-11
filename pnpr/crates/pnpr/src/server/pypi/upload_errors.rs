use axum::response::{IntoResponse, Response};
use hyper::ext::ReasonPhrase;
use pnpr_error::RegistryError;

const MAX_REASON_BYTES: usize = 256;

pub(super) fn response(error: RegistryError) -> Response {
    let message = error.public_message();
    let mut response = error.into_response();
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
