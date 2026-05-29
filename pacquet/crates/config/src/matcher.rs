//! Glob-pattern matcher used by `hoistPattern` and `publicHoistPattern`.
//!
//! Port of upstream's [`@pnpm/config.matcher`](https://github.com/pnpm/pnpm/blob/94240bc046/config/matcher/src/index.ts).
//! The pattern syntax is intentionally tiny: `*` is the only wildcard
//! (matching any sequence of characters, including empty), every other
//! character is matched literally, and a leading `!` flips a pattern into
//! an ignore rule. There is no `?`, no character class, and no escape;
//! pnpm's `escapeStringRegexp` step before the regex compile means
//! every non-`*` byte is literal.
//!
//! The semantics mirror [`createMatcher` /
//! `createMatcherWithIndex`](https://github.com/pnpm/pnpm/blob/94240bc046/config/matcher/src/index.ts#L7-L40):
//!
//! - Empty pattern list: nothing matches.
//! - All-include patterns: first include that matches wins (its index
//!   is returned).
//! - All-ignore patterns: input matches when *no* ignore matches (returns
//!   `0` instead of `-1` to keep `Some(_)` semantics non-empty); when any
//!   ignore matches the input, the matcher returns `None`.
//! - Mixed includes + ignores: include's index sticks unless a later
//!   ignore matches, in which case the index resets to "no match"
//!   (regardless of whether another include comes after — order matters).
//!
//! Pacquet skips upstream's `regex` dependency by hand-rolling the
//! glob matcher: the only wildcard is `*`, so a literal "starts with",
//! "ends with", and "contains in order" walk is enough. Avoiding regex
//! also avoids inheriting its char-class quirks (`?`, `.`, `(`, etc.
//! stay literal here just like upstream's `escapeStringRegexp` ensures
//! they stay literal there).

use std::sync::Arc;

/// Compile a list of patterns into a matcher returning the index of the
/// first matching include, or `None` when nothing matches.
///
/// Mirrors upstream's [`createMatcherWithIndex`](https://github.com/pnpm/pnpm/blob/94240bc046/config/matcher/src/index.ts#L16-L40).
/// The numeric index is `Option<usize>` here rather than `i32` /
/// `-1`-sentinel — the same information, idiomatic for Rust.
pub fn create_matcher_with_index(patterns: &[String]) -> MatcherWithIndex {
    match patterns.len() {
        0 => MatcherWithIndex(MatcherImpl::Never),
        1 => MatcherWithIndex(MatcherImpl::Single(compile_single(&patterns[0]))),
        _ => MatcherWithIndex(compile_many(patterns)),
    }
}

/// Compile a list of patterns into a matcher returning `true` whenever
/// any include matches and no ignore overrides it. Mirrors upstream's
/// [`createMatcher`](https://github.com/pnpm/pnpm/blob/94240bc046/config/matcher/src/index.ts#L7-L10).
pub fn create_matcher(patterns: &[String]) -> Matcher {
    Matcher(create_matcher_with_index(patterns))
}

/// Boolean matcher — opaque wrapper around [`MatcherWithIndex`].
#[derive(Clone)]
pub struct Matcher(MatcherWithIndex);

impl Matcher {
    /// Returns `true` when `input` matches at least one include and no
    /// ignore rule overrides it. Empty pattern lists never match.
    pub fn matches(&self, input: &str) -> bool {
        self.0.matches(input).is_some()
    }

    /// `true` iff this matcher is statically guaranteed to never
    /// match any input — i.e. compiled from an empty pattern list.
    /// Lets callers short-circuit before they walk a graph and call
    /// [`Self::matches`] for every alias. Mirrors upstream's
    /// `case 0: return () => -1` fast path.
    ///
    /// A matcher built from non-empty patterns returns `false` here
    /// even when no realistic input would match (e.g. `["nonexistent-prefix-*"]`)
    /// — the fast path is a static check on the pattern list, not a
    /// runtime analysis of the compiled regex shape.
    pub fn is_empty(&self) -> bool {
        matches!(self.0.0, MatcherImpl::Never)
    }
}

/// Matcher returning the *index* of the include that matched. Empty
/// pattern lists return `None`. All-ignore lists return `Some(0)` when
/// no ignore matched (mirrors upstream's `0` literal in
/// `matchInputWithoutIgnoreMatchers`).
#[derive(Clone)]
pub struct MatcherWithIndex(MatcherImpl);

impl MatcherWithIndex {
    pub fn matches(&self, input: &str) -> Option<usize> {
        self.0.matches(input)
    }
}

#[derive(Clone)]
enum MatcherImpl {
    /// Empty pattern list — never matches.
    Never,
    /// Single-pattern fast path (matches upstream's
    /// `matcherWhenOnlyOnePatternWithIndex`). Returns `Some(0)` on
    /// match, `None` otherwise — including when the lone pattern is
    /// an ignore that matches.
    Single(SingleMatcher),
    /// Many-pattern path with no ignore rules. First matching include
    /// wins.
    AllInclude(Arc<[CompiledPattern]>),
    /// Many-pattern path with no include rules. Matches `Some(0)`
    /// unless any ignore matches.
    AllIgnore(Arc<[CompiledPattern]>),
    /// Mixed includes and ignores. Walk in declaration order: a
    /// matching include claims the sticky slot when it's empty; a
    /// matching ignore clears it. Once cleared, a later matching
    /// include can re-take the slot — net result is "first include
    /// after the last matching ignore wins". Mirrors upstream's
    /// `matchInputWithMatchersArray` and the
    /// `eslint-*`, `!eslint-plugin-*`, `eslint-plugin-bar` test
    /// case where `eslint-plugin-bar` matches via index 2 even
    /// though earlier `eslint-*` (index 0) was wiped by
    /// `!eslint-plugin-*`.
    Mixed(Arc<[CompiledPattern]>),
}

