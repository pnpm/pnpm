/// Index pair returned by [`index_of_dep_path_suffix`]. Both fields are
/// byte offsets into the original depPath, or `None` when the suffix is
/// absent. Mirrors pnpm's
/// [`indexOfDepPathSuffix`](https://github.com/pnpm/pnpm/blob/097983fbca/deps/path/src/index.ts#L9-L31)
/// `{ peersIndex, patchHashIndex }` return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DepPathSuffixIndex {
    pub peers_index: Option<usize>,
    pub patch_hash_index: Option<usize>,
}

/// Walk `dep_path` from right to left, balancing parentheses, and find
/// the boundary of the peer-suffix and (optional) `(patch_hash=…)`
/// segments. Mirrors pnpm's
/// [`indexOfDepPathSuffix`](https://github.com/pnpm/pnpm/blob/097983fbca/deps/path/src/index.ts#L9-L31).
///
/// Returns `(None, None)` when the path has no trailing `)` to anchor
/// the scan from — the depPath has neither a peer suffix nor a patch
/// hash, and the whole string is the `pkgIdWithPatchHash` (without a
/// patch hash, that's just the bare `name@version` id).
pub fn index_of_dep_path_suffix(dep_path: &str) -> DepPathSuffixIndex {
    let bytes = dep_path.as_bytes();
    let absent = DepPathSuffixIndex { peers_index: None, patch_hash_index: None };
    if !dep_path.ends_with(')') {
        return absent;
    }

    let mut open: i32 = 1;
    // Scan from second-to-last byte down to byte 0. Upstream's loop
    // starts at `length - 2` and stops at `>= 0`; we mirror it byte-for-
    // byte (depPath is ASCII outside of the package-name slot, and pnpm
    // doesn't permit non-ASCII there either).
    let mut cursor = bytes.len().checked_sub(2);
    while let Some(idx) = cursor {
        match bytes[idx] {
            b'(' => open -= 1,
            b')' => open += 1,
            _ if open == 0 => {
                // Position `idx + 1` is the start of a balanced
                // top-level segment. Upstream checks if that segment
                // starts with `(patch_hash=` — if so, the patch-hash
                // segment lives here and the peers segment (if any)
                // starts at the next `(` after `idx + 2`.
                let start = idx + 1;
                if dep_path[start..].starts_with("(patch_hash=") {
                    let peers_index = dep_path[start + 2..].find('(').map(|off| start + 2 + off);
                    return DepPathSuffixIndex { peers_index, patch_hash_index: Some(start) };
                }
                return DepPathSuffixIndex { peers_index: Some(start), patch_hash_index: None };
            }
            _ => {}
        }
        cursor = idx.checked_sub(1);
    }
    absent
}

/// Strip the peer-suffix and `(patch_hash=…)` segments from `dep_path`,
/// returning just the `pkgId` (no patch hash) prefix. Mirrors pnpm's
/// [`removeSuffix`](https://github.com/pnpm/pnpm/blob/097983fbca/deps/path/src/index.ts#L52-L61).
pub fn remove_suffix(dep_path: &str) -> &str {
    let DepPathSuffixIndex { peers_index, patch_hash_index } = index_of_dep_path_suffix(dep_path);
    if let Some(idx) = patch_hash_index {
        return &dep_path[..idx];
    }
    if let Some(idx) = peers_index {
        return &dep_path[..idx];
    }
    dep_path
}

