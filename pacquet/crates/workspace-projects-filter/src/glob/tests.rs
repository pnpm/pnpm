use crate::glob::is_match;

#[test]
fn single_star_matches_one_segment() {
    assert!(is_match("/packages/project-0", "/packages/*"));
    assert!(is_match("/packages/project-1", "/packages/*"));
    assert!(!is_match("/packages", "/packages/*"));
    assert!(!is_match("/project-5/packages/project-6", "/packages/*"));
}

#[test]
fn globstar_matches_zero_or_more_segments() {
    assert!(is_match("/project-5", "/project-5/**"));
    assert!(is_match("/project-5/packages/project-6", "/project-5/**"));
    assert!(!is_match("/packages/project-0", "/project-5/**"));
}

#[test]
fn no_wildcard_matches_exact_path_only() {
    assert!(is_match("/project-5", "/project-5"));
    assert!(!is_match("/project-5/packages/project-6", "/project-5"));
}

#[test]
fn partial_segment_wildcard() {
    assert!(is_match("/packages/project-0", "/packages/proj*"));
    assert!(!is_match("/packages/lib-0", "/packages/proj*"));
}

#[test]
fn trailing_slash_is_ignored() {
    assert!(is_match("/packages/project-0/", "/packages/*/"));
}

#[test]
fn multiple_stars_in_one_segment_backtrack() {
    assert!(is_match("/a/xaaaab", "/a/x*aa*aab"));
    assert!(is_match("/a/foo-bar-baz", "/a/foo-*-baz"));
    assert!(!is_match("/a/foo-bar", "/a/foo-*-baz"));
}
