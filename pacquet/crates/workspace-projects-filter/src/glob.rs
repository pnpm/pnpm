//! Minimal path glob matcher for directory selectors, covering the
//! `micromatch.isMatch(dir, pattern, { format })` call upstream uses for
//! `useGlobDirFiltering` selections.
//!
//! Only the two wildcards directory filters rely on are supported: `*`
//! matches any run of characters within a single path segment, and `**`
//! matches any number of whole segments (including zero). Both the
//! pattern and the candidate have a trailing `/` stripped (upstream's
//! `format`), and the pattern's backslashes are normalized to `/`
//! (upstream's `replace(/\\/g, '/')`).

/// Whether `candidate` matches the directory glob `pattern`.
pub fn is_match(candidate: &str, pattern: &str) -> bool {
    let pattern = pattern.replace('\\', "/");
    let pattern = pattern.strip_suffix('/').unwrap_or(&pattern);
    let candidate = candidate.strip_suffix('/').unwrap_or(candidate);

    let pattern_segments: Vec<&str> = pattern.split('/').collect();
    let candidate_segments: Vec<&str> = candidate.split('/').collect();
    match_segments(&pattern_segments, &candidate_segments)
}

fn match_segments(pattern: &[&str], candidate: &[&str]) -> bool {
    match pattern.split_first() {
        None => candidate.is_empty(),
        Some((&"**", rest)) => {
            // Globstar matches zero or more whole segments.
            (0..=candidate.len()).any(|skip| match_segments(rest, &candidate[skip..]))
        }
        Some((&segment, rest)) => match candidate.split_first() {
            Some((&head, tail)) if segment_match(segment, head) => match_segments(rest, tail),
            _ => false,
        },
    }
}

/// Match a single pattern segment against a single candidate segment,
/// treating `*` as any (possibly empty) run of characters.
fn segment_match(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }

    let Some(remainder) = text.strip_prefix(parts[0]) else {
        return false;
    };
    let mut remainder = remainder;
    for part in &parts[1..parts.len() - 1] {
        match remainder.find(part) {
            Some(found) => remainder = &remainder[found + part.len()..],
            None => return false,
        }
    }
    remainder.ends_with(parts[parts.len() - 1])
}

#[cfg(test)]
mod tests;
