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
fn brace_alternatives_select_either_branch() {
    assert!(is_match("/packages/pkg-a", "/packages/pkg-{a,b}"));
    assert!(is_match("/packages/pkg-b", "/packages/pkg-{a,b}"));
    assert!(!is_match("/packages/pkg-c", "/packages/pkg-{a,b}"));
}

#[test]
fn brace_alternatives_nest_and_combine() {
    assert!(is_match("/packages/c", "/packages/{a,{b,c}}"));
    assert!(is_match("/packages/ad", "/packages/{a,b}{c,d}"));
    assert!(!is_match("/packages/cd", "/packages/{a,b}{c,d}"));
    assert!(is_match("/packages/b-1", "/packages/{a,b}-?"));
    assert!(is_match("/packages/bx", "/packages/{a,b}*"));
}

#[test]
fn a_group_inside_literal_braces_still_expands() {
    // picomatch reads `{{a,b}}` as a literal `{`, the group, a literal `}`.
    assert!(is_match("/packages/{a}", "/packages/{{a,b}}"));
    assert!(is_match("/packages/{b}", "/packages/{{a,b}}"));
    assert!(!is_match("/packages/a", "/packages/{{a,b}}"));
    assert!(is_match("/packages/{xa}", "/packages/{x{a,b}}"));
    assert!(!is_match("/packages/{xc}", "/packages/{x{a,b}}"));
}

#[test]
fn a_range_inside_literal_braces_is_the_nested_group() {
    // The `..` belongs to the inner group, so the outer braces are not a
    // range from `{a` to `c}`.
    assert!(is_match("/packages/{b}", "/packages/{{a..c}}"));
    assert!(!is_match("/packages/b", "/packages/{{a..c}}"));
}

#[test]
fn a_brace_alternative_may_span_a_separator() {
    assert!(is_match("/packages/b/c", "/packages/{a,b/c}"));
    assert!(is_match("/packages/a", "/packages/{a,b/c}"));
    assert!(is_match("/packages/a/x", "/packages/{a,b}/x"));
}

#[test]
fn a_comma_inside_a_character_class_does_not_split_alternatives() {
    assert!(is_match("/packages/,", "/packages/{[a,b],c}"));
    assert!(is_match("/packages/a", "/packages/{[a,b],c}"));
    assert!(is_match("/packages/c", "/packages/{[a,b],c}"));
    assert!(!is_match("/packages/d", "/packages/{[a,b],c}"));
}

#[test]
fn an_empty_alternative_still_requires_its_separator() {
    // picomatch compiles `{,pkg}` to `(|pkg)` after the `/`, so the empty
    // branch matches an empty segment rather than dropping the separator.
    assert!(!is_match("/packages", "/packages/{,pkg}"));
    assert!(!is_match("/packages", "/packages/{pkg,}"));
    assert!(is_match("/packages/pkg", "/packages/{,pkg}"));
    assert!(is_match("/packages/pkg", "/packages/{pkg,}"));
}

#[test]
fn braces_without_a_top_level_comma_are_literal() {
    assert!(is_match("/packages/{a}", "/packages/{a}"));
    assert!(!is_match("/packages/a", "/packages/{a}"));
}

#[test]
fn a_brace_left_open_matches_only_its_own_text() {
    // picomatch compiles a pattern holding an unmatched `{` to a regex that
    // matches nothing, so a group nested inside one must not expand either.
    assert!(is_match("/packages/{a,b", "/packages/{a,b"));
    assert!(!is_match("/packages/a", "/packages/{a,b"));
    assert!(is_match("/packages/{a,{b,c}", "/packages/{a,{b,c}"));
    assert!(!is_match("/packages/a", "/packages/{a,{b,c}"));
    assert!(!is_match("/packages/b", "/packages/{a,{b,c}"));
    assert!(!is_match("/packages/{a,b}", "/packages/{a,{b,c}"));
    // Expanding the inner group would leave the open `{` in an earlier
    // segment, where it would match a directory literally named `{x`.
    assert!(!is_match("/packages/{x/a", "/packages/{x/{a,b}"));
    assert!(!is_match("/packages/{x/b", "/packages/{x/{a,b}"));
    assert!(is_match("/packages/{x/{a,b}", "/packages/{x/{a,b}"));
    // A `}` with no `{` is ordinary text, and leaves other groups expanding.
    assert!(is_match("/packages/a}c", "/packages/a}{b,c}"));
}

