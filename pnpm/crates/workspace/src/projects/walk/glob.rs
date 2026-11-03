use super::{
    AncestorStack, BTreeSet, DirEntry, FindWorkspaceProjectsError, MergePatterns, Path, PathBuf,
    WorkspacePattern, collect_candidate_manifests_in, collect_literal_manifests_in,
    entry_is_directory, entry_is_file, for_each_directory_entry, is_always_ignored_name,
    is_ignored_directory, is_literal_segment, is_manifest_basename, is_pruned_subtree,
    manifest_walk_ignores, names_a_dot_component, split_parent_prefix, starts_with_dot,
};
use std::ffi::{OsStr, OsString};
use wax::{Glob, Program as _};

pub(super) struct WalkContext<'a> {
    pub(super) walk_root: &'a Path,
    pub(super) workspace_root: &'a Path,
    pub(super) user_negations: &'a wax::Any<'a>,
    pub(super) subtree_negations: &'a wax::Any<'a>,
    pub(super) ignored_directories: &'a [PathBuf],
}

impl WalkContext<'_> {
    fn collect_literal(&self, dir: &Path, manifest_paths: &mut BTreeSet<PathBuf>) {
        collect_literal_manifests_in(
            dir,
            self.workspace_root,
            self.user_negations,
            self.ignored_directories,
            manifest_paths,
        );
    }

    fn collect_candidates(&self, dir: &Path, manifest_paths: &mut BTreeSet<PathBuf>) {
        collect_candidate_manifests_in(
            dir,
            self.workspace_root,
            self.user_negations,
            self.ignored_directories,
            manifest_paths,
        );
    }
}

struct BoundedWalk<'a> {
    ctx: &'a WalkContext<'a>,
    glob: &'a Glob<'a>,
    ignores: &'a wax::Any<'a>,
    segments: &'a [&'a str],
}

impl BoundedWalk<'_> {
    fn matches(&self, rel_dir: &Path) -> bool {
        self.glob.is_match(rel_dir) && !self.ignores.is_match(rel_dir)
    }
}

struct GlobWalk<'a> {
    ctx: &'a WalkContext<'a>,
    glob: &'a Glob<'a>,
    ignores: &'a wax::Any<'a>,
    allow_dot: bool,
}

pub(in super::super) fn collect_glob_manifests(
    pattern: &WorkspacePattern<'_>,
    merge: &MergePatterns<'_>,
    manifest_paths: &mut BTreeSet<PathBuf>,
) -> Result<(), FindWorkspaceProjectsError> {
    let Some((walk_root, normalized)) =
        split_parent_prefix(merge.workspace_root, &pattern.normalized)
    else {
        return Ok(());
    };
    let ctx = WalkContext {
        walk_root,
        workspace_root: merge.workspace_root,
        user_negations: merge.user_negations,
        subtree_negations: merge.subtree_negations,
        ignored_directories: merge.ignored_directories,
    };
    if normalized == "." {
        ctx.collect_literal(walk_root, manifest_paths);
        return Ok(());
    }
    let mut ancestors = AncestorStack::new(walk_root);
    let (prefix, remaining) = split_literal_prefix(normalized);
    let Some((start_dir, start_rel)) = step_literal_prefix(&prefix, &ctx, &mut ancestors) else {
        return Ok(());
    };
    if remaining.is_empty() {
        ctx.collect_literal(&start_dir, manifest_paths);
        return Ok(());
    }
    if remaining == ["**"] {
        return walk_recursive_tail(&start_dir, &ctx, &mut ancestors, manifest_paths);
    }
    dispatch_glob_walk(
        pattern,
        normalized,
        &remaining,
        merge.dot_pruning_ignore_template,
        &ctx,
        (&start_dir, &start_rel),
        &mut ancestors,
        manifest_paths,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "dispatches a prepared pattern walk across bounded or recursive traversal"
)]
fn dispatch_glob_walk(
    pattern: &WorkspacePattern<'_>,
    normalized: &str,
    remaining: &[&str],
    dot_pruning_ignore_template: &wax::Any<'_>,
    ctx: &WalkContext<'_>,
    (start_dir, start_rel): (&Path, &Path),
    ancestors: &mut AncestorStack,
    manifest_paths: &mut BTreeSet<PathBuf>,
) -> Result<(), FindWorkspaceProjectsError> {
    let invalid_glob = |err: wax::BuildError| FindWorkspaceProjectsError::InvalidGlob {
        pattern: pattern.source.to_string(),
        message: err.to_string(),
    };
    let glob = Glob::new(normalized).map_err(invalid_glob)?;
    let ignores =
        manifest_walk_ignores(normalized, dot_pruning_ignore_template).map_err(invalid_glob)?;
    if !normalized.starts_with('/') && !has_slash_in_group(normalized) && !remaining.contains(&"**")
    {
        let bounded = BoundedWalk { ctx, glob: &glob, ignores: &ignores, segments: remaining };
        return walk_bounded_segments(&bounded, start_dir, start_rel, 0, ancestors, manifest_paths);
    }
    let allow_dot = remaining.iter().any(|segment| names_a_dot_component(segment));
    let glob_walk = GlobWalk { ctx, glob: &glob, ignores: &ignores, allow_dot };
    walk_recursive_glob(&glob_walk, start_dir, start_rel, ancestors, manifest_paths)
}

