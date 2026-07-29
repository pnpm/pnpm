use pacquet_network::LimitedBody;
use std::borrow::Cow;

#[cfg(test)]
mod tests;

/// Strip control and formatting characters from store-derived text before it
/// reaches the terminal, keeping `\n` and `\t`.
pub fn sanitize(text: &str) -> Cow<'_, str> {
    if text.chars().any(|ch| is_format_character(ch) || ch.is_control() && ch != '\n' && ch != '\t')
    {
        Cow::Owned(
            text.chars()
                .filter(|ch| {
                    !is_format_character(*ch) && (!ch.is_control() || *ch == '\n' || *ch == '\t')
                })
                .collect(),
        )
    } else {
        Cow::Borrowed(text)
    }
}

/// Strip every control and formatting character from text embedded in a
/// single-line field.
pub fn sanitize_inline(text: &str) -> Cow<'_, str> {
    if text.chars().any(|ch| ch.is_control() || is_format_character(ch)) {
        Cow::Owned(
            text.chars().filter(|ch| !ch.is_control() && !is_format_character(*ch)).collect(),
        )
    } else {
        Cow::Borrowed(text)
    }
}

fn is_format_character(ch: char) -> bool {
    matches!(
        ch,
        '\u{00AD}'
            | '\u{0600}'..='\u{0605}'
            | '\u{061C}'
            | '\u{06DD}'
            | '\u{070F}'
            | '\u{0890}'..='\u{0891}'
            | '\u{08E2}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{110BD}'
            | '\u{110CD}'
            | '\u{13430}'..='\u{1343F}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0001}'
            | '\u{E0020}'..='\u{E007F}',
    )
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
