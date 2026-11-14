use super::modules_overlap::{escape_wax_path, paths_match_on_host, relative_subpath_posix};
use pnpm_fs::{lexical_normalize, lexical_normalize_posix};
use std::path::Path;
use wax::Program as _;

const MAX_BRACE_EXPANSIONS: usize = 64;

pub(super) fn patterns_entering_root_dir(
    workspace_dir: &Path,
    root_clean_dir: &Path,
    patterns: Option<&[String]>,
    custom_leaves: &[String],
) -> Option<Vec<String>> {
    let patterns = patterns?;
    let rel_target = relative_root_dir_posix(workspace_dir, root_clean_dir)?;
    let target_segments: Vec<&str> = rel_target.split('/').collect();
    let mut includes = Vec::new();
    let mut negations = Vec::new();
    for pattern in patterns {
        if pattern.starts_with('!') {
            negations.push(pattern.clone());
        } else {
            includes.extend(matching_recovery_branches(pattern, &target_segments));
        }
    }
    if includes.is_empty() {
        return None;
    }
    includes.extend(negations);
    for leaf in custom_leaves {
        includes.push(format!("!*/**/{}/**", escape_wax_path(leaf)));
    }
    Some(includes)
}

fn relative_root_dir_posix(workspace_dir: &Path, target_dir: &Path) -> Option<String> {
    let workspace_norm = lexical_normalize(workspace_dir);
    let target_norm = lexical_normalize(target_dir);
    let rel = target_norm
        .strip_prefix(&workspace_norm)
        .ok()
        .map(Path::to_path_buf)
        .or_else(|| {
            let real_ws = dunce::canonicalize(workspace_dir).ok()?;
            let real_target = dunce::canonicalize(target_dir).ok()?;
            real_target
                .strip_prefix(&real_ws)
                .ok()
                .map(Path::to_path_buf)
        })?;
    relative_subpath_posix(&rel)
}

fn matching_recovery_branches(pattern: &str, target_segments: &[&str]) -> Vec<String> {
    expand_braces(pattern)
        .into_iter()
        .filter_map(|branch| {
            let normalized = lexical_normalize_posix(&branch);
            let trimmed = normalized.trim_end_matches('/');
            branch_reaches_target(trimmed, target_segments).then(|| trimmed.to_string())
        })
        .collect()
}

fn branch_reaches_target(branch: &str, target: &[&str]) -> bool {
    if branch.is_empty() || branch == "." || branch.starts_with('/') {
        return false;
    }
    let segments: Vec<&str> = branch.split('/').collect();
    if segments.is_empty() || segments.contains(&"..") || segments[0].contains("**") {
        return false;
    }
    let mut memo = vec![vec![None; target.len() + 1]; segments.len() + 1];
    matches_prefix(&segments, target, (0, 0), &mut memo)
}

fn matches_prefix(
    pattern: &[&str],
    target: &[&str],
    (pi, mi): (usize, usize),
    memo: &mut [Vec<Option<bool>>],
) -> bool {
    if let Some(cached) = memo[pi][mi] {
        return cached;
    }
    let matched = if mi == target.len() {
        true
    } else if pi == pattern.len() {
        false
    } else if pattern[pi] == "**" {
        matches_prefix(pattern, target, (pi + 1, mi), memo)
            || matches_prefix(pattern, target, (pi, mi + 1), memo)
    } else {
        segment_glob_matches(pattern[pi], target[mi])
            && matches_prefix(pattern, target, (pi + 1, mi + 1), memo)
    };
    memo[pi][mi] = Some(matched);
    matched
}

fn segment_glob_matches(pattern_seg: &str, target_seg: &str) -> bool {
    if target_seg.starts_with('.') && !pattern_seg.starts_with('.') {
        return false;
    }
    if is_literal_glob_segment(pattern_seg) {
        return paths_match_on_host(pattern_seg, target_seg);
    }
    let (pat, seg) = if cfg!(any(windows, target_os = "macos")) {
        (pattern_seg.to_ascii_lowercase(), target_seg.to_ascii_lowercase())
    } else {
        (pattern_seg.to_string(), target_seg.to_string())
    };
    wax::Glob::new(&pat).is_ok_and(|glob| glob.is_match(Path::new(&seg)))
}

