pub(super) use glob::collect_glob_manifests;

use super::{
    BTreeSet, DirEntry, ErrorKind, FindWorkspaceProjectsError, Glob, MergePatterns,
    PROJECT_MANIFEST_BASENAMES, Path, PathBuf, WorkspacePattern, fs,
    managed::is_under_ignored_directory,
    negations::{is_negated_directory, is_pruned_by_subtree_negation},
};
use ancestors::AncestorStack;
use glob::WalkContext;
use std::{ffi::OsStr, path::Component};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum SpecializedPattern<'pattern> {
    Literal(&'pattern str),
    ChildrenOf(&'pattern str),
}

pub(super) fn specialized_pattern(pattern: &str) -> Option<SpecializedPattern<'_>> {
    let pattern = pattern.trim_end_matches('/');
    if let Some(parent) = pattern.strip_suffix("/*") {
        return is_safe_relative_literal(parent).then_some(SpecializedPattern::ChildrenOf(parent));
    }
    is_safe_relative_literal(pattern).then_some(SpecializedPattern::Literal(pattern))
}

fn is_safe_relative_literal(pattern: &str) -> bool {
    !pattern.is_empty() && !pattern.starts_with('/') && pattern.split('/').all(is_literal_segment)
}

pub(super) fn is_literal_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && !segment
            .chars()
            .any(|ch| ch == '\\' || wax::is_meta_character(ch))
}

pub(super) fn collect_manifests_in_children(
    parent: &Path,
    workspace_root: &Path,
    user_negations: &wax::Any<'_>,
    ignored_directories: &[PathBuf],
    manifest_paths: &mut BTreeSet<PathBuf>,
) -> Result<(), FindWorkspaceProjectsError> {
    for_each_directory_entry(parent, workspace_root, |entry| {
        if starts_with_dot(&entry.file_name()) {
            return Ok(());
        }
        if entry_is_directory(&entry, workspace_root)?.is_none() {
            return Ok(());
        }
        collect_candidate_manifests_in(
            &entry.path(),
            workspace_root,
            user_negations,
            ignored_directories,
            manifest_paths,
        );
        Ok(())
    })
}

pub(super) fn entry_is_directory(
    entry: &DirEntry,
    walk_root: &Path,
) -> Result<Option<bool>, FindWorkspaceProjectsError> {
    let Some(file_type) = ignore_not_found_or_loop(entry.file_type())
        .map_err(|source| workspace_walk_error(walk_root, source))?
    else {
        return Ok(None);
    };
    if file_type.is_dir() {
        return Ok(Some(false));
    }
    if file_type.is_symlink() {
        let Some(metadata) = ignore_not_found_or_loop(fs::metadata(entry.path()))
            .map_err(|source| workspace_walk_error(walk_root, source))?
        else {
            return Ok(None);
        };
        return Ok(metadata.is_dir().then_some(true));
    }
    Ok(None)
}

pub(super) fn entry_is_file(
    entry: &DirEntry,
    walk_root: &Path,
) -> Result<bool, FindWorkspaceProjectsError> {
    let Some(file_type) = ignore_not_found_or_loop(entry.file_type())
        .map_err(|source| workspace_walk_error(walk_root, source))?
    else {
        return Ok(false);
    };
    if file_type.is_file() {
        return Ok(true);
    }
    if file_type.is_symlink() {
        let Some(metadata) = ignore_not_found_or_loop(fs::metadata(entry.path()))
            .map_err(|source| workspace_walk_error(walk_root, source))?
        else {
            return Ok(false);
        };
        return Ok(metadata.is_file());
    }
    Ok(false)
}