fn split_literal_prefix(pattern: &str) -> (Vec<&str>, Vec<&str>) {
    if pattern.starts_with('/') || has_slash_in_group(pattern) {
        return (Vec::new(), vec![pattern]);
    }
    let all_segments: Vec<&str> = pattern.split('/').collect();
    let prefix_len = all_segments
        .iter()
        .take_while(|segment| is_literal_segment(segment))
        .count();
    let (prefix, remaining) = all_segments.split_at(prefix_len);
    (prefix.to_vec(), remaining.to_vec())
}

fn has_slash_in_group(pattern: &str) -> bool {
    let mut depth = 0usize;
    let mut escaped = false;
    for ch in pattern.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '{' | '(' => depth += 1,
            '}' | ')' => depth = depth.saturating_sub(1),
            '/' if depth > 0 => return true,
            _ => {}
        }
    }
    false
}

fn step_literal_prefix(
    prefix: &[&str],
    ctx: &WalkContext<'_>,
    ancestors: &mut AncestorStack,
) -> Option<(PathBuf, PathBuf)> {
    let mut current_dir = ctx.walk_root.to_path_buf();
    let mut current_rel = PathBuf::new();
    for segment in prefix {
        if is_always_ignored_name(OsStr::new(segment)) {
            return None;
        }
        current_dir.push(segment);
        current_rel.push(segment);
        if is_pruned_subtree(&current_dir, ctx) {
            return None;
        }
        ancestors.push_literal(current_dir.clone());
    }
    Some((current_dir, current_rel))
}

fn walk_recursive_tail(
    dir: &Path,
    ctx: &WalkContext<'_>,
    ancestors: &mut AncestorStack,
    manifest_paths: &mut BTreeSet<PathBuf>,
) -> Result<(), FindWorkspaceProjectsError> {
    if is_pruned_subtree(dir, ctx) {
        return Ok(());
    }
    let mut subdirs = Vec::new();
    for_each_directory_entry(dir, ctx.walk_root, |entry| {
        inspect_recursive_entry(&entry, dir, ctx, manifest_paths, &mut subdirs)
    })?;
    for (child_path, is_symlink) in subdirs {
        if !ancestors.push_directory(&child_path, is_symlink) {
            continue;
        }
        let result = walk_recursive_tail(&child_path, ctx, ancestors, manifest_paths);
        ancestors.pop();
        result?;
    }
    Ok(())
}

fn inspect_recursive_entry(
    entry: &DirEntry,
    dir: &Path,
    ctx: &WalkContext<'_>,
    manifest_paths: &mut BTreeSet<PathBuf>,
    subdirs: &mut Vec<(PathBuf, bool)>,
) -> Result<(), FindWorkspaceProjectsError> {
    let name = entry.file_name();
    if is_manifest_basename(&name) && entry_is_file(entry, ctx.walk_root)? {
        if !is_ignored_directory(dir, ctx) {
            manifest_paths.insert(entry.path());
        }
        return Ok(());
    }
    if starts_with_dot(&name) || is_always_ignored_name(&name) {
        return Ok(());
    }
    if let Some(is_symlink) = entry_is_directory(entry, ctx.walk_root)? {
        subdirs.push((entry.path(), is_symlink));
    }
    Ok(())
}

fn walk_bounded_segments(
    walk: &BoundedWalk<'_>,
    dir: &Path,
    rel_dir: &Path,
    depth: usize,
    ancestors: &mut AncestorStack,
    manifest_paths: &mut BTreeSet<PathBuf>,
) -> Result<(), FindWorkspaceProjectsError> {
    if depth == walk.segments.len() {
        if walk.matches(rel_dir) {
            walk.ctx.collect_literal(dir, manifest_paths);
        }
        return Ok(());
    }
    let segment = walk.segments[depth];
    if is_literal_segment(segment) {
        let Some((child_dir, child_rel)) = step_bounded_literal(walk, dir, rel_dir, segment, depth)
        else {
            return Ok(());
        };
        return walk_bounded_segments(
            walk,
            &child_dir,
            &child_rel,
            depth + 1,
            ancestors,
            manifest_paths,
        );
    }
    walk_bounded_wildcard(walk, dir, rel_dir, depth, ancestors, manifest_paths)
}

