use super::rule::IgnoreRule;

/// Each case is `(rule, path, partial, minimatch's result)`, recorded from
/// minimatch with the options ignore-walk passes it.
const CASES: &[(&str, &str, bool, bool)] = &[
    ("*", "", false, false),
    ("*", "a/", false, true),
    ("lib/*", "lib/", false, false),
    ("a*", "A", false, true),
    ("lib/!(x)", "lib/", false, true),
    ("[a-c]", "B", false, true),
    ("a{b,c}d", "acd", false, true),
    ("a{b}d", "a{b}d", false, true),
    ("*.js", "x.JS", false, true),
    ("foo/", "foo", false, false),
    ("foo/", "foo", true, true),
    ("!(a)b", "cb", false, false),
    ("*(a|b)c", "ababc", false, true),
    ("+(a)", "", false, false),
    ("?(a)", "", false, true),
    (r"\*", "*", false, true),
    (r"\*", "x", false, false),
    ("[!a]", "b", false, true),
    ("[^a]", "a", false, false),
    ("[]a]", "]", false, true),
    ("[[:digit:]]x", "1x", false, true),
    ("a[", "a[", false, true),
    ("@(a", "@(a", false, true),
    ("l/**", "l/", false, true),
    ("l/?(a)", "l/", false, true),
    ("l/+(a)", "l/", false, false),
    ("l/**/*", "l/", false, false),
    ("l/*/", "l/", false, false),
    ("x/!(a)b", "x/ab", false, false),
    ("x/!(a)b", "x/cb", false, true),
    ("x/!(a)b", "x/b", false, true),
    ("x/!(a|ab)", "x/ab", false, false),
    ("x/a!(b)", "x/a", false, true),
    ("x/a!(b)c", "x/abc", false, false),
    ("x/!(*.js)", "x/.js", false, false),
    ("x/@(a|b)*", "x/bz", false, true),
    ("a/**", "a/.hidden/x", false, true),
    ("x/[A-C]", "x/b", false, true),
    ("x/É", "x/é", false, true),
    ("lib/**/!(*.js.map|.tsbuildinfo)", "lib/utils/foo.js", false, true),
    ("lib/**/!(*.js.map|.tsbuildinfo)", "lib/utils/foo.js.map", false, false),
    ("lib/**/!(*.js.map|.tsbuildinfo)", "lib/.tsbuildinfo", false, false),
];

#[test]
fn rules_match_like_minimatch() {
    for &(rule, path, partial, expected) in CASES {
        let parsed = IgnoreRule::parse(rule);
        assert_eq!(
            parsed.matches(path, partial),
            expected,
            "rule {rule:?} on {path:?} (partial: {partial})",
        );
    }
}

#[test]
fn leading_exclamation_marks_toggle_negation() {
    assert!(!IgnoreRule::parse("lib").negate);
    assert!(IgnoreRule::parse("!lib").negate);
    assert!(!IgnoreRule::parse("!!lib").negate);
    assert!(IgnoreRule::parse("!(lib)").negate);
}

#[test]
fn many_stars_do_not_backtrack_exponentially() {
    let rule = IgnoreRule::parse(&format!("{}b", "*a".repeat(30)));
    assert!(!rule.matches(&"a".repeat(200), false));
}

#[test]
fn repeated_brace_groups_expand_to_a_bounded_number_of_alternatives() {
    let rule = IgnoreRule::parse(&"{a,b}".repeat(40));
    assert!(rule.matches(&"a".repeat(40), false));
}

#[test]
fn ambiguous_extglob_repetition_is_bounded() {
    let rule = IgnoreRule::parse("+(a|aa)");
    assert!(!rule.matches(&format!("{}b", "a".repeat(200)), false));
}

#[test]
fn deeply_nested_extglobs_parse() {
    let rule = IgnoreRule::parse(&format!("{}a{}", "@(".repeat(100_000), ")".repeat(100_000)));
    assert!(!rule.matches("b", false));
}

#[test]
fn separated_globstars_are_bounded() {
    let rule = IgnoreRule::parse(&format!("{}z", "**/a/".repeat(20)));
    assert!(!rule.matches(&"a/".repeat(60), false));
}
