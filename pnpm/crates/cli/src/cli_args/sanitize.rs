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
