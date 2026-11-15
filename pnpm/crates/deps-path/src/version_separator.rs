/// Byte index of the `@` that separates `<name>` from `<version>` in a
/// package identifier or dependency path.
///
/// A leading `@` starts a scope (`@scope/pkg@1.0.0`), so the separator is
/// the first `@` after it; for unscoped inputs it is the first `@`.
#[must_use]
pub fn version_separator_index(value: &str) -> Option<usize> {
    match value.strip_prefix('@') {
        Some(rest) => rest.find('@').map(|offset| offset + 1),
        None => value.find('@'),
    }
}

#[cfg(test)]
mod tests;