impl MatcherImpl {
    fn matches(&self, input: &str) -> Option<usize> {
        match self {
            MatcherImpl::Never => None,
            MatcherImpl::Single(s) => s.matches(input),
            MatcherImpl::AllInclude(patterns) => {
                for (i, p) in patterns.iter().enumerate() {
                    debug_assert!(!p.is_ignore);
                    if p.matches(input) {
                        return Some(i);
                    }
                }
                None
            }
            MatcherImpl::AllIgnore(patterns) => {
                for p in patterns.iter() {
                    debug_assert!(p.is_ignore);
                    if p.matches(input) {
                        return None;
                    }
                }
                Some(0)
            }
            MatcherImpl::Mixed(patterns) => {
                let mut sticky: Option<usize> = None;
                for (i, p) in patterns.iter().enumerate() {
                    if p.is_ignore {
                        if p.matches(input) {
                            sticky = None;
                        }
                    } else if sticky.is_none() && p.matches(input) {
                        sticky = Some(i);
                    }
                }
                sticky
            }
        }
    }
}

#[derive(Clone)]
struct SingleMatcher {
    glob: Glob,
    is_ignore: bool,
}

impl SingleMatcher {
    fn matches(&self, input: &str) -> Option<usize> {
        let raw = self.glob.matches(input);
        let matched = if self.is_ignore { !raw } else { raw };
        matched.then_some(0)
    }
}

#[derive(Clone)]
struct CompiledPattern {
    glob: Glob,
    is_ignore: bool,
}

impl CompiledPattern {
    fn matches(&self, input: &str) -> bool {
        self.glob.matches(input)
    }
}

fn compile_single(pattern: &str) -> SingleMatcher {
    if let Some(rest) = pattern.strip_prefix('!') {
        SingleMatcher { glob: Glob::compile(rest), is_ignore: true }
    } else {
        SingleMatcher { glob: Glob::compile(pattern), is_ignore: false }
    }
}

fn compile_many(patterns: &[String]) -> MatcherImpl {
    let mut compiled: Vec<CompiledPattern> = Vec::with_capacity(patterns.len());
    let mut has_include = false;
    let mut has_ignore = false;
    for pattern in patterns {
        if let Some(rest) = pattern.strip_prefix('!') {
            has_ignore = true;
            compiled.push(CompiledPattern { glob: Glob::compile(rest), is_ignore: true });
        } else {
            has_include = true;
            compiled.push(CompiledPattern { glob: Glob::compile(pattern), is_ignore: false });
        }
    }
    let arc: Arc<[CompiledPattern]> = compiled.into();
    match (has_include, has_ignore) {
        (true, false) => MatcherImpl::AllInclude(arc),
        (false, true) => MatcherImpl::AllIgnore(arc),
        // The two-pattern paths above always set at least one of the
        // booleans; this arm is only reached when both are true (the
        // mixed case) or when `patterns` is empty (handled by the
        // caller). Treat unreachable-by-construction cases as Mixed
        // for safety.
        _ => MatcherImpl::Mixed(arc),
    }
}

/// A compiled glob pattern. The only wildcard is `*` (matches any
/// sequence including empty); every other character is literal. The
/// match is anchored — pattern must consume the whole input.
///
/// Representation choice: split the pattern by `*` and store the
/// literal segments. Match by checking that segment 0 is a prefix,
/// the last segment is a suffix, and intermediate segments appear in
/// order in the remaining slice. This is O(|input| * |segments|),
/// which is fine for the few short patterns hoisting deals with.
#[derive(Clone)]
struct Glob {
    /// Segments between `*`s. For pattern `a*b*c` this is
    /// `["a", "b", "c"]`. For `*` alone it is `["", ""]`. For pure
    /// literal `foo` it is `["foo"]` and `had_wildcard` is false.
    segments: Arc<[String]>,
    had_wildcard: bool,
}

impl Glob {
    fn compile(pattern: &str) -> Self {
        let segments: Vec<String> = pattern.split('*').map(str::to_owned).collect();
        let had_wildcard = segments.len() > 1;
        Glob { segments: segments.into(), had_wildcard }
    }

    fn matches(&self, input: &str) -> bool {
        if !self.had_wildcard {
            // `segments.len() == 1`. Pattern is a literal — exact
            // string equality.
            return self.segments[0] == input;
        }
        // First segment is a prefix; last segment is a suffix; in
        // between, each segment must appear in order, non-overlapping.
        let first = &self.segments[0];
        let last = &self.segments[self.segments.len() - 1];
        let Some(rest) = input.strip_prefix(first.as_str()) else { return false };
        if first.len() + last.len() > input.len() {
            return false;
        }
        let Some(mut middle) = rest.strip_suffix(last.as_str()) else { return false };
        // The prefix-strip already advanced past `first`; the
        // suffix-strip already accounted for `last`. Walk the
        // middle segments greedily.
        if self.segments.len() > 2 {
            for seg in &self.segments[1..self.segments.len() - 1] {
                if seg.is_empty() {
                    continue;
                }
                let Some(idx) = middle.find(seg.as_str()) else { return false };
                middle = &middle[idx + seg.len()..];
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
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

        let m =
            create_matcher_with_index(&pats(["eslint-*", "!eslint-plugin-*", "eslint-plugin-bar"]));
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
}
