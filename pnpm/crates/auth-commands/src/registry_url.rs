//! Registry URL normalization shared by the `login` and `logout` commands.

use std::borrow::Cow;

/// Append a trailing slash if the registry URL lacks one, borrowing the input
/// unchanged when it already ends in one. Mirrors npm's `normalize-registry-url`.
pub(crate) fn normalize_registry_url(registry: &str) -> Cow<'_, str> {
    if registry.ends_with('/') {
        Cow::Borrowed(registry)
    } else {
        Cow::Owned(format!("{registry}/"))
    }
}
