//! Glob matching and ordered include/ignore pattern lists.
//!
//! The pattern syntax is intentionally tiny: `*` matches any sequence of
//! characters (including empty), `?` matches one character, and every other
//! character is matched literally. Pattern lists also interpret a leading `!`
//! as an ignore rule; [`WildcardMatcher`] treats it literally.
//!
//! The glob matcher is hand-rolled rather than backed by a regex engine.
//! Patterns without `?` use a literal-segment walk; the character-aware
//! fallback handles `?` patterns.

use std::sync::Arc;

/// Compile a list of patterns into a matcher returning the index of the
/// first matching include, or `None` when nothing matches.
///
/// The match position is an `Option<usize>` rather than an `i32` /
/// `-1`-sentinel — the same information, idiomatic for Rust.
#[must_use]
pub fn create_matcher_with_index(patterns: &[String]) -> MatcherWithIndex {
    match patterns.len() {
        0 => MatcherWithIndex(MatcherImpl::Never),
        1 => MatcherWithIndex(MatcherImpl::Single(compile_single(&patterns[0]))),
        _ => MatcherWithIndex(compile_many(patterns)),
    }
}

/// Compile a list of patterns into a matcher returning `true` whenever
/// any include matches and no ignore overrides it.
#[must_use]
pub fn create_matcher(patterns: &[String]) -> Matcher {
    Matcher(create_matcher_with_index(patterns))
}

/// Boolean matcher — opaque wrapper around [`MatcherWithIndex`].
#[derive(Clone)]
pub struct Matcher(MatcherWithIndex);

impl Matcher {
    /// Returns `true` when `input` matches at least one include and no
    /// ignore rule overrides it. Empty pattern lists never match.
    #[must_use]
    pub fn matches(&self, input: &str) -> bool {
        self.0.matches(input).is_some()
    }

    /// `true` iff this matcher is statically guaranteed to never
    /// match any input — i.e. compiled from an empty pattern list.
    /// Lets callers short-circuit before they walk a graph and call
    /// [`Self::matches`] for every alias.
    ///
    /// A matcher built from non-empty patterns returns `false` here
    /// even when no realistic input would match (e.g. `["nonexistent-prefix-*"]`)
    /// — the fast path is a static check on the pattern list, not a
    /// runtime analysis of the compiled regex shape.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        matches!(self.0.0, MatcherImpl::Never)
    }
}

/// Matcher returning the *index* of the include that matched.
#[derive(Clone)]
pub struct MatcherWithIndex(MatcherImpl);

impl MatcherWithIndex {
    #[must_use]
    pub fn matches(&self, input: &str) -> Option<usize> {
        self.0.matches(input)
    }
}

#[derive(Clone)]
enum MatcherImpl {
    /// Empty pattern list.
    Never,
    /// Single-pattern fast path.
    Single(SingleMatcher),
    /// Many-pattern path with no ignore rules.
    AllInclude(Arc<[CompiledPattern]>),
    /// Many-pattern path with no include rules.
    AllIgnore(Arc<[CompiledPattern]>),
    /// Mixed includes and ignores.
    Mixed(Arc<[CompiledPattern]>),
}

impl MatcherImpl {
    fn matches(&self, input: &str) -> Option<usize> {
        match self {
            MatcherImpl::Never => None,
            MatcherImpl::Single(single) => single.matches(input),
            MatcherImpl::AllInclude(patterns) => first_include(patterns, input),
            MatcherImpl::AllIgnore(patterns) => none_ignores(patterns, input),
            MatcherImpl::Mixed(patterns) => last_verdict(patterns, input),
        }
    }
}

/// The first include pattern that matches, by position.
fn first_include(patterns: &[CompiledPattern], input: &str) -> Option<usize> {
    patterns
        .iter()
        .position(|pattern| {
            debug_assert!(!pattern.is_ignore);
            pattern.matches(input)
        })
}

/// Position `0` unless an ignore pattern matches: with no include rules,
/// everything the ignores leave alone is included.
fn none_ignores(patterns: &[CompiledPattern], input: &str) -> Option<usize> {
    let ignored = patterns
        .iter()
        .any(|pattern| {
            debug_assert!(pattern.is_ignore);
            pattern.matches(input)
        });
    (!ignored).then_some(0)
}

/// Includes and ignores in one list: a later ignore cancels an earlier
/// include, and only the first include after it counts again.
fn last_verdict(patterns: &[CompiledPattern], input: &str) -> Option<usize> {
    let mut sticky: Option<usize> = None;
    for (index, pattern) in patterns.iter().enumerate() {
        if !pattern.matches(input) {
            continue;
        }
        if pattern.is_ignore {
            sticky = None;
        } else if sticky.is_none() {
            sticky = Some(index);
        }
    }
    sticky
}

