//! One `.npmignore` / `.gitignore` line, matched the way ignore-walk matches
//! it: minimatch with `matchBase`, `dot`, `nocase`, and `flipNegate`.

use super::segment::SegmentPattern;

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
        let negations = line
            .chars()
            .take_while(|&current| current == '!')
            .count();
        let alternatives = expand_braces(&line[negations..])
            .iter()
            .map(|pattern| parse_segments(pattern))
            .collect();
        IgnoreRule { negate: negations % 2 == 1, alternatives }
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
            .map_or("", |segment| segment);
        self.alternatives
            .iter()
            .any(|segments| {
                if segments.len() == 1 {
                    match_segments(&[basename], segments, partial)
                } else {
                    match_segments(&file, segments, partial)
                }
            })
    }
}

fn parse_segments(pattern: &str) -> Vec<Segment> {
    let mut segments: Vec<Segment> = Vec::new();
    for segment in split_on_slash_runs(pattern) {
        if segment == "**" {
            if !matches!(segments.last(), Some(Segment::Globstar)) {
                segments.push(Segment::Globstar);
            }
            continue;
        }
        segments.push(Segment::Pattern(SegmentPattern::parse(segment)));
    }
    segments
}

/// Splits like JavaScript's `split(/\/+/)`: a leading or trailing slash
/// yields an empty first or last segment.
fn split_on_slash_runs(path: &str) -> Vec<&str> {
    let parts: Vec<&str> = path.split('/').collect();
    let last = parts.len() - 1;
    parts
        .iter()
        .enumerate()
        .filter(|&(index, part)| !part.is_empty() || index == 0 || index == last)
        .map(|(_, part)| *part)
        .collect()
}

fn match_segments(file: &[&str], pattern: &[Segment], partial: bool) -> bool {
    for (index, segment) in pattern.iter().enumerate() {
        let Some(name) = file.get(index) else {
            return partial;
        };
        match segment {
            Segment::Globstar => {
                return match_globstar(&file[index..], &pattern[index + 1..], partial);
            }
            Segment::Pattern(segment_pattern) if !segment_pattern.matches(name) => return false,
            Segment::Pattern(_) => {}
        }
    }
    matches!(&file[pattern.len()..], [] | [""])
}

/// `**` swallows zero or more segments, but never `.` or `..`.
fn match_globstar(file: &[&str], rest: &[Segment], partial: bool) -> bool {
    let is_traversal = |segment: &&str| *segment == "." || *segment == "..";
    if rest.is_empty() {
        return !file.iter().any(is_traversal);
    }
    for start in 0..file.len() {
        if match_segments(&file[start..], rest, partial) {
            return true;
        }
        if is_traversal(&file[start]) {
            return false;
        }
    }
    partial
}

/// Expands `{a,b}` alternations, nested ones included. A brace pair without
/// a top-level comma stays literal.
fn expand_braces(pattern: &str) -> Vec<String> {
    let Some((open, close, commas)) = find_alternation(pattern) else {
        return vec![pattern.to_string()];
    };
    let prefix = &pattern[..open];
    let suffix = &pattern[close + 1..];
    let mut bounds = vec![open];
    bounds.extend(commas);
    bounds.push(close);
    bounds
        .windows(2)
        .flat_map(|window| {
            let expanded = format!("{prefix}{}{suffix}", &pattern[window[0] + 1..window[1]]);
            expand_braces(&expanded)
        })
        .collect()
}

/// The first `{…}` with a top-level comma: its byte offsets and the commas'.
fn find_alternation(pattern: &str) -> Option<(usize, usize, Vec<usize>)> {
    let bytes = pattern.as_bytes();
    let mut open_stack: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 1,
            b'{' => open_stack.push((index, Vec::new())),
            b',' => {
                if let Some((_, commas)) = open_stack.last_mut() {
                    commas.push(index);
                }
            }
            b'}' => {
                if let Some((open, commas)) = open_stack.pop()
                    && !commas.is_empty()
                {
                    return Some((open, index, commas));
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}
