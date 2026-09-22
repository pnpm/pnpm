//! Literal-star matching and ordered include/ignore pattern lists.
//!
//! The pattern syntax is intentionally tiny: `*` is the only wildcard
//! (matching any sequence of characters, including empty), every other
//! character is matched literally. Pattern lists also interpret a leading
//! `!` as an ignore rule; [`WildcardMatcher`] treats it literally.
//!
//! The glob matcher is hand-rolled rather than backed by a regex engine:
//! the only wildcard is `*`, so a literal "starts with", "ends with", and
//! "contains in order" walk is enough.

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

/// A compiled glob pattern. The only wildcard is `*` (matches any
/// sequence including empty); every other character is literal. The
/// match is anchored — pattern must consume the whole input.

#[derive(Clone)]
pub struct WildcardMatcher {
    segments: Arc<[String]>,
    had_wildcard: bool,
    had_single_wildcard: bool,
}

impl WildcardMatcher {
    #[must_use]
    pub fn new(pattern: &str) -> Self {
        let segments: Vec<String> = pattern
            .split('*')
            .map(str::to_owned)
            .collect();
        let had_wildcard = segments.len() > 1;
        let had_single_wildcard = pattern.contains('?');
        WildcardMatcher { segments: segments.into(), had_wildcard, had_single_wildcard }
    }

    #[must_use]
    pub fn matches(&self, input: &str) -> bool {
        if !self.had_wildcard {
            if !self.had_single_wildcard {
                return self.segments[0] == input;
            }
            return segment_matches_exact(&self.segments[0], input);
        }
        let first = &self.segments[0];
        let last = &self.segments[self.segments.len() - 1];
        let Some(rest) = strip_prefix_pattern(input, first.as_str()) else {
            return false;
        };
        let Some(middle) = strip_suffix_pattern(rest, last.as_str()) else {
            return false;
        };
        contains_in_order(middle, &self.segments[1..self.segments.len() - 1])
    }
}

fn segment_matches_exact(pattern: &str, input: &str) -> bool {
    let mut pat_chars = pattern.chars();
    let mut inp_chars = input.chars();
    loop {
        match (pat_chars.next(), inp_chars.next()) {
            (None, None) => return true,
            (Some('?'), Some(_)) => continue,
            (Some(p), Some(i)) if p == i => continue,
            _ => return false,
        }
    }
}

fn strip_prefix_pattern<'a>(input: &'a str, pattern: &str) -> Option<&'a str> {
    let pat_chars = pattern.chars();
    let mut inp_chars = input.char_indices();
    for p in pat_chars {
        let (_, c) = inp_chars.next()?;
        if p != '?' && p != c {
            return None;
        }
    }
    let remainder_start = inp_chars.next().map_or(input.len(), |(idx, _)| idx);
    Some(&input[remainder_start..])
}

fn strip_suffix_pattern<'a>(input: &'a str, pattern: &str) -> Option<&'a str> {
    let pat_chars = pattern.chars().rev();
    let mut inp_chars = input.char_indices().rev();
    let mut last_idx = input.len();
    for p in pat_chars {
        let (idx, c) = inp_chars.next()?;
        if p != '?' && p != c {
            return None;
        }
        last_idx = idx;
    }
    Some(&input[..last_idx])
}

fn find_segment(input: &str, pattern: &str) -> Option<(usize, usize)> {
    if pattern.is_empty() {
        return Some((0, 0));
    }
    if !pattern.contains('?') {
        let idx = input.find(pattern)?;
        return Some((idx, pattern.len()));
    }
    for (start_idx, _) in input.char_indices() {
        let slice = &input[start_idx..];
        if let Some(rest) = strip_prefix_pattern(slice, pattern) {
            let match_len = slice.len() - rest.len();
            return Some((start_idx, match_len));
        }
    }
    None
}

fn contains_in_order(mut input: &str, segments: &[String]) -> bool {
    for segment in segments {
        if segment.is_empty() {
            continue;
        }
        let Some((idx, len)) = find_segment(input, segment.as_str()) else {
            return false;
        };
        input = &input[idx + len..];
    }
    true
}

#[cfg(test)]
mod tests;
