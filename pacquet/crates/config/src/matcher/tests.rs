use super::{create_matcher, create_matcher_with_index};

fn pats<const LEN: usize>(patterns: [&str; LEN]) -> Vec<String> {
    patterns.iter().map(std::string::ToString::to_string).collect()
}

/// Direct port of upstream's `matcher()` test at
/// [`config/matcher/test/index.ts`](https://github.com/pnpm/pnpm/blob/94240bc046/config/matcher/test/index.ts#L4-L48).
#[test]
fn matcher_boolean_semantics() {
    let matcher = create_matcher(&pats(["*"]));
    assert!(matcher.matches("@eslint/plugin-foo"));
    assert!(matcher.matches("express"));

    let matcher = create_matcher(&pats(["eslint-*"]));
    assert!(matcher.matches("eslint-plugin-foo"));
    assert!(!matcher.matches("express"));

    let matcher = create_matcher(&pats(["*plugin*"]));
    assert!(matcher.matches("@eslint/plugin-foo"));
    assert!(!matcher.matches("express"));

    let matcher = create_matcher(&pats(["a*c"]));
    assert!(matcher.matches("abc"));

    let matcher = create_matcher(&pats(["*-positive"]));
    assert!(matcher.matches("is-positive"));

    let matcher = create_matcher(&pats(["foo", "bar"]));
    assert!(matcher.matches("foo"));
    assert!(matcher.matches("bar"));
    assert!(!matcher.matches("express"));

    let matcher = create_matcher(&pats(["eslint-*", "!eslint-plugin-bar"]));
    assert!(matcher.matches("eslint-plugin-foo"));
    assert!(!matcher.matches("eslint-plugin-bar"));

    let matcher = create_matcher(&pats(["!eslint-plugin-bar", "eslint-*"]));
    assert!(matcher.matches("eslint-plugin-foo"));
    // Upstream returns `1` (the include matched, after the ignore
    // missed) — boolean-side that's "matched".
    assert!(matcher.matches("eslint-plugin-bar"));

    let matcher = create_matcher(&pats(["eslint-*", "!eslint-plugin-*", "eslint-plugin-bar"]));
    assert!(matcher.matches("eslint-config-foo"));
    assert!(!matcher.matches("eslint-plugin-foo"));
    assert!(matcher.matches("eslint-plugin-bar"));
}

/// Direct port of upstream's `createMatcherWithIndex()` test at
/// [`config/matcher/test/index.ts`](https://github.com/pnpm/pnpm/blob/94240bc046/config/matcher/test/index.ts#L50-L107).
#[test]
fn matcher_with_index_semantics() {
    let matcher = create_matcher_with_index(&pats(["*"]));
    assert_eq!(matcher.matches("@eslint/plugin-foo"), Some(0));
    assert_eq!(matcher.matches("express"), Some(0));

    let matcher = create_matcher_with_index(&pats(["eslint-*"]));
    assert_eq!(matcher.matches("eslint-plugin-foo"), Some(0));
    assert_eq!(matcher.matches("express"), None);

    let matcher = create_matcher_with_index(&pats(["*plugin*"]));
    assert_eq!(matcher.matches("@eslint/plugin-foo"), Some(0));
    assert_eq!(matcher.matches("express"), None);

    let matcher = create_matcher_with_index(&pats(["a*c"]));
    assert_eq!(matcher.matches("abc"), Some(0));

    let matcher = create_matcher_with_index(&pats(["*-positive"]));
    assert_eq!(matcher.matches("is-positive"), Some(0));

    let matcher = create_matcher_with_index(&pats(["foo", "bar"]));
    assert_eq!(matcher.matches("foo"), Some(0));
    assert_eq!(matcher.matches("bar"), Some(1));
    assert_eq!(matcher.matches("express"), None);

    let matcher = create_matcher_with_index(&pats(["eslint-*", "!eslint-plugin-bar"]));
    assert_eq!(matcher.matches("eslint-plugin-foo"), Some(0));
    assert_eq!(matcher.matches("eslint-plugin-bar"), None);

    let matcher = create_matcher_with_index(&pats(["!eslint-plugin-bar", "eslint-*"]));
    assert_eq!(matcher.matches("eslint-plugin-foo"), Some(1));
    assert_eq!(matcher.matches("eslint-plugin-bar"), Some(1));

    let matcher =
        create_matcher_with_index(&pats(["eslint-*", "!eslint-plugin-*", "eslint-plugin-bar"]));
    assert_eq!(matcher.matches("eslint-config-foo"), Some(0));
    assert_eq!(matcher.matches("eslint-plugin-foo"), None);
    assert_eq!(matcher.matches("eslint-plugin-bar"), Some(2));

    let matcher = create_matcher_with_index(&pats(["!@pnpm.e2e/peer-*"]));
    assert_eq!(matcher.matches("@pnpm.e2e/foo"), Some(0));

    let matcher = create_matcher_with_index(&pats(["!foo", "!bar"]));
    assert_eq!(matcher.matches("foo"), None);
    assert_eq!(matcher.matches("bar"), None);
    assert_eq!(matcher.matches("baz"), Some(0));

    let matcher = create_matcher_with_index(&pats(["!foo", "!bar", "qar"]));
    assert_eq!(matcher.matches("foo"), None);
    assert_eq!(matcher.matches("bar"), None);
    assert_eq!(matcher.matches("baz"), None);
}