#[derive(Clone)]
struct SingleMatcher {
    glob: WildcardMatcher,
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
    glob: WildcardMatcher,
    is_ignore: bool,
}

impl CompiledPattern {
    fn matches(&self, input: &str) -> bool {
        self.glob.matches(input)
    }
}

fn compile_single(pattern: &str) -> SingleMatcher {
    if let Some(rest) = pattern.strip_prefix('!') {
        SingleMatcher { glob: WildcardMatcher::new(rest), is_ignore: true }
    } else {
        SingleMatcher { glob: WildcardMatcher::new(pattern), is_ignore: false }
    }
}

fn compile_many(patterns: &[String]) -> MatcherImpl {
    let mut compiled: Vec<CompiledPattern> = Vec::with_capacity(patterns.len());
    let mut has_include = false;
    let mut has_ignore = false;
    for pattern in patterns {
        if let Some(rest) = pattern.strip_prefix('!') {
            has_ignore = true;
            compiled.push(CompiledPattern { glob: WildcardMatcher::new(rest), is_ignore: true });
        } else {
            has_include = true;
            compiled.push(CompiledPattern {
                glob: WildcardMatcher::new(pattern),
                is_ignore: false,
            });
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

/// A compiled glob pattern. `*` matches any sequence including empty,
/// `?` matches one character, and every other character is literal. The
/// match is anchored — pattern must consume the whole input.

#[derive(Clone)]
pub struct WildcardMatcher {
    /// Segments between `*`s. For pattern `a*b*c` this is
    /// `["a", "b", "c"]`. For `*` alone it is `["", ""]`. For pure
    /// literal `foo` it is `["foo"]` and `had_wildcard` is false.
    segments: Arc<[String]>,
    had_wildcard: bool,
    question_pattern: Option<Arc<[char]>>,
}

impl WildcardMatcher {
    /// Compiles a pattern. A leading `!` is literal, not an ignore rule.
    #[must_use]
    pub fn new(pattern: &str) -> Self {
        let segments: Vec<String> = pattern
            .split('*')
            .map(str::to_owned)
            .collect();
        let had_wildcard = segments.len() > 1;
        let question_pattern = pattern
            .contains('?')
            .then(|| {
                pattern
                    .chars()
                    .collect::<Vec<_>>()
                    .into()
            });
        WildcardMatcher { segments: segments.into(), had_wildcard, question_pattern }
    }

    /// Returns whether the pattern consumes the whole input.
    #[must_use]
    pub fn matches(&self, input: &str) -> bool {
        if let Some(pattern) = &self.question_pattern {
            return matches_question_pattern(pattern, input);
        }
        if !self.had_wildcard {
            return self.segments[0] == input;
        }
        let first = &self.segments[0];
        let last = &self.segments[self.segments.len() - 1];
        let Some(rest) = input.strip_prefix(first.as_str()) else { return false };
        if first.len() + last.len() > input.len() {
            return false;
        }
        let Some(middle) = rest.strip_suffix(last.as_str()) else { return false };
        // The prefix-strip already advanced past `first`; the
        // suffix-strip already accounted for `last`. Walk the
        // middle segments greedily.
        contains_in_order(middle, &self.segments[1..self.segments.len() - 1])
    }
}

fn matches_question_pattern(pattern: &[char], input: &str) -> bool {
    let mut pattern_index = 0;
    let mut input_index = 0;
    let mut star_pattern_index = None;
    let mut star_input_index = 0;

    while input_index < input.len() {
        match pattern.get(pattern_index) {
            Some('?') => {
                pattern_index += 1;
                input_index = next_char_index(input, input_index);
            }
            Some('*') => {
                star_pattern_index = Some(pattern_index);
                pattern_index += 1;
                star_input_index = input_index;
            }
            Some(&literal) if input[input_index..].starts_with(literal) => {
                pattern_index += 1;
                input_index = next_char_index(input, input_index);
            }
            _ => {
                let Some(star) = star_pattern_index else { return false };
                star_input_index = next_char_index(input, star_input_index);
                input_index = star_input_index;
                pattern_index = star + 1;
            }
        }
    }

    pattern[pattern_index..]
        .iter()
        .all(|character| *character == '*')
}

fn next_char_index(input: &str, index: usize) -> usize {
    let character = input[index..]
        .chars()
        .next()
        .expect("input index must point at a character");
    index + character.len_utf8()
}

/// Whether `segments` all occur in `input`, in order and without overlap.
/// An empty segment is two adjacent wildcards and constrains nothing.
fn contains_in_order(mut input: &str, segments: &[String]) -> bool {
    for segment in segments {
        if segment.is_empty() {
            continue;
        }
        let Some(index) = input.find(segment.as_str()) else { return false };
        input = &input[index + segment.len()..];
    }
    true
}

#[cfg(test)]
mod tests;