fn step_bounded_literal(
    walk: &BoundedWalk<'_>,
    dir: &Path,
    rel_dir: &Path,
    segment: &str,
    depth: usize,
) -> Option<(PathBuf, PathBuf)> {
    if is_always_ignored_name(OsStr::new(segment)) {
        return None;
    }
    let child_dir = dir.join(segment);
    if is_pruned_subtree(&child_dir, walk.ctx) {
        return None;
    }
    if depth + 1 < walk.segments.len() && !child_dir.is_dir() {
        return None;
    }
    Some((child_dir, rel_dir.join(segment)))
}

fn walk_bounded_wildcard(
    walk: &BoundedWalk<'_>,
    dir: &Path,
    rel_dir: &Path,
    depth: usize,
    ancestors: &mut AncestorStack,
    manifest_paths: &mut BTreeSet<PathBuf>,
) -> Result<(), FindWorkspaceProjectsError> {
    if is_pruned_subtree(dir, walk.ctx) {
        return Ok(());
    }
    let allow_dot = walk.segments[depth].starts_with('.');
    for subdir in collect_wildcard_subdirs(dir, walk.ctx.walk_root, allow_dot)? {
        visit_bounded_subdir(walk, rel_dir, depth, subdir, ancestors, manifest_paths)?;
    }
    Ok(())
}

fn visit_bounded_subdir(
    walk: &BoundedWalk<'_>,
    rel_dir: &Path,
    depth: usize,
    (name, child_dir, is_symlink): (OsString, PathBuf, bool),
    ancestors: &mut AncestorStack,
    manifest_paths: &mut BTreeSet<PathBuf>,
) -> Result<(), FindWorkspaceProjectsError> {
    if is_pruned_subtree(&child_dir, walk.ctx) {
        return Ok(());
    }
    let child_rel = rel_dir.join(&name);
    if depth + 1 == walk.segments.len() {
        if walk.matches(&child_rel) {
            walk.ctx.collect_candidates(&child_dir, manifest_paths);
        }
        return Ok(());
    }
    if !ancestors.push_directory(&child_dir, is_symlink) {
        return Ok(());
    }
    let result =
        walk_bounded_segments(walk, &child_dir, &child_rel, depth + 1, ancestors, manifest_paths);
    ancestors.pop();
    result
}

fn collect_wildcard_subdirs(
    dir: &Path,
    walk_root: &Path,
    allow_dot: bool,
) -> Result<Vec<(OsString, PathBuf, bool)>, FindWorkspaceProjectsError> {
    let mut subdirs = Vec::new();
    for_each_directory_entry(dir, walk_root, |entry| {
        let name = entry.file_name();
        if is_always_ignored_name(&name) || (!allow_dot && starts_with_dot(&name)) {
            return Ok(());
        }
        if let Some(is_symlink) = entry_is_directory(&entry, walk_root)? {
            subdirs.push((name, entry.path(), is_symlink));
        }
        Ok(())
    })?;
    Ok(subdirs)
}

fn walk_recursive_glob(
    walk: &GlobWalk<'_>,
    dir: &Path,
    rel_dir: &Path,
    ancestors: &mut AncestorStack,
    manifest_paths: &mut BTreeSet<PathBuf>,
) -> Result<(), FindWorkspaceProjectsError> {
    if is_pruned_subtree(dir, walk.ctx) {
        return Ok(());
    }
    let dir_matches = !rel_dir.as_os_str().is_empty()
        && walk.glob.is_match(rel_dir)
        && !walk.ignores.is_match(rel_dir);
    let mut subdirs = Vec::new();
    for_each_directory_entry(dir, walk.ctx.walk_root, |entry| {
        inspect_glob_entry(&entry, dir, dir_matches, walk, manifest_paths, &mut subdirs)
    })?;
    for (name, child_dir, is_symlink) in subdirs {
        let child_rel = rel_dir.join(&name);
        if walk.ignores.is_match(child_rel.as_path())
            || !ancestors.push_directory(&child_dir, is_symlink)
        {
            continue;
        }
        let result = walk_recursive_glob(walk, &child_dir, &child_rel, ancestors, manifest_paths);
        ancestors.pop();
        result?;
    }
    Ok(())
}

fn inspect_glob_entry(
    entry: &DirEntry,
    dir: &Path,
    dir_matches: bool,
    walk: &GlobWalk<'_>,
    manifest_paths: &mut BTreeSet<PathBuf>,
    subdirs: &mut Vec<(OsString, PathBuf, bool)>,
) -> Result<(), FindWorkspaceProjectsError> {
    let name = entry.file_name();
    if is_manifest_basename(&name) && entry_is_file(entry, walk.ctx.walk_root)? {
        if dir_matches && !is_ignored_directory(dir, walk.ctx) {
            manifest_paths.insert(entry.path());
        }
        return Ok(());
    }
    if is_always_ignored_name(&name) || (!walk.allow_dot && starts_with_dot(&name)) {
        return Ok(());
    }
    if let Some(is_symlink) = entry_is_directory(entry, walk.ctx.walk_root)? {
        subdirs.push((name, entry.path(), is_symlink));
    }
    Ok(())
}