fn ignore_not_found_or_loop<Value>(
    result: std::io::Result<Value>,
) -> std::io::Result<Option<Value>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == ErrorKind::NotFound || is_symlink_loop(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Record the child directory's manifest candidates without checking
/// which exist: the read phase absorbs a candidate that is not there,
/// so learning the answer here would pay a directory enumeration for
/// what one failed open later reports for free.
pub(super) fn collect_candidate_manifests_in(
    directory: &Path,
    workspace_root: &Path,
    user_negations: &wax::Any<'_>,
    ignored_directories: &[PathBuf],
    manifest_paths: &mut BTreeSet<PathBuf>,
) {
    if is_ignored_manifest_directory(directory, workspace_root, user_negations, ignored_directories)
    {
        return;
    }
    for basename in PROJECT_MANIFEST_BASENAMES {
        manifest_paths.insert(directory.join(basename));
    }
}

pub(super) fn collect_literal_manifests_in(
    directory: &Path,
    workspace_root: &Path,
    user_negations: &wax::Any<'_>,
    ignored_directories: &[PathBuf],
    manifest_paths: &mut BTreeSet<PathBuf>,
) {
    if is_ignored_manifest_directory(directory, workspace_root, user_negations, ignored_directories)
    {
        return;
    }
    for basename in PROJECT_MANIFEST_BASENAMES {
        let manifest_path = directory.join(basename);
        if manifest_path.is_file() {
            manifest_paths.insert(manifest_path);
        }
    }
}

pub(super) fn for_each_directory_entry(
    directory: &Path,
    walk_root: &Path,
    mut visit: impl FnMut(DirEntry) -> Result<(), FindWorkspaceProjectsError>,
) -> Result<(), FindWorkspaceProjectsError> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(source) => return Err(workspace_walk_error(walk_root, source)),
    };
    for entry in entries {
        if let Some(entry) = ignore_not_found_or_loop(entry)
            .map_err(|source| workspace_walk_error(walk_root, source))?
        {
            visit(entry)?;
        }
    }
    Ok(())
}

fn workspace_walk_error(walk_root: &Path, source: std::io::Error) -> FindWorkspaceProjectsError {
    FindWorkspaceProjectsError::Walk { root: walk_root.to_path_buf(), source }
}

fn is_ignored_manifest_directory(
    directory: &Path,
    workspace_root: &Path,
    user_negations: &wax::Any<'_>,
    ignored_directories: &[PathBuf],
) -> bool {
    let relative = directory.strip_prefix(workspace_root).unwrap_or(directory);
    is_under_ignored_directory(directory, ignored_directories)
        || has_always_ignored_component(relative)
        || is_negated_directory(directory, workspace_root, user_negations)
}

fn is_ignored_directory(dir: &Path, ctx: &WalkContext<'_>) -> bool {
    is_under_ignored_directory(dir, ctx.ignored_directories)
        || is_negated_directory(dir, ctx.workspace_root, ctx.user_negations)
}

fn is_pruned_subtree(dir: &Path, ctx: &WalkContext<'_>) -> bool {
    is_under_ignored_directory(dir, ctx.ignored_directories)
        || is_pruned_by_subtree_negation(dir, ctx.workspace_root, ctx.subtree_negations)
}

pub(super) fn has_always_ignored_component(path: &Path) -> bool {
    path.components()
        .any(|component| {
            matches!(
                component,
                Component::Normal(name) if is_always_ignored_name(name),
            )
        })
}

fn is_always_ignored_name(name: &OsStr) -> bool {
    name == "node_modules" || name == "bower_components"
}

fn is_manifest_basename(name: &OsStr) -> bool {
    PROJECT_MANIFEST_BASENAMES
        .iter()
        .any(|basename| name == *basename)
}

/// Strip the pattern's leading `../` components, walking `workspace_root`
/// up one directory for each. wax globs cannot express parent traversal,
/// so a pattern such as `../shared/*` only matches when the walk starts
/// from the ancestor it names. `None` — the traversal climbs past the
/// filesystem root — matches nothing.
pub(super) fn split_parent_prefix<'root, 'pattern>(
    workspace_root: &'root Path,
    pattern: &'pattern str,
) -> Option<(&'root Path, &'pattern str)> {
    let mut walk_root = workspace_root;
    let mut rest = pattern;
    while let Some(tail) = rest.strip_prefix("../") {
        walk_root = walk_root.parent()?;
        rest = tail;
    }
    if rest == ".." {
        walk_root = walk_root.parent()?;
        rest = ".";
    }
    Some((walk_root, rest))
}

/// The walk ignores of a pattern that names no dot component: built once and
/// cloned for every such walk.
pub(super) fn dot_pruning_ignore_template() -> Result<wax::Any<'static>, FindWorkspaceProjectsError>
{
    compile_walk_ignores(None)
        .map_err(|err| FindWorkspaceProjectsError::InvalidGlob {
            pattern: "<built-in ignore>".to_string(),
            message: err.to_string(),
        })
}