/// Empty list never matches — upstream's `case 0` of
/// `createMatcherWithIndex`.
#[test]
fn empty_pattern_list_never_matches() {
    let matcher = create_matcher(&[]);
    assert!(!matcher.matches("anything"));
    let matcher = create_matcher_with_index(&[]);
    assert_eq!(matcher.matches("anything"), None);
}

/// Pattern characters that are regex-special in upstream get
/// escaped before compilation, so they match literally here too.
#[test]
fn regex_special_chars_are_literal() {
    let matcher = create_matcher(&pats(["a.b"]));
    assert!(matcher.matches("a.b"));
    assert!(!matcher.matches("axb"));

    let matcher = create_matcher(&pats(["a?b"]));
    assert!(matcher.matches("a?b"));
    assert!(!matcher.matches("axb"));

    let matcher = create_matcher(&pats(["(foo)"]));
    assert!(matcher.matches("(foo)"));
    assert!(!matcher.matches("foo"));
}

/// `*` matches the empty string — pattern `a*b` accepts `ab`.
#[test]
fn star_matches_empty() {
    let matcher = create_matcher(&pats(["a*b"]));
    assert!(matcher.matches("ab"));
    assert!(matcher.matches("axb"));
    assert!(matcher.matches("axxxb"));
}

/// Triple-segment patterns find segments greedily but in order.
#[test]
fn multi_segment_glob_in_order() {
    let matcher = create_matcher(&pats(["a*b*c"]));
    assert!(matcher.matches("abc"));
    assert!(matcher.matches("axxbxxc"));
    assert!(!matcher.matches("acb"));
    // Last segment must be the suffix — `c` not at end fails.
    assert!(!matcher.matches("axbcx"));
}

/// `is_empty` is the static fast-path check callers use to skip
/// graph walks. Only `MatcherImpl::Never` (built from an empty
/// pattern list) reports `true`; non-empty pattern lists report
/// `false` even if no realistic input would match — the check
/// is on the pattern list, not on regex shape.
#[test]
fn is_empty_only_for_empty_pattern_list() {
    assert!(create_matcher(&[]).is_empty());
    assert!(!create_matcher(&pats(["*"])).is_empty());
    assert!(!create_matcher(&pats(["foo"])).is_empty());
    assert!(!create_matcher(&pats(["!nothing"])).is_empty());
    assert!(!create_matcher(&pats(["foo", "bar"])).is_empty());
}
