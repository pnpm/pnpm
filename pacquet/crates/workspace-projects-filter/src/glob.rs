//! Minimal path glob matcher for directory selectors, covering the
//! `micromatch.isMatch(dir, pattern, { format })` call upstream uses for
//! `useGlobDirFiltering` selections.
//!
//! Only the two wildcards directory filters rely on are supported: `*`
//! matches any run of characters within a single path segment, and `**`
//! matches any number of whole segments (including zero). Both the
//! pattern and the candidate are normalized the same way before
//! matching: backslashes become `/` and a trailing `/` is stripped.
//! This mirrors upstream's pattern `replace(/\\/g, '/')` together with
//! micromatch's separator handling, which treats `\` in the candidate
//! as a path separator too — so a Windows `ProjectRootDir` rendered with
//! backslashes by `PathBuf::to_string_lossy()` still matches.

/// Whether `candidate` matches the directory glob `pattern`.
pub fn is_match(candidate: &str, pattern: &str) -> bool {
    let pattern = normalize(pattern);
    let candidate = normalize(candidate);

    let pattern_segments: Vec<&str> = pattern.split('/').collect();
    let candidate_segments: Vec<&str> = candidate.split('/').collect();
    match_segments(&pattern_segments, &candidate_segments)
}

/// Normalize a glob pattern or candidate path: backslashes to `/`, then
/// a single trailing `/` stripped.
fn normalize(path: &str) -> String {
    let path = path.replace('\\', "/");
    match path.strip_suffix('/') {
        Some(stripped) => stripped.to_string(),
        None => path,
    }
}

fn match_segments(pattern: &[&str], candidate: &[&str]) -> bool {
    match pattern.split_first() {
        None => candidate.is_empty(),
        Some((&"**", rest)) => {
            (0..=candidate.len()).any(|skip| match_segments(rest, &candidate[skip..]))
        }
        Some((&segment, rest)) => match candidate.split_first() {
            Some((&head, tail)) if segment_match(segment, head) => match_segments(rest, tail),
            _ => false,
        },
    }
}

/// Match a single pattern segment against a single candidate segment,
/// treating `*` as any (possibly empty) run of characters. Uses the
/// classic iterative wildcard match with backtracking so multiple `*`
/// in one segment (`a*b*c`) match correctly.
fn segment_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut pat, mut txt) = (0usize, 0usize);
    // The last `*` seen and the text position it was matched against, so
    // a failed match can backtrack and let the `*` consume one more char.
    let mut backtrack: Option<(usize, usize)> = None;

    while txt < text.len() {
        if pattern.get(pat) == Some(&'*') {
            backtrack = Some((pat, txt));
            pat += 1;
        } else if pattern.get(pat) == Some(&text[txt]) {
            pat += 1;
            txt += 1;
        } else if let Some((star_pat, star_txt)) = backtrack {
            pat = star_pat + 1;
            txt = star_txt + 1;
            backtrack = Some((star_pat, txt));
        } else {
            return false;
        }
    }
    while pattern.get(pat) == Some(&'*') {
        pat += 1;
    }
    pat == pattern.len()
}

#[cfg(test)]
mod tests;
