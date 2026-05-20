use pacquet_crypto_hash::shorten_virtual_store_name;

/// Turn a depPath into a filesystem-safe directory name. Mirrors pnpm's
/// [`depPathToFilename`](https://github.com/pnpm/pnpm/blob/097983fbca/deps/path/src/index.ts#L169-L180).
///
/// Pipeline:
///
/// 1. **Escape the scheme prefix.** A `file:` prefix has its `:`
///    rewritten to `+`. The unescape branch upstream calls
///    `depPathToFilenameUnescaped`.
/// 2. **Strip a leading `/`** for the relative-depPath shape (legacy
///    pre-v9 lockfiles), then re-join the `@version` half so the
///    resulting name still has the `name@version` shape — upstream's
///    `${first}@${rest}` rebuild is a no-op for already-flat depPaths
///    and pacquet's port matches it directly.
/// 3. **Replace path-unsafe characters** (`\\ / : * ? " < > | #`) with
///    `+`.
/// 4. **Flatten parens** — strip the trailing `)`, then rewrite `)(`,
///    `(`, and `)` to `_`. After this step a depPath like
///    `foo@1.0.0(bar@2.0.0)` becomes `foo@1.0.0_bar@2.0.0`.
/// 5. **Cap length / case** via
///    [`pacquet_crypto_hash::shorten_virtual_store_name`]. Same trailing
///    branch the flat-name call sites already consume — single source of
///    truth for the truncation arithmetic and `file+` carve-out.
pub fn dep_path_to_filename(dep_path: &str, max_length_without_hash: usize) -> String {
    let mut filename = dep_path_to_filename_unescaped(dep_path);
    filename = filename.replace(['\\', '/', ':', '*', '?', '"', '<', '>', '|', '#'], "+");
    if filename.contains('(') {
        if filename.ends_with(')') {
            filename.pop();
        }
        filename = filename.replace(")(", "_").replace(['(', ')'], "_");
    }
    shorten_virtual_store_name(filename, max_length_without_hash)
}

/// Pre-escape pass: rewrite `file:` to `file+`, strip a single leading
/// `/`, and re-join `@version`. Mirrors pnpm's private
/// [`depPathToFilenameUnescaped`](https://github.com/pnpm/pnpm/blob/097983fbca/deps/path/src/index.ts#L182-L192).
fn dep_path_to_filename_unescaped(dep_path: &str) -> String {
    if dep_path.starts_with("file:") {
        return dep_path.replacen(':', "+", 1);
    }
    let trimmed = dep_path.strip_prefix('/').unwrap_or(dep_path);
    // Find the `@` separator after position 1 (upstream's
    // `depPath.indexOf('@', 1)` — position 1 skips a scope marker
    // `@scope/...`).
    let after_first = &trimmed.as_bytes()[1..];
    let Some(rel) = after_first.iter().position(|&b| b == b'@') else {
        return trimmed.to_string();
    };
    let split = rel + 1;
    let (name, rest) = trimmed.split_at(split);
    // Upstream rebuilds as `${name}@${rest.slice(1)}` — i.e. it consumes
    // the `@` and re-emits one. The transformation is a no-op for any
    // input whose `name` slot does not already end in `@`; matches the
    // upstream form byte-for-byte.
    let rest = rest.strip_prefix('@').unwrap_or(rest);
    format!("{name}@{rest}")
}

#[cfg(test)]
mod tests {
    use super::dep_path_to_filename;

    #[test]
    fn plain_name_at_version_round_trips() {
        assert_eq!(dep_path_to_filename("foo@1.0.0", 120), "foo@1.0.0");
    }

    #[test]
    fn scoped_name_keeps_at_replaces_slash_with_plus() {
        assert_eq!(dep_path_to_filename("@scope/foo@1.0.0", 120), "@scope+foo@1.0.0");
    }

    #[test]
    fn peer_suffix_is_flattened_with_underscores() {
        assert_eq!(
            dep_path_to_filename("foo@1.0.0(bar@2.0.0)(baz@3.0.0)", 120),
            "foo@1.0.0_bar@2.0.0_baz@3.0.0",
        );
    }

    #[test]
    fn file_scheme_keeps_path_separators_via_plus_escape() {
        assert_eq!(dep_path_to_filename("file:packages/foo", 120), "file+packages+foo",);
    }

    #[test]
    fn exceeding_length_replaces_with_hash_suffix() {
        let very_long_input = format!("foo@1.0.0{}", "(bar@2.0.0)".repeat(40));
        let got = dep_path_to_filename(&very_long_input, 60);
        assert_eq!(got.len(), 60);
        assert!(got.contains('_'));
        // The hash suffix is `_` + 32 hex chars at the end.
        let hash_part = &got[got.len() - 32..];
        assert!(hash_part.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn uppercase_outside_file_scheme_forces_hash_suffix() {
        let got = dep_path_to_filename("FOO@1.0.0", 120);
        assert!(got.starts_with("FOO@1.0.0_"));
        assert_eq!(got.len(), "FOO@1.0.0_".len() + 32);
    }

    #[test]
    fn uppercase_in_file_scheme_is_preserved_untouched() {
        // `file+...` is excluded from the case-mismatch branch — the
        // filesystem casing of `file:` paths is part of the install
        // address, so hashing it would split the cache.
        assert_eq!(dep_path_to_filename("file:Pkg", 120), "file+Pkg");
    }
}