pub(super) fn manifest_walk_ignores<'a>(
    normalized: &str,
    dot_pruning_ignore_template: &wax::Any<'a>,
) -> Result<wax::Any<'a>, wax::BuildError> {
    let Some(dot_ignores) = positional_dot_ignores(normalized) else {
        return Ok(dot_pruning_ignore_template.clone());
    };
    compile_walk_ignores(Some(&dot_ignores))
}

fn compile_walk_ignores(
    dot_ignores: Option<&[String]>,
) -> Result<wax::Any<'static>, wax::BuildError> {
    let dot_ignores: Vec<&str> = match dot_ignores {
        None => vec![DOT_COMPONENT_IGNORE_PATTERN],
        Some(dot_ignores) => dot_ignores
            .iter()
            .map(String::as_str)
            .collect(),
    };
    let patterns = IGNORE_PATTERNS
        .iter()
        .copied()
        .chain(dot_ignores)
        .map(|pattern| Glob::new(pattern).map(Glob::into_owned))
        .collect::<Result<Vec<_>, _>>()?;
    wax::any(patterns)
}

/// Ignore globs that forbid a dot-prefixed component at each position where
/// `pattern` has a wildcard, or `None` when no segment names a dot component
/// and the hoisted [`DOT_COMPONENT_IGNORE_PATTERN`] already says the same thing.
///
/// A wildcard must never match a dot-prefixed component, but a pattern that
/// spells one out must still reach it, and only there. Deriving one ignore per
/// wildcard position keeps that distinction: given `packages/.cache/*/lib`,
/// `.cache` stays reachable while `packages/.cache/.hidden/lib` and
/// `packages/.cache/.cache/lib` are both pruned. A wildcard that itself starts
/// with a dot, as in `packages/.*`, is asking for dot components and gets no
/// ignore.
pub(super) fn positional_dot_ignores(pattern: &str) -> Option<Vec<String>> {
    let segments: Vec<&str> = pattern.split('/').collect();
    if !segments.iter().any(|segment| names_a_dot_component(segment)) {
        return None;
    }
    let ignores = segments
        .iter()
        .enumerate()
        .filter(|(_, segment)| !segment.starts_with('.') && !is_literal_pattern(segment))
        .map(|(index, segment)| {
            let dotted = if *segment == "**" { "**/.*/**" } else { ".*" };
            let mut replaced = segments.clone();
            replaced[index] = dotted;
            replaced.join("/")
        })
        .collect();
    Some(ignores)
}

pub(super) fn names_a_dot_component(segment: &str) -> bool {
    segment.starts_with('.') && segment != "." && segment != ".."
}

pub(super) fn starts_with_dot(name: &OsStr) -> bool {
    name.as_encoded_bytes().first() == Some(&b'.')
}

fn is_literal_pattern(pattern: &str) -> bool {
    !pattern
        .chars()
        .any(|ch| matches!(ch, '*' | '?' | '[' | ']' | '{' | '}'))
}

fn is_symlink_loop(error: &std::io::Error) -> bool {
    is_raw_loop_error(error) || is_filesystem_loop_kind(error.kind())
}

fn is_filesystem_loop_kind(kind: ErrorKind) -> bool {
    // `ErrorKind::FilesystemLoop` is unstable in the standard library (rust-lang/rust#86442).
    // Matching its debug representation recognizes the standard library enum variant structurally.
    format!("{kind:?}") == "FilesystemLoop"
}

#[cfg(unix)]
fn is_raw_loop_error(error: &std::io::Error) -> bool {
    error.raw_os_error() == Some(libc::ELOOP)
}

#[cfg(not(unix))]
fn is_raw_loop_error(_error: &std::io::Error) -> bool {
    false
}

/// Hardcoded ignore patterns. Enumerating a real workspace excludes
/// only `node_modules` and `bower_components`, not the `**/test/**` /
/// `**/tests/**` directories that the lower-level package-finding path
/// excludes.
const IGNORE_PATTERNS: &[&str] = &["**/node_modules/**", "**/bower_components/**"];

/// Prunes every path with a dot-prefixed component, so a wildcard cannot
/// descend into `.git`, `.cache`, and friends. Applied only to patterns that
/// do not name a dot component themselves — see [`positional_dot_ignores`].
const DOT_COMPONENT_IGNORE_PATTERN: &str = "**/.*/**";

mod ancestors;
mod glob;
