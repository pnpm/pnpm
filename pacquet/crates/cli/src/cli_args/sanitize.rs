use std::borrow::Cow;

/// Strip control characters from store-derived text before it reaches the
/// terminal, keeping `\n` and `\t`. Prevents stored metadata from emitting
/// raw escape sequences to the user's terminal.
pub fn sanitize(text: &str) -> Cow<'_, str> {
    if text.bytes().any(|byte| byte < 0x20 && byte != b'\n' && byte != b'\t') {
        Cow::Owned(
            text.chars()
                .filter(|character| {
                    !character.is_control() || *character == '\n' || *character == '\t'
                })
                .collect(),
        )
    } else {
        Cow::Borrowed(text)
    }
}
