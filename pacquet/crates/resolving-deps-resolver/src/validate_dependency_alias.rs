//! Reject dependency aliases that, joined with a `node_modules` path,
//! would escape the intended directory. Mirrors pnpm's
//! [`isValidDependencyAlias`](https://github.com/pnpm/pnpm/blob/main/installing/deps-resolver/src/validateDependencyAlias.ts):
//! only `name` and `@scope/name` shapes are accepted, and any
//! `..`-segment, embedded backslash, or null byte is rejected.

/// `true` when `alias` is safe to join onto a `node_modules` path.
pub fn is_valid_dependency_alias(alias: &str) -> bool {
    if alias.is_empty() {
        return false;
    }
    if alias.contains('\0') || alias.contains('\\') {
        return false;
    }
    let segments: Vec<&str> = alias.split('/').collect();
    if segments.len() > 2 {
        return false;
    }
    for segment in &segments {
        if segment.is_empty() || *segment == "." || *segment == ".." {
            return false;
        }
    }
    if segments.len() == 2 && !segments[0].starts_with('@') {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::is_valid_dependency_alias;

    #[test]
    fn accepts_valid_aliases() {
        for alias in ["foo", "Foo", "@scope/name", "@s/x", "lodash.merge", "a_b", "a-b"] {
            assert!(is_valid_dependency_alias(alias), "expected valid: {alias:?}");
        }
    }

    #[test]
    fn rejects_invalid_aliases() {
        for alias in [
            "",
            "..",
            ".",
            "/foo",
            "foo/bar",
            "@scope/name/extra",
            "@scope/../etc",
            "@x/../../../../../.git/hooks",
            r"foo\bar",
            r"C:\Windows\System32",
            "foo\0bar",
            "scope/name",
            "./foo",
        ] {
            assert!(!is_valid_dependency_alias(alias), "expected invalid: {alias:?}");
        }
    }
}
