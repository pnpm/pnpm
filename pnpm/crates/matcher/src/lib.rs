//! Glob matching and ordered include/ignore pattern lists.
//!
//! The pattern syntax supports `*` (matching any sequence of characters,
//! including empty) and `?` (matching any single character). Pattern lists
//! also interpret a leading `!` as an ignore rule; [`WildcardMatcher`] treats it literally.

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

/// A compiled glob pattern supporting `*` and `?` wildcards.
#[derive(Clone)]
pub struct WildcardMatcher {
    segments: Arc<[String]>,
    had_wildcard: bool,
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
        WildcardMatcher { segments: segments.into(), had_wildcard }
    }

    /// Returns whether the pattern consumes the whole input.
    #[must_use]
    pub fn matches(&self, input: &str) -> bool {
        if !self.had_wildcard {
            return segment_matches_exact(&self.segments[0], input);
        }
        let first = &self.segments[0];
        let last = &self.segments[self.segments.len() - 1];
        let Some(rest) = strip_segment_prefix(input, first.as_str()) else { return false };
        let Some(middle) = strip_segment_suffix(rest, last.as_str()) else { return false };
        contains_in_order(middle, &self.segments[1..self.segments.len() - 1])
    }
}

fn segment_matches_exact(segment: &str, target: &str) -> bool {
    if !segment.contains('?') {
        return segment == target;
    }
    let mut s_chars = segment.chars();
    let mut t_chars = target.chars();
    loop {
        match (s_chars.next(), t_chars.next()) {
            (Some('?'), Some(_)) => {}
            (Some(sc), Some(tc)) if sc == tc => {}
            (None, None) => return true,
            _ => return false,
        }
    }
}

fn strip_segment_prefix<'a>(input: &'a str, segment: &str) -> Option<&'a str> {
    if !segment.contains('?') {
        return input.strip_prefix(segment);
    }
    let seg_chars = segment.chars().count();
    let mut char_count = 0;
    let mut split_idx = input.len();
    for (idx, _) in input.char_indices() {
        if char_count == seg_chars {
            split_idx = idx;
            break;
        }
        char_count += 1;
    }
    if char_count < seg_chars {
        if char_count + 1 == seg_chars {
            split_idx = input.len();
        } else {
            return None;
        }
    }
    let prefix = &input[..split_idx];
    segment_matches_exact(segment, prefix).then(|| &input[split_idx..])
}

fn strip_segment_suffix<'a>(input: &'a str, segment: &str) -> Option<&'a str> {
    if !segment.contains('?') {
        return input.strip_suffix(segment);
    }
    let seg_chars = segment.chars().count();
    let input_chars = input.chars().count();
    if input_chars < seg_chars {
        return None;
    }
    let skip = input_chars - seg_chars;
    let split_idx = input
        .char_indices()
        .nth(skip)
        .map_or(input.len(), |(idx, _)| idx);
    let suffix = &input[split_idx..];
    segment_matches_exact(segment, suffix).then(|| &input[..split_idx])
}

/// Returns the byte length of the first `n` Unicode characters of `s`,
/// or `None` if `s` has fewer than `n` characters.
fn char_window_len(s: &str, n: usize) -> Option<usize> {
    let mut char_count = 0;
    for (idx, _) in s.char_indices() {
        if char_count == n {
            return Some(idx);
        }
        char_count += 1;
    }
    (char_count == n).then_some(s.len())
}

fn find_segment(input: &str, segment: &str) -> Option<(usize, usize)> {
    if !segment.contains('?') {
        return input
            .find(segment)
            .map(|idx| (idx, segment.len()));
    }
    let seg_chars = segment.chars().count();
    for (start_idx, _) in input.char_indices() {
        let rest = &input[start_idx..];
        let Some(end_offset) = char_window_len(rest, seg_chars) else {
            continue;
        };
        let candidate = &rest[..end_offset];
        if segment_matches_exact(segment, candidate) {
            return Some((start_idx, end_offset));
        }
    }
    None
}

/// Whether `segments` all occur in `input`, in order and without overlap.
fn contains_in_order(mut input: &str, segments: &[String]) -> bool {
    for segment in segments {
        if segment.is_empty() {
            continue;
        }
        let Some((idx, len)) = find_segment(input, segment.as_str()) else { return false };
        input = &input[idx + len..];
    }
    true
}

#[cfg(test)]
mod tests;
