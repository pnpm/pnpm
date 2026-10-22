//! One `.npmignore` / `.gitignore` line, matched the way ignore-walk matches
//! it: minimatch with `matchBase`, `dot`, `nocase`, and `flipNegate`.

mod sequence;

use super::segment::{LiteralCase, SegmentPattern};
use sequence::Sequence;
use std::{collections::HashSet, iter};

/// Brace expansion stops after this many alternatives, or once it has
/// produced this many bytes, so a short rule of repeated `{a,b}` groups
/// cannot exhaust memory. Alternatives past either limit are dropped.
const MAX_BRACE_ALTERNATIVES: usize = 1024;
const MAX_BRACE_EXPANSION_BYTES: usize = 1 << 20;

#[derive(Debug)]
pub(super) struct IgnoreRule {
    /// A leading `!`: a match includes the path instead of excluding it.
    pub(super) negate: bool,
    /// One alternative per brace expansion, each split on `/`.
    alternatives: Vec<Vec<Segment>>,
}

#[derive(Debug)]
enum Segment {
    Globstar,
    Pattern(SegmentPattern),
}

impl IgnoreRule {
    /// `line` is already trimmed and is neither empty nor a comment.
    pub(super) fn parse(line: &str) -> Self {
        Self::parse_with_case(line, LiteralCase::Unicode)
    }

    /// Like [`Self::parse`], but literal characters ignore ASCII case only.
    pub(super) fn parse_ascii_case(line: &str) -> Self {
        Self::parse_with_case(line, LiteralCase::Ascii)
    }

    fn parse_with_case(line: &str, literal_case: LiteralCase) -> Self {
        let negations = line
            .chars()
            .take_while(|&current| current == '!')
            .count();
        let alternatives = expand_braces(&line[negations..])
            .iter()
            .map(|pattern| parse_segments(pattern, literal_case))
            .collect();
        IgnoreRule { negate: negations % 2 == 1, alternatives }
    }

    /// npm-packlist's `!/<path>` strict rule for a package-relative path,
    /// except that `path` matches only itself, with exact case.
    pub(super) fn reinclude_path(path: &str) -> Self {
        let segments = split_on_slash_runs(&format!("/{path}"))
            .into_iter()
            .map(|segment| Segment::Pattern(SegmentPattern::exact(segment)))
            .collect();
        IgnoreRule { negate: true, alternatives: vec![segments] }
    }

    /// Whether some alternative is a single segment, optionally followed by
    /// a trailing `/`.
    pub(super) fn is_relative(&self) -> bool {
        self.alternatives
            .iter()
            .any(|segments| match segments.as_slice() {
                [_] => true,
                [_, Segment::Pattern(last)] => last.is_empty(),
                _ => false,
            })
    }

    /// With `partial`, a path that ends before the rule does still matches,
    /// so a directory whose contents the rule may match passes.
    pub(super) fn matches(&self, path: &str, partial: bool) -> bool {
        let file = split_on_slash_runs(path);
        let basename = file
            .iter()
            .rev()
            .find(|segment| !segment.is_empty())
            .copied()
            .unwrap_or("");
        self.alternatives
            .iter()
            .any(|segments| {
                let file = if segments.len() == 1 { &[basename][..] } else { &file };
                match_segments(file, segments, partial, &mut HashSet::new())
            })
    }
}

fn parse_segments(pattern: &str, literal_case: LiteralCase) -> Vec<Segment> {
    let mut segments: Vec<Segment> = Vec::new();
    for segment in split_on_slash_runs(pattern) {
        if segment == "**" {
            if !matches!(segments.last(), Some(Segment::Globstar)) {
                segments.push(Segment::Globstar);
            }
            continue;
        }
        segments.push(Segment::Pattern(SegmentPattern::parse(segment, literal_case)));
    }
    segments
}

/// Split slash runs, preserving Windows UNC roots as minimatch does.
fn split_on_slash_runs(path: &str) -> Vec<&str> {
    let preserve_unc = cfg!(windows)
        && path
            .strip_prefix("//")
            .is_some_and(|suffix| !suffix.is_empty() && !suffix.starts_with('/'));
    let parts: Vec<&str> = path.split('/').collect();
    let last = parts.len() - 1;
    parts
        .iter()
        .enumerate()
        .filter(|&(index, part)| {
            !part.is_empty() || index == 0 || index == last || (preserve_unc && index == 1)
        })
        .map(|(_, part)| *part)
        .collect()
}

/// Suffix lengths of the path and of the pattern known not to match. `**`
/// retries the same suffixes from many starting points, and recording the
/// failures keeps that polynomial.
type FailedSuffixes = HashSet<(usize, usize)>;

fn match_segments(
    file: &[&str],
    pattern: &[Segment],
    partial: bool,
    failed: &mut FailedSuffixes,
) -> bool {
    for (index, segment) in pattern.iter().enumerate() {
        let Some(name) = file.get(index) else {
            return partial;
        };
        match segment {
            Segment::Globstar => {
                return match_globstar(&file[index..], &pattern[index + 1..], partial, failed);
            }
            Segment::Pattern(segment_pattern) if !segment_pattern.matches(name) => return false,
            Segment::Pattern(_) => {}
        }
    }
    matches!(&file[pattern.len()..], [] | [""])
}

