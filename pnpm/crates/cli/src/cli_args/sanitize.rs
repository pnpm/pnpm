use pacquet_network::LimitedBody;
use std::borrow::Cow;

/// Strip control characters from store-derived text before it reaches the
/// terminal, keeping `\n` and `\t`. Prevents stored metadata from emitting
/// raw escape sequences to the user's terminal.
pub fn sanitize(text: &str) -> Cow<'_, str> {
    if text.chars().any(|ch| ch.is_control() && ch != '\n' && ch != '\t') {
        Cow::Owned(
            text.chars().filter(|ch| !ch.is_control() || *ch == '\n' || *ch == '\t').collect(),
        )
    } else {
        Cow::Borrowed(text)
    }
}

/// Strip every control character from text embedded in a single-line field.
pub fn sanitize_inline(text: &str) -> Cow<'_, str> {
    if text.chars().any(char::is_control) {
        Cow::Owned(text.chars().filter(|ch| !ch.is_control()).collect())
    } else {
        Cow::Borrowed(text)
    }
}

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