/// Strip just the peer-suffix from `dep_path`, keeping the
/// `(patch_hash=…)` segment if present. Mirrors pnpm's
/// [`getPkgIdWithPatchHash`](https://github.com/pnpm/pnpm/blob/097983fbca/deps/path/src/index.ts#L63-L70).
pub fn get_pkg_id_with_patch_hash(dep_path: &str) -> &str {
    let DepPathSuffixIndex { peers_index, .. } = index_of_dep_path_suffix(dep_path);
    match peers_index {
        Some(idx) => &dep_path[..idx],
        None => dep_path,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DepPathSuffixIndex, get_pkg_id_with_patch_hash, index_of_dep_path_suffix, remove_suffix,
    };

    #[test]
    fn no_trailing_paren_means_no_suffix() {
        assert_eq!(
            index_of_dep_path_suffix("foo@1.0.0"),
            DepPathSuffixIndex { peers_index: None, patch_hash_index: None },
        );
    }

    #[test]
    fn locates_a_peer_suffix() {
        let dep_path = "foo@1.0.0(bar@2.0.0)";
        let got = index_of_dep_path_suffix(dep_path);
        assert_eq!(got.peers_index, Some("foo@1.0.0".len()));
        assert_eq!(got.patch_hash_index, None);
        assert_eq!(remove_suffix(dep_path), "foo@1.0.0");
        assert_eq!(get_pkg_id_with_patch_hash(dep_path), "foo@1.0.0");
    }

    #[test]
    fn locates_a_patch_hash_suffix_without_peers() {
        let dep_path = "foo@1.0.0(patch_hash=abc)";
        let got = index_of_dep_path_suffix(dep_path);
        assert_eq!(got.patch_hash_index, Some("foo@1.0.0".len()));
        assert_eq!(got.peers_index, None);
        assert_eq!(remove_suffix(dep_path), "foo@1.0.0");
        assert_eq!(get_pkg_id_with_patch_hash(dep_path), "foo@1.0.0(patch_hash=abc)");
    }

    #[test]
    fn locates_both_patch_hash_and_peers() {
        let dep_path = "foo@1.0.0(patch_hash=abc)(bar@2.0.0)";
        let got = index_of_dep_path_suffix(dep_path);
        assert_eq!(got.patch_hash_index, Some("foo@1.0.0".len()));
        assert_eq!(got.peers_index, Some("foo@1.0.0(patch_hash=abc)".len()));
        assert_eq!(remove_suffix(dep_path), "foo@1.0.0");
        assert_eq!(get_pkg_id_with_patch_hash(dep_path), "foo@1.0.0(patch_hash=abc)");
    }

    #[test]
    fn handles_nested_parens_in_peer_segment() {
        // A transitive peer can itself carry a peer suffix —
        // `(bar@2.0.0(baz@3.0.0))` is one balanced segment.
        let dep_path = "foo@1.0.0(bar@2.0.0(baz@3.0.0))";
        let got = index_of_dep_path_suffix(dep_path);
        assert_eq!(got.peers_index, Some("foo@1.0.0".len()));
    }

    /// Mirrors the `getPkgIdWithPatchHash('node@runtime:24.11.1')` leg of
    /// pnpm's [`deps/path/test/index.ts`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/path/test/index.ts#L119):
    /// a runtime depPath has no parenthesised suffix, so both helpers
    /// return the path verbatim.
    #[test]
    fn runtime_dep_path_has_no_suffix() {
        let dep_path = "node@runtime:24.11.1";
        let got = index_of_dep_path_suffix(dep_path);
        assert_eq!(got, DepPathSuffixIndex { peers_index: None, patch_hash_index: None });
        assert_eq!(remove_suffix(dep_path), "node@runtime:24.11.1");
        assert_eq!(get_pkg_id_with_patch_hash(dep_path), "node@runtime:24.11.1");
    }

    /// Mirrors the scoped-name leg of pnpm's `getPkgIdWithPatchHash`:
    /// `@foo/bar@1.0.0` round-trips verbatim through both helpers.
    /// See [`deps/path/test/index.ts:134`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/path/test/index.ts#L134).
    #[test]
    fn scoped_name_without_suffix_round_trips() {
        let dep_path = "@foo/bar@1.0.0";
        assert_eq!(remove_suffix(dep_path), "@foo/bar@1.0.0");
        assert_eq!(get_pkg_id_with_patch_hash(dep_path), "@foo/bar@1.0.0");
    }

    /// Mirrors `getPkgIdWithPatchHash('@foo/bar@1.0.0(patch_hash=yyyy)')`.
    /// See [`deps/path/test/index.ts:137`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/path/test/index.ts#L137).
    #[test]
    fn scoped_name_with_patch_hash_keeps_patch_hash() {
        let dep_path = "@foo/bar@1.0.0(patch_hash=yyyy)";
        assert_eq!(remove_suffix(dep_path), "@foo/bar@1.0.0");
        assert_eq!(get_pkg_id_with_patch_hash(dep_path), "@foo/bar@1.0.0(patch_hash=yyyy)");
    }

    /// Mirrors `getPkgIdWithPatchHash('@foo/bar@1.0.0(@types/node@18.0.0)')`.
    /// See [`deps/path/test/index.ts:140`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/path/test/index.ts#L140).
    #[test]
    fn scoped_name_with_peer_strips_to_bare() {
        let dep_path = "@foo/bar@1.0.0(@types/node@18.0.0)";
        assert_eq!(remove_suffix(dep_path), "@foo/bar@1.0.0");
        assert_eq!(get_pkg_id_with_patch_hash(dep_path), "@foo/bar@1.0.0");
    }

    /// Mirrors `getPkgIdWithPatchHash('@foo/bar@1.0.0(patch_hash=zzzz)(@types/node@18.0.0)')`.
    /// `remove_suffix` strips both, while `get_pkg_id_with_patch_hash`
    /// keeps the patch-hash segment.
    /// See [`deps/path/test/index.ts:143`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/path/test/index.ts#L143).
    #[test]
    fn scoped_name_with_patch_hash_and_peer_keeps_only_patch_hash() {
        let dep_path = "@foo/bar@1.0.0(patch_hash=zzzz)(@types/node@18.0.0)";
        assert_eq!(remove_suffix(dep_path), "@foo/bar@1.0.0");
        assert_eq!(get_pkg_id_with_patch_hash(dep_path), "@foo/bar@1.0.0(patch_hash=zzzz)");
    }

    /// Mirrors `tryGetPackageId('/foo@1.0.0(@types/babel__core@7.1.14(is-odd@1.0.0))')`.
    /// A nested peer-on-peer segment stays balanced; the outer `(`
    /// is the start of the (single) peer-graph segment.
    /// See [`deps/path/test/index.ts:112`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/path/test/index.ts#L112).
    #[test]
    fn leading_slash_legacy_with_nested_peer_strips_to_bare() {
        // Pacquet doesn't parse the leading-slash legacy shape via
        // `PkgNameVerPeer`, but the lower-level depPath helpers do
        // operate on the raw string — keep the contract pinned.
        let dep_path = "/foo@1.0.0(@types/babel__core@7.1.14(is-odd@1.0.0))";
        assert_eq!(remove_suffix(dep_path), "/foo@1.0.0");
        assert_eq!(get_pkg_id_with_patch_hash(dep_path), "/foo@1.0.0");
    }

    /// Mirrors `tryGetPackageId('/@(-.-)/foo@1.0.0(@types/babel__core@7.1.14)')`.
    /// The `(-.-)` parens belong to the scope name, not a peer suffix —
    /// the balanced-paren scan from the right correctly recognises the
    /// trailing `(@types/babel__core@7.1.14)` as the single peer
    /// segment and leaves the scope intact.
    /// See [`deps/path/test/index.ts:113`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/path/test/index.ts#L113).
    #[test]
    fn scope_with_parens_does_not_confuse_suffix_scan() {
        let dep_path = "/@(-.-)/foo@1.0.0(@types/babel__core@7.1.14)";
        assert_eq!(remove_suffix(dep_path), "/@(-.-)/foo@1.0.0");
        assert_eq!(get_pkg_id_with_patch_hash(dep_path), "/@(-.-)/foo@1.0.0");
    }
}