/// `**` swallows zero or more segments, but never `.` or `..`.
fn match_globstar(
    file: &[&str],
    rest: &[Segment],
    partial: bool,
    failed: &mut FailedSuffixes,
) -> bool {
    let is_traversal = |segment: &&str| *segment == "." || *segment == "..";
    if rest.is_empty() {
        return !file.iter().any(is_traversal);
    }
    for start in 0..file.len() {
        let suffix = &file[start..];
        let key = (suffix.len(), rest.len());
        if !failed.contains(&key) {
            if match_segments(suffix, rest, partial, failed) {
                return true;
            }
            failed.insert(key);
        }
        if is_traversal(&file[start]) {
            return false;
        }
    }
    partial
}

/// Expands brace alternatives and sequences up to
/// [`MAX_BRACE_ALTERNATIVES`] and [`MAX_BRACE_EXPANSION_BYTES`].
fn expand_braces(pattern: &str) -> Vec<String> {
    let mut pending = vec![pattern.to_string()];
    let mut expanded = Vec::new();
    let mut produced_bytes = 0;
    while let Some(current) = pending.pop() {
        let Some(expansion) = find_expansion(&current) else {
            expanded.push(current);
            if expanded.len() >= MAX_BRACE_ALTERNATIVES {
                break;
            }
            continue;
        };
        let Some((patterns, bytes)) = expansion.expand(
            &current,
            MAX_BRACE_ALTERNATIVES - expanded.len(),
            MAX_BRACE_EXPANSION_BYTES - produced_bytes,
        ) else {
            break;
        };
        produced_bytes += bytes;
        pending.extend(patterns.into_iter().rev());
    }
    expanded
}

enum Expansion {
    Alternation(Alternation),
    Range { open: usize, close: usize, range: Sequence },
}

impl Expansion {
    fn expand(
        &self,
        pattern: &str,
        max_arms: usize,
        byte_limit: usize,
    ) -> Option<(Vec<String>, usize)> {
        match self {
            Self::Alternation(alternation) => {
                let arms = alternation.arms(max_arms);
                let bytes = alternation.expanded_bytes(pattern, &arms);
                if bytes > byte_limit {
                    return None;
                }
                Some((alternation.expand(pattern, &arms), bytes))
            }
            Self::Range { open, close, range } => {
                range.expand(&pattern[..*open], &pattern[close + 1..], max_arms, byte_limit)
            }
        }
    }
}

/// Byte offsets of a `{`, its `}`, and the top-level commas between them.
/// At most [`MAX_BRACE_ALTERNATIVES`] commas are recorded.
struct Alternation {
    open: usize,
    close: usize,
    commas: Vec<usize>,
}

impl Alternation {
    /// The byte ranges of the first `max_arms` arms.
    fn arms(&self, max_arms: usize) -> Vec<(usize, usize)> {
        let bounds: Vec<usize> = iter::once(self.open)
            .chain(self.commas.iter().copied())
            .chain(iter::once(self.close))
            .collect();
        bounds
            .windows(2)
            .take(max_arms)
            .map(|window| (window[0] + 1, window[1]))
            .collect()
    }

    /// The bytes [`Alternation::expand`] would produce, counting each arm as
    /// at least one byte so that empty arms count too.
    fn expanded_bytes(&self, pattern: &str, arms: &[(usize, usize)]) -> usize {
        let affixes = self.open + (pattern.len() - self.close - 1) + 1;
        arms.iter()
            .map(|(start, end)| affixes + (end - start))
            .sum()
    }

    fn expand(&self, pattern: &str, arms: &[(usize, usize)]) -> Vec<String> {
        let prefix = &pattern[..self.open];
        let suffix = &pattern[self.close + 1..];
        arms.iter()
            .map(|&(start, end)| format!("{prefix}{}{suffix}", &pattern[start..end]))
            .collect()
    }
}

/// The first completed brace group with alternatives or a sequence.
fn find_expansion(pattern: &str) -> Option<Expansion> {
    let bytes = pattern.as_bytes();
    let mut open_stack: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 1,
            b'{' => open_stack.push((index, Vec::new())),
            b',' => record_comma(&mut open_stack, index),
            b'}' => {
                if let Some((open, commas)) = open_stack.pop()
                    && let Some(expansion) = completed_expansion(pattern, open, index, commas)
                {
                    return Some(expansion);
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

fn completed_expansion(
    pattern: &str,
    open: usize,
    close: usize,
    commas: Vec<usize>,
) -> Option<Expansion> {
    if !commas.is_empty() {
        return Some(Expansion::Alternation(Alternation { open, close, commas }));
    }
    Sequence::parse(&pattern[open + 1..close]).map(|range| Expansion::Range { open, close, range })
}

/// Records a comma for the innermost open brace, up to
/// [`MAX_BRACE_ALTERNATIVES`] commas per brace.
fn record_comma(open_stack: &mut [(usize, Vec<usize>)], index: usize) {
    if let Some((_, commas)) = open_stack.last_mut()
        && commas.len() < MAX_BRACE_ALTERNATIVES
    {
        commas.push(index);
    }
}
