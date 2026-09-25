use super::matches_params;
use pnpm_matcher::WildcardMatcher;

#[test]
fn params_match_exact_names_and_wildcards() {
    for (pattern, input, expected) in [
        ("foo", "foo", true),
        ("foo", "bar", false),
        ("*", "anything", true),
        ("@scope/*", "@scope/pkg", true),
        ("@scope/*", "@other/pkg", false),
        ("foo*bar", "fooXYZbar", true),
    ] {
        assert_eq!(matches_params(&[WildcardMatcher::new(pattern)], input), expected);
    }
}

#[test]
fn empty_params_match_everything_and_negations_are_literal() {
    assert!(matches_params(&[], "foo"));
    assert!(!matches_params(&[WildcardMatcher::new("!foo")], "bar"));
    assert!(matches_params(&[WildcardMatcher::new("!foo")], "!foo"));
    assert!(matches_params(&[WildcardMatcher::new("foo"), WildcardMatcher::new("!foo")], "foo",));
}
