use pnpm_network::LimitedBody;

pub use pnpm_text_sanitize::{sanitize, sanitize_inline};

/// Render a capped response body for an error message: lossy UTF-8,
/// sanitized, with a truncation note when the cap was hit.
pub fn body_display_string(body: &LimitedBody) -> String {
    let text = String::from_utf8_lossy(&body.bytes);
    let mut text = sanitize(&text).into_owned();
    if body.truncated {
        if !text.is_empty() && !text.chars().next_back().is_some_and(char::is_whitespace) {
            text.push(' ');
        }
        text.push_str("(response body truncated)");
    }
    text
}
