use super::{create_matcher, create_matcher_with_index};

#[cfg_attr(
    dylint_lib = "perfectionist",
    allow(
        perfectionist::single_letter_const_generic,
        reason = "`N` is the idiomatic const-generic array-length name, matching `[T; N]`"
    )
)]
fn pats<const N: usize>(patterns: [&str; N]) -> Vec<String> {
    patterns.iter().map(|pattern| pattern.to_string()).collect()
}

/// Direct port of upstream's `matcher()` test at
/// [`config/matcher/test/index.ts`](https://github.com/pnpm/pnpm/blob/94240bc046/config/matcher/test/index.ts#L4-L48).
#[test]
fn matcher_boolean_semantics() {
    let m = create_matcher(&pats(["*"]));
    assert!(m.matches("@eslint/plugin-foo"));
    assert!(m.matches("express"));

    let m = create_matcher(&pats(["eslint-*"]));
    assert!(m.matches("eslint-plugin-foo"));
    assert!(!m.matches("express"));

    let m = create_matcher(&pats(["*plugin*"]));
    assert!(m.matches("@eslint/plugin-foo"));
    assert!(!m.matches("express"));

    let m = create_matcher(&pats(["a*c"]));
    assert!(m.matches("abc"));

    let m = create_matcher(&pats(["*-positive"]));
    assert!(m.matches("is-positive"));

    let m = create_matcher(&pats(["foo", "bar"]));
    assert!(m.matches("foo"));
    assert!(m.matches("bar"));
    assert!(!m.matches("express"));

    let m = create_matcher(&pats(["eslint-*", "!eslint-plugin-bar"]));
    assert!(m.matches("eslint-plugin-foo"));
    assert!(!m.matches("eslint-plugin-bar"));

    let m = create_matcher(&pats(["!eslint-plugin-bar", "eslint-*"]));
    assert!(m.matches("eslint-plugin-foo"));
    // Upstream returns `1` (the include matched, after the ignore
    // missed) — boolean-side that's "matched".
    assert!(m.matches("eslint-plugin-bar"));

    let m = create_matcher(&pats(["eslint-*", "!eslint-plugin-*", "eslint-plugin-bar"]));
    assert!(m.matches("eslint-config-foo"));
    assert!(!m.matches("eslint-plugin-foo"));
    assert!(m.matches("eslint-plugin-bar"));
}

/// Direct port of upstream's `createMatcherWithIndex()` test at
/// [`config/matcher/test/index.ts`](https://github.com/pnpm/pnpm/blob/94240bc046/config/matcher/test/index.ts#L50-L107).
#[test]
fn matcher_with_index_semantics() {
    let m = create_matcher_with_index(&pats(["*"]));
    assert_eq!(m.matches("@eslint/plugin-foo"), Some(0));
    assert_eq!(m.matches("express"), Some(0));

    let m = create_matcher_with_index(&pats(["eslint-*"]));
    assert_eq!(m.matches("eslint-plugin-foo"), Some(0));
    assert_eq!(m.matches("express"), None);

    let m = create_matcher_with_index(&pats(["*plugin*"]));
    assert_eq!(m.matches("@eslint/plugin-foo"), Some(0));
    assert_eq!(m.matches("express"), None);

    let m = create_matcher_with_index(&pats(["a*c"]));
    assert_eq!(m.matches("abc"), Some(0));

    let m = create_matcher_with_index(&pats(["*-positive"]));
    assert_eq!(m.matches("is-positive"), Some(0));

    let m = create_matcher_with_index(&pats(["foo", "bar"]));
    assert_eq!(m.matches("foo"), Some(0));
    assert_eq!(m.matches("bar"), Some(1));
    assert_eq!(m.matches("express"), None);

    let m = create_matcher_with_index(&pats(["eslint-*", "!eslint-plugin-bar"]));
    assert_eq!(m.matches("eslint-plugin-foo"), Some(0));
    assert_eq!(m.matches("eslint-plugin-bar"), None);

    let m = create_matcher_with_index(&pats(["!eslint-plugin-bar", "eslint-*"]));
    assert_eq!(m.matches("eslint-plugin-foo"), Some(1));
    assert_eq!(m.matches("eslint-plugin-bar"), Some(1));

    let m = create_matcher_with_index(&pats(["eslint-*", "!eslint-plugin-*", "eslint-plugin-bar"]));
    assert_eq!(m.matches("eslint-config-foo"), Some(0));
    assert_eq!(m.matches("eslint-plugin-foo"), None);
    assert_eq!(m.matches("eslint-plugin-bar"), Some(2));

    let m = create_matcher_with_index(&pats(["!@pnpm.e2e/peer-*"]));
    assert_eq!(m.matches("@pnpm.e2e/foo"), Some(0));

    let m = create_matcher_with_index(&pats(["!foo", "!bar"]));
    assert_eq!(m.matches("foo"), None);
    assert_eq!(m.matches("bar"), None);
    assert_eq!(m.matches("baz"), Some(0));

    let m = create_matcher_with_index(&pats(["!foo", "!bar", "qar"]));
    assert_eq!(m.matches("foo"), None);
    assert_eq!(m.matches("bar"), None);
    assert_eq!(m.matches("baz"), None);
}

/// Empty list never matches — upstream's `case 0` of
/// `createMatcherWithIndex`.
#[test]
fn empty_pattern_list_never_matches() {
    let m = create_matcher(&[]);
    assert!(!m.matches("anything"));
    let m = create_matcher_with_index(&[]);
    assert_eq!(m.matches("anything"), None);
}

/// Pattern characters that are regex-special in upstream get
/// escaped before compilation, so they match literally here too.
#[test]
fn regex_special_chars_are_literal() {
    let m = create_matcher(&pats(["a.b"]));
    assert!(m.matches("a.b"));
    assert!(!m.matches("axb"));

    let m = create_matcher(&pats(["a?b"]));
    assert!(m.matches("a?b"));
    assert!(!m.matches("axb"));

    let m = create_matcher(&pats(["(foo)"]));
    assert!(m.matches("(foo)"));
    assert!(!m.matches("foo"));
}

/// `*` matches the empty string — pattern `a*b` accepts `ab`.
#[test]
fn star_matches_empty() {
    let m = create_matcher(&pats(["a*b"]));
    assert!(m.matches("ab"));
    assert!(m.matches("axb"));
    assert!(m.matches("axxxb"));
}

/// Triple-segment patterns find segments greedily but in order.
#[test]
fn multi_segment_glob_in_order() {
    let m = create_matcher(&pats(["a*b*c"]));
    assert!(m.matches("abc"));
    assert!(m.matches("axxbxxc"));
    assert!(!m.matches("acb"));
    // Last segment must be the suffix — `c` not at end fails.
    assert!(!m.matches("axbcx"));
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
