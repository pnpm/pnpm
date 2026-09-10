use crate::glob::DirGlob;

fn is_match(candidate: &str, pattern: &str) -> bool {
    DirGlob::new(pattern).is_match(candidate)
}

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
fn trailing_star_consumes_after_exact_prefix() {
    assert!(is_match("/a/foo", "/a/foo*"));
    assert!(is_match("/a/foobar", "/a/foo*"));
}

#[test]
fn multiple_stars_in_one_segment_backtrack() {
    assert!(is_match("/a/xaaaab", "/a/x*aa*aab"));
    assert!(is_match("/a/foo-bar-baz", "/a/foo-*-baz"));
    assert!(!is_match("/a/foo-bar", "/a/foo-*-baz"));
}

#[test]
fn question_mark_matches_one_character() {
    assert!(is_match("/packages/pkg-a", "/packages/pkg-?"));
    assert!(!is_match("/packages/pkg-ab", "/packages/pkg-?"));
    assert!(!is_match("/packages/pkg-", "/packages/pkg-?"));
}

#[test]
fn character_class_matches_one_character() {
    assert!(is_match("/packages/pkg-a", "/packages/pkg-[ab]"));
    assert!(is_match("/packages/pkg-b", "/packages/pkg-[ab]"));
    assert!(!is_match("/packages/pkg-c", "/packages/pkg-[ab]"));
}

#[test]
fn character_class_matches_a_range() {
    assert!(is_match("/packages/pkg-b", "/packages/pkg-[a-c]"));
    assert!(!is_match("/packages/pkg-d", "/packages/pkg-[a-c]"));
}

#[test]
fn dash_at_either_end_of_a_character_class_is_a_member() {
    assert!(is_match("/packages/pkg--", "/packages/pkg-[-a]"));
    assert!(is_match("/packages/pkg--", "/packages/pkg-[a-]"));
    assert!(!is_match("/packages/pkg-b", "/packages/pkg-[a-]"));
}

#[test]
fn caret_negates_a_character_class_but_exclamation_mark_does_not() {
    assert!(is_match("/packages/pkg-c", "/packages/pkg-[^ab]"));
    assert!(!is_match("/packages/pkg-a", "/packages/pkg-[^ab]"));
    assert!(is_match("/packages/pkg-a", "/packages/pkg-[!ab]"));
    assert!(is_match("/packages/pkg-!", "/packages/pkg-[!ab]"));
    assert!(!is_match("/packages/pkg-c", "/packages/pkg-[!ab]"));
}

#[test]
fn closing_bracket_first_is_a_member() {
    assert!(is_match("/packages/]", "/packages/[]a]"));
    assert!(is_match("/packages/a", "/packages/[]a]"));
    assert!(!is_match("/packages/b", "/packages/[]a]"));
}

#[test]
fn unterminated_bracket_is_a_literal() {
    assert!(is_match("/packages/[ab", "/packages/[ab"));
    assert!(!is_match("/packages/a", "/packages/[ab"));
}

#[test]
fn a_directory_named_like_a_pattern_matches_its_own_path() {
    assert!(is_match("/packages/pkg[1]", "/packages/pkg[1]"));
    assert!(is_match("/packages/pkg{1}", "/packages/pkg{1}"));
    assert!(is_match("/packages/pkg$a", "/packages/pkg$a"));
    assert!(is_match("/packages/pkg(1)", "/packages/pkg(1)"));
    assert!(!is_match("/packages/pkg1", "/packages/pkg$a"));
    assert!(!is_match("/packages/pkg+a", "/packages/pkg$a"));
}

#[test]
fn wildcards_do_not_match_a_leading_dot() {
    assert!(!is_match("/packages/.hidden", "/packages/*"));
    assert!(!is_match("/packages/.hidden", "/packages/?hidden"));
    assert!(!is_match("/packages/.hidden", "/packages/**"));
    assert!(!is_match("/packages/.hidden/nested", "/packages/**"));
    assert!(is_match("/packages/pkg-a", "/packages/*"));
}

#[test]
fn a_literal_dot_or_a_character_class_matches_a_hidden_segment() {
    assert!(is_match("/packages/.hidden", "/packages/.*"));
    assert!(is_match("/packages/.hidden", "/packages/[.]hidden"));
    assert!(is_match("/packages/.hidden", "/packages/[a-z.]hidden"));
}

#[test]
fn a_dot_inside_a_segment_is_an_ordinary_character() {
    assert!(is_match("/packages/a.c", "/packages/a?c"));
    assert!(is_match("/packages/a.c", "/packages/a*c"));
}

#[test]
fn backslash_separators_are_normalized_in_both_candidate_and_pattern() {
    assert!(is_match(r"C:\packages\project-0", "C:/packages/*"));
    assert!(is_match("C:/packages/project-0", r"C:\packages\*"));
    assert!(is_match(r"C:\packages\project-0\", r"C:\packages\*"));
}

#[test]
fn windows_drive_paths_support_micromatch_wildcards() {
    assert!(is_match(r"C:\packages\pkg-a", r"C:\packages\pkg-?"));
    assert!(is_match(r"C:\packages\pkg-b", r"C:\packages\pkg-[ab]"));
    assert!(!is_match(r"C:\packages\pkg-c", r"C:\packages\pkg-[ab]"));
    assert!(!is_match(r"D:\packages\pkg-a", r"C:\packages\pkg-?"));
}
