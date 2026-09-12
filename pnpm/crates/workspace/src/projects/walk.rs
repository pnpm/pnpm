use super::{
    BTreeSet, DirEntry, Entry, ErrorKind, FindWorkspaceProjectsError, PROJECT_MANIFEST_BASENAMES,
    Path, PathBuf, fs,
};
use wax::Program as _;

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
    !pattern.chars().any(|ch| ch == '\\' || wax::is_meta_character(ch))
        && !pattern.starts_with('/')
        && pattern
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

pub(super) fn normalize_manifest_patterns(pattern: &str) -> Vec<String> {
    PROJECT_MANIFEST_BASENAMES.iter().map(|basename| format!("{pattern}/{basename}")).collect()
}

pub(super) fn collect_manifests_in_children(
    parent: &Path,
    workspace_root: &Path,
    user_negations: &wax::Any<'_>,
    manifest_paths: &mut BTreeSet<PathBuf>,
) -> Result<(), FindWorkspaceProjectsError> {
    for_each_directory_entry(parent, workspace_root, |entry| {
        if starts_with_dot(&entry.file_name()) {
            return Ok(());
        }
        if !ignore_not_found(entry.file_type())
            .map_err(|source| workspace_walk_error(workspace_root, source))?
            .is_some_and(|file_type| file_type.is_dir())
        {
            return Ok(());
        }
        collect_candidate_manifests_in(
            &entry.path(),
            workspace_root,
            user_negations,
            manifest_paths,
        );
        Ok(())
    })
}

/// Record the child directory's manifest candidates without checking
/// which exist: the read phase absorbs a candidate that is not there,
/// so learning the answer here would pay a directory enumeration for
/// what one failed open later reports for free.
fn collect_candidate_manifests_in(
    directory: &Path,
    workspace_root: &Path,
    user_negations: &wax::Any<'_>,
    manifest_paths: &mut BTreeSet<PathBuf>,
) {
    for basename in PROJECT_MANIFEST_BASENAMES {
        let manifest_path = directory.join(basename);
        if !is_ignored_manifest(&manifest_path, workspace_root, user_negations) {
            manifest_paths.insert(manifest_path);
        }
    }
}

pub(super) fn collect_literal_manifests_in(
    directory: &Path,
    workspace_root: &Path,
    user_negations: &wax::Any<'_>,
    manifest_paths: &mut BTreeSet<PathBuf>,
) {
    for basename in PROJECT_MANIFEST_BASENAMES {
        let manifest_path = directory.join(basename);
        if manifest_path.is_file()
            && !is_ignored_manifest(&manifest_path, workspace_root, user_negations)
        {
            manifest_paths.insert(manifest_path);
        }
    }
}

fn for_each_directory_entry(
    directory: &Path,
    workspace_root: &Path,
    mut visit: impl FnMut(DirEntry) -> Result<(), FindWorkspaceProjectsError>,
) -> Result<(), FindWorkspaceProjectsError> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(source) => return Err(workspace_walk_error(workspace_root, source)),
    };
    for entry in entries {
        if let Some(entry) = ignore_not_found(entry)
            .map_err(|source| workspace_walk_error(workspace_root, source))?
        {
            visit(entry)?;
        }
    }
    Ok(())
}

fn ignore_not_found<Value>(result: std::io::Result<Value>) -> std::io::Result<Option<Value>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn workspace_walk_error(
    workspace_root: &Path,
    source: std::io::Error,
) -> FindWorkspaceProjectsError {
    FindWorkspaceProjectsError::Walk { root: workspace_root.to_path_buf(), source }
}

fn is_ignored_manifest(
    manifest_path: &Path,
    workspace_root: &Path,
    user_negations: &wax::Any<'_>,
) -> bool {
    let relative = manifest_path.strip_prefix(workspace_root).unwrap_or(manifest_path);
    has_always_ignored_component(relative) || user_negations.is_match(relative)
}

/// [`IGNORE_PATTERNS`](super::IGNORE_PATTERNS) by hand: `**/node_modules/**` and
/// `**/bower_components/**` hold exactly when some non-final component
/// bears one of those names, and a manifest candidate's final component
/// is always a manifest basename, so any-component equality answers the
/// match without running the glob engine once per candidate.
fn has_always_ignored_component(path: &Path) -> bool {
    use std::path::Component;
    path.components().any(|component| {
        matches!(
            component,
            Component::Normal(name) if name == "node_modules" || name == "bower_components",
        )
    })
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
    Some((walk_root, rest))
}

/// Ignore globs that forbid a dot-prefixed component at each position where
/// `pattern` has a wildcard, or `None` when no segment names a dot component
/// and the hoisted [`DOT_COMPONENT_IGNORE_PATTERN`](super::DOT_COMPONENT_IGNORE_PATTERN) already says the same thing.
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
    let globbed = segments
        .iter()
        .enumerate()
        .filter(|(_, segment)| !segment.starts_with('.') && !is_literal_pattern(segment));
    let ignores = globbed
        .map(|(index, segment)| {
            let dotted = if *segment == "**" { "**/.*/**" } else { ".*" };
            let mut replaced = segments.clone();
            replaced[index] = dotted;
            replaced.join("/")
        })
        .collect();
    Some(ignores)
}

/// Drain a prepared walk into `manifest_paths`, absorbing `NotFound` and
/// applying the user negations that `Walk::not` cannot express.
pub(super) fn collect_walk_manifests<Entries, Matched, Failure>(
    walk: Entries,
    walk_root: &Path,
    workspace_root: &Path,
    user_negations: &wax::Any<'_>,
    manifest_paths: &mut BTreeSet<PathBuf>,
) -> Result<(), FindWorkspaceProjectsError>
where
    Entries: Iterator<Item = Result<Matched, Failure>>,
    Matched: Entry,
    Failure: Into<std::io::Error>,
{
    for entry in walk {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                // Converting rather than restringifying keeps the underlying
                // `io::ErrorKind`, which the skip below needs.
                let err: std::io::Error = err.into();
                if err.kind() == ErrorKind::NotFound {
                    continue;
                }
                return Err(FindWorkspaceProjectsError::Walk {
                    root: walk_root.to_path_buf(),
                    source: err,
                });
            }
        };
        let manifest_path = entry.path();
        if pathdiff::diff_paths(manifest_path, workspace_root)
            .is_some_and(|relative| user_negations.is_match(relative.as_path()))
        {
            continue;
        }
        manifest_paths.insert(manifest_path.to_path_buf());
    }
    Ok(())
}

fn names_a_dot_component(segment: &str) -> bool {
    segment.starts_with('.') && segment != "." && segment != ".."
}

fn starts_with_dot(name: &std::ffi::OsStr) -> bool {
    name.as_encoded_bytes().first() == Some(&b'.')
}

pub(super) fn is_literal_pattern(pattern: &str) -> bool {
    !pattern.chars().any(|ch| matches!(ch, '*' | '?' | '[' | ']' | '{' | '}'))
}
