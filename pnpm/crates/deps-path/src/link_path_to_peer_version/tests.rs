use super::link_path_to_peer_version;

#[test]
fn replaces_path_separators_with_plus() {
    assert_eq!(link_path_to_peer_version("packages/b"), "packages+b");
}

#[test]
fn collapses_leading_dot_segments() {
    assert_eq!(link_path_to_peer_version("./packages/b"), "packages+b");
    assert_eq!(link_path_to_peer_version("../packages/b"), "packages+b");
}

#[test]
fn leading_dot_in_filename_is_dropped() {
    assert_eq!(link_path_to_peer_version(".hidden/pkg"), "hidden+pkg");
}

#[test]
fn collapses_runs_of_reserved_chars_into_one_plus() {
    assert_eq!(link_path_to_peer_version("a///b"), "a+b");
}

#[test]
fn windows_separators_collapse() {
    assert_eq!(link_path_to_peer_version(r"a\b\c"), "a+b+c");
}

#[test]
fn external_link_target_under_node_modules_matches_upstream() {
    assert_eq!(
        link_path_to_peer_version("node_modules/@pnpm.e2e/peer-a"),
        "node_modules+@pnpm.e2e+peer-a",
    );
}

#[test]
fn empty_input_returns_empty() {
    assert_eq!(link_path_to_peer_version(""), "");
}

#[test]
fn dot_only_collapses_to_single_plus() {
    assert_eq!(link_path_to_peer_version("."), "+");
    assert_eq!(link_path_to_peer_version(".."), "+");
}

#[test]
fn trailing_dots_and_plusses_are_trimmed() {
    assert_eq!(link_path_to_peer_version("a/b."), "a+b");
    assert_eq!(link_path_to_peer_version("a/b/"), "a+b");
}

/// Unicode path segments (multi-byte UTF-8) survive intact. A
/// byte-wise loop would corrupt them.
#[test]
fn non_ascii_path_segments_round_trip() {
    assert_eq!(link_path_to_peer_version("packages/café"), "packages+café");
    assert_eq!(link_path_to_peer_version("パッケージ/foo"), "パッケージ+foo");
    assert_eq!(link_path_to_peer_version("📦/pkg"), "📦+pkg");
}
