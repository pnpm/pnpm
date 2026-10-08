pub use pnpm_text_sanitize::{sanitize, sanitize_inline};

use pnpm_network::{LimitedBody, read_limited_body, redact_and_sanitize};

/// Default limit for error response bodies across registry operations (64 KiB).
pub const DEFAULT_ERROR_BODY_LIMIT: usize = 64 * 1024;

/// Render a capped response body for an error message: lossy UTF-8,
/// sanitized, with a truncation note when the cap was hit.
pub fn body_display_string(body: &LimitedBody) -> String {
    let text = String::from_utf8_lossy(&body.bytes);
    let mut text = sanitize(&text).into_owned();
    if body.truncated {
        if !text.is_empty()
            && !text
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace)
        {
            text.push(' ');
        }
        text.push_str("(response body truncated)");
    }
    text
}

/// Read a capped response body for an error report, extracting the status code,
/// canonical status text, and sanitized error body.
pub async fn read_sanitized_error_body(
    response: reqwest::Response,
    limit: usize,
) -> (reqwest::StatusCode, String, String) {
    let status = response.status();
    let status_text = status
        .canonical_reason()
        .unwrap_or_default()
        .to_string();
    let body = match read_limited_body(response, limit).await {
        Ok(body) => redact_and_sanitize(&body_display_string(&body)),
        Err(_) => String::new(),
    };
    (status, status_text, body)
}

#[cfg(test)]
mod tests;
