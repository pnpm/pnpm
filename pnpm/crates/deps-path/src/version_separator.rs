use pnpm_pkg_name_split::split_name_suffix;

/// Byte index of the `@` that separates `<name>` from `<version>` in a
/// package identifier or dependency path.
///
/// A leading `@` starts a scope (`@scope/pkg@1.0.0`), so the separator is
/// the first `@` after it; for unscoped inputs it is the first `@`.
#[must_use]
pub fn version_separator_index(value: &str) -> Option<usize> {
    split_name_suffix(value)
        .ok()
        .map(|(name, _)| name.as_str().len())
}

#[cfg(test)]
mod tests;
