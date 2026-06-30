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

/// Mirrors pnpm's `getPkgIdWithPatchHash` test, runtime leg
/// ([`deps/path/test/index.ts`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/path/test/index.ts#L119)).
#[test]
fn runtime_dep_path_has_no_suffix() {
    let dep_path = "node@runtime:24.11.1";
    let got = index_of_dep_path_suffix(dep_path);
    assert_eq!(got, DepPathSuffixIndex { peers_index: None, patch_hash_index: None });
    assert_eq!(remove_suffix(dep_path), "node@runtime:24.11.1");
    assert_eq!(get_pkg_id_with_patch_hash(dep_path), "node@runtime:24.11.1");
}

/// Mirrors pnpm's `getPkgIdWithPatchHash` test, scoped-name leg
/// ([`deps/path/test/index.ts`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/path/test/index.ts#L134)).
#[test]
fn scoped_name_without_suffix_round_trips() {
    let dep_path = "@foo/bar@1.0.0";
    assert_eq!(remove_suffix(dep_path), "@foo/bar@1.0.0");
    assert_eq!(get_pkg_id_with_patch_hash(dep_path), "@foo/bar@1.0.0");
}

/// Mirrors pnpm's `getPkgIdWithPatchHash` test, scoped + patch-hash leg
/// ([`deps/path/test/index.ts`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/path/test/index.ts#L137)).
#[test]
fn scoped_name_with_patch_hash_keeps_patch_hash() {
    let dep_path = "@foo/bar@1.0.0(patch_hash=yyyy)";
    assert_eq!(remove_suffix(dep_path), "@foo/bar@1.0.0");
    assert_eq!(get_pkg_id_with_patch_hash(dep_path), "@foo/bar@1.0.0(patch_hash=yyyy)");
}

/// Mirrors pnpm's `getPkgIdWithPatchHash` test, scoped + peer leg
/// ([`deps/path/test/index.ts`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/path/test/index.ts#L140)).
#[test]
fn scoped_name_with_peer_strips_to_bare() {
    let dep_path = "@foo/bar@1.0.0(@types/node@18.0.0)";
    assert_eq!(remove_suffix(dep_path), "@foo/bar@1.0.0");
    assert_eq!(get_pkg_id_with_patch_hash(dep_path), "@foo/bar@1.0.0");
}

/// Mirrors pnpm's `getPkgIdWithPatchHash` test, scoped + patch-hash + peer leg
/// ([`deps/path/test/index.ts`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/path/test/index.ts#L143)).
#[test]
fn scoped_name_with_patch_hash_and_peer_keeps_only_patch_hash() {
    let dep_path = "@foo/bar@1.0.0(patch_hash=zzzz)(@types/node@18.0.0)";
    assert_eq!(remove_suffix(dep_path), "@foo/bar@1.0.0");
    assert_eq!(get_pkg_id_with_patch_hash(dep_path), "@foo/bar@1.0.0(patch_hash=zzzz)");
}

/// Mirrors pnpm's `tryGetPackageId` test, leading-slash legacy + nested-peer leg
/// ([`deps/path/test/index.ts`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/path/test/index.ts#L112)).
/// `PkgNameVerPeer` rejects the leading-slash shape, but the
/// string-level helpers still need to handle it for older lockfile
/// readers.
#[test]
fn leading_slash_legacy_with_nested_peer_strips_to_bare() {
    let dep_path = "/foo@1.0.0(@types/babel__core@7.1.14(is-odd@1.0.0))";
    assert_eq!(remove_suffix(dep_path), "/foo@1.0.0");
    assert_eq!(get_pkg_id_with_patch_hash(dep_path), "/foo@1.0.0");
}

/// Mirrors pnpm's `tryGetPackageId` test, scope-with-parens leg
/// ([`deps/path/test/index.ts`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/path/test/index.ts#L113)).
/// The right-to-left balanced scan is what makes this work — a
/// left-to-right `find('(')` would split inside `(-.-)`.
#[test]
fn scope_with_parens_does_not_confuse_suffix_scan() {
    let dep_path = "/@(-.-)/foo@1.0.0(@types/babel__core@7.1.14)";
    assert_eq!(remove_suffix(dep_path), "/@(-.-)/foo@1.0.0");
    assert_eq!(get_pkg_id_with_patch_hash(dep_path), "/@(-.-)/foo@1.0.0");
}