#[test]
fn many_range_groups_stay_linear() {
    // Each `{a..c}` yields a single branch, so the alternative cap never
    // trips: the pattern must be scanned once rather than once per group.
    let pattern = format!("/packages/{}", "{a..c}".repeat(3_000));
    assert!(!is_match("/packages/x", &pattern));
}

#[test]
fn nesting_past_the_depth_cap_keeps_its_braces_literal() {
    let deep = format!("/packages/{}a{}", "{x,".repeat(64), "}".repeat(64));
    assert!(!is_match("/packages/a", &deep));
    assert!(is_match(&deep, &deep));
}

#[test]
fn many_unterminated_brackets_stay_linear() {
    let pattern = format!("/packages/{{a,b}}{}", "[".repeat(200_000));
    assert!(!is_match("/packages/a", &pattern));
    assert!(is_match(&pattern, &pattern));
}

#[test]
fn a_two_sided_brace_range_is_a_character_class() {
    // picomatch compiles `{x..y}` to `[x-y]` rather than expanding a range.
    assert!(is_match("/packages/b", "/packages/{a..c}"));
    assert!(!is_match("/packages/d", "/packages/{a..c}"));
    assert!(is_match("/packages/2", "/packages/{1..3}"));
    assert!(!is_match("/packages/10", "/packages/{1..3}"));
}

#[test]
fn range_endpoints_are_ordered() {
    // picomatch orders them, so `{x..c}` is the class `[c-x]`.
    assert!(is_match("/packages/m", "/packages/{x..c}"));
    assert!(!is_match("/packages/z", "/packages/{x..c}"));
    assert!(is_match("/packages/2", "/packages/{3..1}"));
    // Ordering is by character, so a letter and a digit span everything between.
    assert!(is_match("/packages/a", "/packages/{a..3}"));
    assert!(is_match("/packages/3", "/packages/{a..3}"));
}

#[test]
fn a_one_sided_range_is_the_endpoint_it_has() {
    assert!(is_match("/packages/a", "/packages/{a..}"));
    assert!(is_match("/packages/c", "/packages/{..c}"));
    assert!(!is_match("/packages/b", "/packages/{a..}"));
}

#[test]
fn a_group_that_is_not_a_range_stays_literal() {
    // Neither endpoint, and a second `..`, leave the braces as text.
    assert!(is_match("/packages/{..}", "/packages/{..}"));
    assert!(is_match("/packages/{1..9..2}", "/packages/{1..9..2}"));
    assert!(!is_match("/packages/1", "/packages/{1..9..2}"));
}

#[test]
fn a_bracket_expression_hides_the_dots_inside_it() {
    // The `..` sits in a class, so the group is not a range.
    assert!(is_match("/packages/{[a..b]}", "/packages/{[a..b]}"));
    assert!(!is_match("/packages/a", "/packages/{[a..b]}"));
}

#[test]
fn an_alternative_too_wide_to_expand_takes_the_whole_pattern_with_it() {
    // The narrow branch must not stay selectable once the wide one is
    // refused, or `{safe,<1025 branches>}` would still select `safe`.
    let wide = "{a,b}".repeat(11);
    let pattern = format!("/packages/{{safe,{wide}}}");
    assert!(!is_match("/packages/safe", &pattern));
    assert!(is_match(&pattern, &pattern));
}

#[test]
fn a_pathological_brace_pattern_keeps_its_braces_literal() {
    let product = format!("/packages/{}", "{a,b}".repeat(20));
    assert!(!is_match("/packages/aaaaaaaaaaaaaaaaaaaa", &product));
    assert!(is_match(&product, &product));

    let branches = (0..2000).map(|branch| branch.to_string()).collect::<Vec<_>>().join(",");
    let wide = format!("/packages/{{{branches}}}");
    assert!(!is_match("/packages/5", &wide));
    assert!(is_match(&wide, &wide));
}

#[test]
fn deeply_nested_braces_do_not_exhaust_the_stack() {
    let unterminated = format!("/packages/{}", "{".repeat(100_000));
    assert!(!is_match("/packages/a", &unterminated));

    let balanced = format!("/packages/{}{}", "{".repeat(100_000), "}".repeat(100_000));
    assert!(!is_match("/packages/a", &balanced));
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