fn is_literal_glob_segment(seg: &str) -> bool {
    !seg.is_empty()
        && seg != "."
        && seg != ".."
        && !seg
            .chars()
            .any(|ch| matches!(ch, '*' | '?' | '[' | ']' | '{' | '}' | '\\'))
}

fn expand_braces(pattern: &str) -> Vec<String> {
    let mut queue = vec![pattern.to_string()];
    let mut expanded = Vec::new();
    while let Some(current) = queue.pop() {
        let Some((open, close, alts)) = find_first_brace_group(&current) else {
            expanded.push(current);
            continue;
        };
        if queue.len() + expanded.len() + alts.len() > MAX_BRACE_EXPANSIONS {
            expanded.push(current);
            continue;
        }
        let prefix = &current[..open];
        let suffix = &current[close + 1..];
        for alt in alts.into_iter().rev() {
            queue.push(format!("{prefix}{alt}{suffix}"));
        }
    }
    expanded
}

#[derive(Default)]
struct BraceScan {
    escaped: bool,
    in_brackets: bool,
    open_idx: Option<usize>,
    depth: usize,
    commas: Vec<usize>,
}

fn find_first_brace_group(input: &str) -> Option<(usize, usize, Vec<&str>)> {
    let mut scan = BraceScan::default();
    for (idx, ch) in input.char_indices() {
        if let Some(found) = scan.step(input, idx, ch) {
            return Some(found);
        }
    }
    None
}

impl BraceScan {
    fn step<'a>(
        &mut self,
        input: &'a str,
        idx: usize,
        ch: char,
    ) -> Option<(usize, usize, Vec<&'a str>)> {
        if self.escaped {
            self.escaped = false;
            return None;
        }
        if ch == '\\' {
            self.escaped = true;
            return None;
        }
        if self.consume_bracket_char(ch) {
            return None;
        }
        self.step_brace_delimiter(input, idx, ch)
    }

    fn consume_bracket_char(&mut self, ch: char) -> bool {
        if self.in_brackets {
            if ch == ']' {
                self.in_brackets = false;
            }
            return true;
        }
        if ch == '[' {
            self.in_brackets = true;
            return true;
        }
        false
    }

    fn step_brace_delimiter<'a>(
        &mut self,
        input: &'a str,
        idx: usize,
        ch: char,
    ) -> Option<(usize, usize, Vec<&'a str>)> {
        match ch {
            '{' => self.open_brace(idx),
            ',' if self.depth == 1 => self.commas.push(idx),
            '}' if self.depth > 0 => return self.close_brace(input, idx),
            _ => {}
        }
        None
    }

    fn open_brace(&mut self, idx: usize) {
        if self.depth == 0 {
            self.open_idx = Some(idx);
            self.commas.clear();
        }
        self.depth += 1;
    }

    fn close_brace<'a>(
        &mut self,
        input: &'a str,
        close: usize,
    ) -> Option<(usize, usize, Vec<&'a str>)> {
        self.depth -= 1;
        if self.depth > 0 {
            return None;
        }
        let open = self.open_idx.take()?;
        (!self.commas.is_empty()).then(|| {
            (open, close, slice_brace_alts(input, open, close, &self.commas))
        })
    }
}

fn slice_brace_alts<'a>(
    input: &'a str,
    open: usize,
    close: usize,
    commas: &[usize],
) -> Vec<&'a str> {
    let mut alts = Vec::with_capacity(commas.len() + 1);
    let mut start = open + 1;
    for &comma in commas {
        alts.push(&input[start..comma]);
        start = comma + 1;
    }
    alts.push(&input[start..close]);
    alts
}
