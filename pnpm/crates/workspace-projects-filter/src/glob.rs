//! Path glob matcher for directory selectors, covering the
//! `micromatch.isMatch(dir, pattern, { format })` call upstream uses for
//! `useGlobDirFiltering` selections.
//!
//! `wax` provides the glob syntax used by micromatch for directory
//! selectors, including `*`, `**`, `?`, and character classes. Windows
//! drive-prefixed paths are not valid wax expressions, so their drive is
//! matched separately and the normalized path tail is passed to `wax`.

use wax::{Glob, Program};

/// Whether `candidate` matches the directory glob `pattern`.
pub fn is_match(candidate: &str, pattern: &str) -> bool {
    let pattern = normalize(pattern);
    let candidate = normalize(candidate);

    if let Some((pattern_drive, pattern_tail)) = split_windows_drive(&pattern) {
        let Some((candidate_drive, candidate_tail)) = split_windows_drive(&candidate) else {
            return false;
        };
        if !pattern_drive.eq_ignore_ascii_case(candidate_drive) {
            return false;
        }
        if let Ok(glob) = Glob::new(pattern_tail) {
            return glob.is_match(candidate_tail);
        }
    }

    if let Ok(glob) = Glob::new(&pattern) {
        return glob.is_match(candidate.as_str());
    }

    // Wax intentionally rejects Windows drive prefixes. Keep matching those
    // paths so directory filters remain portable across platforms.
    let pattern_segments: Vec<&str> = pattern.split('/').collect();
    let candidate_segments: Vec<&str> = candidate.split('/').collect();
    match_segments(&pattern_segments, &candidate_segments)
}

/// Split a normalized Windows drive path into its drive and slash-prefixed
/// path. Wax intentionally does not accept drive prefixes in glob expressions.
fn split_windows_drive(path: &str) -> Option<(&str, &str)> {
    let bytes = path.as_bytes();
    (bytes.len() >= 3 && bytes[1] == b':' && bytes[2] == b'/').then(|| (&path[..2], &path[2..]))
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

/// Match a single pattern segment against a single candidate segment for
/// patterns that wax cannot parse, such as Windows drive-prefixed paths.
/// This preserves the original `*`/`**` behavior for that compatibility path.
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
