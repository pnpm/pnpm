//! Glob-expand `packages:` from `pnpm-workspace.yaml` into the
//! workspace's [`Project`] list.
//!
//! Out of scope (tracked as parity follow-ups):
//!
//! - `engines` / `os` / `cpu` installability filtering. Issue [#431]
//!   explicitly defers this.
//! - The `resolutions`-on-non-root warning. Single-line emission that
//!   can land when the reporter side is in place.
//! - Real-path resolution of `rootDir` for case-insensitive
//!   filesystems. Same divergence as [`root_finder`].
//!
//! [`root_finder`]: super::root_finder
//!
//! [#431]: https://github.com/pnpm/pacquet/issues/431

use crate::project_manifest::{ReadProjectManifestError, read_exact_project_manifest};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_package_manifest::{PackageManifest, PackageManifestError};
use rayon::prelude::*;
use std::{
    collections::BTreeSet,
    fs::{self, DirEntry},
    io::ErrorKind,
    path::{Path, PathBuf},
};
use wax::{
    Glob, Program,
    walk::{Entry, FileIterator},
};

/// A project discovered under the workspace root.
///
/// Pacquet keeps this shape narrower than pnpm's project type (which
/// also carries `rootDirRealPath`, `modulesDir`, etc.). The fields here
/// are what `pnpm-package-manager` actually needs at install time;
/// anything else is read on demand from the manifest. If a caller
/// needs more, extend here rather than reaching back into the
/// `package.json` value directly.
pub struct Project {
    pub root_dir: PathBuf,
    pub manifest: PackageManifest,
    /// Manifest to expose when this project is resolved as a *dependency* of
    /// another importer (an injected workspace instance), instead of
    /// `manifest`. `None` — the common case, including every project the
    /// on-disk walk discovers — means the two views are the same. Embedders
    /// (`@pnpm/napi`'s `dependencyManifest`) split them when their importer
    /// manifests are pre-transformed (e.g. workspace-sibling deps stripped)
    /// but dependency instances must keep the raw dependency graph.
    pub dependency_manifest: Option<PackageManifest>,
}

/// Options for [`find_workspace_projects`].
#[derive(Debug, Default, Clone)]
pub struct FindWorkspaceProjectsOpts {
    /// Package discovery patterns. When `None`, the lower-level
    /// enumeration falls back to `['.', '**']`. Callers enumerating a
    /// real workspace manifest should pass
    /// [`crate::workspace_package_patterns`] instead.
    pub patterns: Option<Vec<String>>,
}

/// Error type of the public entry points.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum FindWorkspaceProjectsError {
    #[display("Invalid glob pattern in pnpm-workspace.yaml packages: {pattern:?}: {message}")]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_INVALID_GLOB))]
    InvalidGlob {
        pattern: String,
        // Built once at construction. wax errors carry a borrow of the
        // input glob, so flatten to a string here for ergonomic storage.
        message: String,
    },

    #[display("Failed to walk workspace projects under {}: {source}", root.display())]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_WALK_ERROR))]
    Walk {
        root: PathBuf,
        #[error(source)]
        source: std::io::Error,
    },

    #[diagnostic(transparent)]
    ReadManifest(#[error(source)] ReadProjectManifestError),
}

/// Find every project under `workspace_root` matching `opts.patterns`.
///
/// The per-project installability and non-root-manifest validations are
/// explicitly deferred by [#431]. When validation lands, this entry
/// point grows the filter; today it's a thin wrapper over
/// [`find_workspace_projects_no_check`].
///
/// [#431]: https://github.com/pnpm/pacquet/issues/431
pub fn find_workspace_projects(
    workspace_root: &Path,
    opts: &FindWorkspaceProjectsOpts,
) -> Result<Vec<Project>, FindWorkspaceProjectsError> {
    find_workspace_projects_no_check(workspace_root, opts)
}

/// Skip-validation variant.
pub fn find_workspace_projects_no_check(
    workspace_root: &Path,
    opts: &FindWorkspaceProjectsOpts,
) -> Result<Vec<Project>, FindWorkspaceProjectsError> {
    // When no patterns were configured, search the workspace root
    // non-recursively *and* recursively. The two-pattern fallback fires
    // only on `None`, not on `Some(vec![])` — an explicit empty array
    // means "enumerate only the workspace root" (which is
    // unconditionally added below per
    // <https://github.com/pnpm/pnpm/issues/1986>).
    let default_patterns = [".".to_string(), "**".to_string()];
    let patterns: &[String] = match opts.patterns.as_deref() {
        Some(p) => p,
        None => &default_patterns,
    };

    // `!`-prefixed patterns are negations. wax does not accept `!`
    // inside `Glob::new()`, so split them out and feed them through
    // `.not()` instead. `!/...` remains a no-op: relative workspace
    // paths never match that absolute form.
    let mut include_patterns: Vec<&str> = Vec::new();
    let mut user_negation_globs: Vec<String> = Vec::new();
    for pattern in patterns {
        if let Some(body) = pattern.strip_prefix('!') {
            if body.starts_with('/') {
                continue;
            }
            for normalized in normalize_manifest_patterns(body) {
                Glob::new(&normalized).map_err(|err| FindWorkspaceProjectsError::InvalidGlob {
                    pattern: pattern.clone(),
                    message: err.to_string(),
                })?;
                user_negation_globs.push(normalized);
            }
        } else {
            include_patterns.push(pattern);
        }
    }

    // wax's `not` takes a single pattern; combine the ignores with
    // `wax::any` so the walk filters them all in one pass. Built once
    // outside the per-pattern loop to avoid reparsing the constant
    // ignores.
    let build_ignores = |patterns: &mut dyn Iterator<Item = &'static str>| {
        wax::any(patterns).map_err(|err| FindWorkspaceProjectsError::InvalidGlob {
            pattern: "<built-in ignore>".to_string(),
            message: err.to_string(),
        })
    };
    let dot_pruning_ignore_template =
        build_ignores(&mut IGNORE_PATTERNS.iter().copied().chain([DOT_COMPONENT_IGNORE_PATTERN]))?;

    // User negations are written relative to the workspace root, while a
    // parent-relative include walks from an ancestor of it, so they are
    // matched against the path each entry has *from the workspace root*
    // rather than handed to `Walk::not` alongside the built-in ignores.
    let user_negations = wax::any(user_negation_globs.iter().map(std::string::String::as_str))
        .map_err(|err| FindWorkspaceProjectsError::InvalidGlob {
            pattern: "<negated pattern>".to_string(),
            message: err.to_string(),
        })?;

    // Parse-check the generic-walk patterns up front, so a malformed
    // glob fails before any pattern pays for a workspace walk. The fast
    // paths accept only meta-character-free patterns, which cannot fail
    // to parse.
    for pattern in &include_patterns {
        if specialized_pattern(pattern).is_some() {
            continue;
        }
        for normalized in normalize_manifest_patterns(pattern) {
            let Some((_, normalized)) = split_parent_prefix(workspace_root, &normalized) else {
                continue;
            };
            Glob::new(normalized).map_err(|err| FindWorkspaceProjectsError::InvalidGlob {
                pattern: (*pattern).to_string(),
                message: err.to_string(),
            })?;
        }
    }

    // Each pattern's set folds into the shared merge as it completes,
    // so peak memory stays one merged set plus the in-flight patterns —
    // overlapping patterns don't multiply it. Set union commutes and
    // the first error *in pattern-list order* wins, keeping the result
    // and the reported failure a function of the pattern list alone.
    let merged_manifest_paths: std::sync::Mutex<BTreeSet<PathBuf>> = std::sync::Mutex::default();
    let pattern_errors: Vec<Option<FindWorkspaceProjectsError>> = include_patterns
        .par_iter()
        .map(|pattern| {
            match collect_pattern_manifests(
                pattern,
                workspace_root,
                &dot_pruning_ignore_template,
                &user_negations,
            ) {
                Ok(set) => {
                    merged_manifest_paths.lock().expect("merge lock never poisoned").extend(set);
                    None
                }
                Err(error) => Some(error),
            }
        })
        .collect();
    if let Some(error) = pattern_errors.into_iter().flatten().next() {
        return Err(error);
    }
    let mut manifest_paths = merged_manifest_paths.into_inner().expect("merge lock never poisoned");

    for basename in PROJECT_MANIFEST_BASENAMES {
        let root_manifest = workspace_root.join(basename);
        if root_manifest.is_file() {
            manifest_paths.insert(root_manifest);
        }
    }

    // Sort lexicographically by `rootDir` (= parent of the manifest).
    let mut sorted: Vec<PathBuf> = manifest_paths.into_iter().collect();
    sorted.sort_by(|left, right| {
        let dir_left = left.parent().unwrap_or_else(|| Path::new(""));
        let dir_right = right.parent().unwrap_or_else(|| Path::new(""));
        dir_left.cmp(dir_right)
    });

    // A root's candidates stay in manifest-precedence order —
    // `package.json` before `package.yaml`, because the sort above is
    // stable and ties keep the set's full-path order — and share one
    // read task, so "first readable manifest wins" holds under
    // concurrency: a candidate that vanishes mid-run hands its root to
    // the next candidate, never to a skipped root.
    let mut root_groups: Vec<(PathBuf, Vec<PathBuf>)> = Vec::new();
    for manifest_path in sorted {
        let root_dir = manifest_path.parent().unwrap_or(workspace_root).to_path_buf();
        match root_groups.last_mut() {
            Some((last_root, candidates)) if *last_root == root_dir => {
                candidates.push(manifest_path);
            }
            _ => root_groups.push((root_dir, vec![manifest_path])),
        }
    }

    let read_results: Vec<Result<Option<Project>, FindWorkspaceProjectsError>> = root_groups
        .into_par_iter()
        .map(|(root_dir, candidates)| read_first_project_manifest(root_dir, candidates))
        .collect();
    let mut projects = Vec::with_capacity(read_results.len());
    for result in read_results {
        if let Some(project) = result? {
            projects.push(project);
        }
    }

    Ok(projects)
}

/// Expand one include pattern into the manifest paths it matches. The
/// contract [`find_workspace_projects_no_check`] states — which error
/// kinds are absorbed, how the fast paths and the generic walk divide
/// the pattern space — lives there; this is its per-pattern body.
fn collect_pattern_manifests(
    pattern: &str,
    workspace_root: &Path,
    dot_pruning_ignore_template: &wax::Any<'_>,
    user_negations: &wax::Any<'_>,
) -> Result<BTreeSet<PathBuf>, FindWorkspaceProjectsError> {
    let mut manifest_paths: BTreeSet<PathBuf> = BTreeSet::new();
    match specialized_pattern(pattern) {
        Some(SpecializedPattern::ChildrenOf(parent)) => {
            collect_manifests_in_children(
                &workspace_root.join(parent),
                workspace_root,
                user_negations,
                &mut manifest_paths,
            )?;
            return Ok(manifest_paths);
        }
        Some(SpecializedPattern::Literal(directory)) => {
            collect_literal_manifests_in(
                &workspace_root.join(directory),
                workspace_root,
                user_negations,
                &mut manifest_paths,
            );
            return Ok(manifest_paths);
        }
        None => {}
    }

    for normalized in normalize_manifest_patterns(pattern) {
        let Some((walk_root, normalized)) = split_parent_prefix(workspace_root, &normalized) else {
            continue;
        };
        if is_literal_pattern(normalized) && !walk_root.join(normalized).is_file() {
            continue;
        }
        let glob =
            Glob::new(normalized).map_err(|err| FindWorkspaceProjectsError::InvalidGlob {
                pattern: pattern.to_string(),
                message: err.to_string(),
            })?;

        let invalid_glob = |err: wax::BuildError| FindWorkspaceProjectsError::InvalidGlob {
            pattern: pattern.to_string(),
            message: err.to_string(),
        };
        match positional_dot_ignores(normalized) {
            None => collect_walk_manifests(
                glob.walk(walk_root)
                    .not(dot_pruning_ignore_template.clone())
                    .map_err(invalid_glob)?,
                walk_root,
                workspace_root,
                user_negations,
                &mut manifest_paths,
            )?,
            Some(dot_ignores) => {
                let ignores = wax::any(
                    IGNORE_PATTERNS.iter().copied().chain(dot_ignores.iter().map(String::as_str)),
                )
                .map_err(invalid_glob)?;
                collect_walk_manifests(
                    glob.walk(walk_root).not(ignores).map_err(invalid_glob)?,
                    walk_root,
                    workspace_root,
                    user_negations,
                    &mut manifest_paths,
                )?;
            }
        }
    }
    Ok(manifest_paths)
}

/// Read `root_dir`'s project from the first readable candidate.
/// `Ok(None)` when every candidate is gone or names no importer
/// manifest — the root then simply isn't a project.
fn read_first_project_manifest(
    root_dir: PathBuf,
    candidates: Vec<PathBuf>,
) -> Result<Option<Project>, FindWorkspaceProjectsError> {
    for manifest_path in candidates {
        let manifest = match read_exact_project_manifest(&manifest_path) {
            Ok(m) => m,
            Err(ReadProjectManifestError::Read(PackageManifestError::Io(err)))
                if err.kind() == ErrorKind::NotFound =>
            {
                continue;
            }
            Err(ReadProjectManifestError::ReadFile { source, .. })
                if source.kind() == ErrorKind::NotFound =>
            {
                continue;
            }
            Err(ReadProjectManifestError::Read(PackageManifestError::NoImporterManifestFound(
                _,
            ))) => continue,
            Err(err) => return Err(FindWorkspaceProjectsError::ReadManifest(err)),
        };
        return Ok(Some(Project { root_dir, manifest, dependency_manifest: None }));
    }
    Ok(None)
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
const PROJECT_MANIFEST_BASENAMES: &[&str] = &["package.json", "package.yaml"];

fn normalize_directory_pattern(pattern: &str) -> Option<&str> {
    let trimmed = pattern.trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "." {
        return None;
    }
    Some(trimmed)
}

#[derive(Debug, PartialEq, Eq)]
enum SpecializedPattern<'pattern> {
    Literal(&'pattern str),
    ChildrenOf(&'pattern str),
}

fn specialized_pattern(pattern: &str) -> Option<SpecializedPattern<'_>> {
    let pattern = normalize_directory_pattern(pattern)?;
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

fn normalize_manifest_patterns(pattern: &str) -> Vec<String> {
    let Some(trimmed) = normalize_directory_pattern(pattern) else { return Vec::new() };
    PROJECT_MANIFEST_BASENAMES.iter().map(|basename| format!("{trimmed}/{basename}")).collect()
}

fn collect_manifests_in_children(
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

fn collect_literal_manifests_in(
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

/// [`IGNORE_PATTERNS`] by hand: `**/node_modules/**` and
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
fn split_parent_prefix<'root, 'pattern>(
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
/// and the hoisted [`DOT_COMPONENT_IGNORE_PATTERN`] already says the same thing.
///
/// A wildcard must never match a dot-prefixed component, but a pattern that
/// spells one out must still reach it, and only there. Deriving one ignore per
/// wildcard position keeps that distinction: given `packages/.cache/*/lib`,
/// `.cache` stays reachable while `packages/.cache/.hidden/lib` and
/// `packages/.cache/.cache/lib` are both pruned. A wildcard that itself starts
/// with a dot, as in `packages/.*`, is asking for dot components and gets no
/// ignore.
fn positional_dot_ignores(pattern: &str) -> Option<Vec<String>> {
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

/// Drain a prepared walk into `manifest_paths`, absorbing `NotFound` and
/// applying the user negations that `Walk::not` cannot express.
fn collect_walk_manifests<Entries, Matched, Failure>(
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

fn is_literal_pattern(pattern: &str) -> bool {
    !pattern.chars().any(|ch| matches!(ch, '*' | '?' | '[' | ']' | '{' | '}'))
}

#[cfg(test)]
mod tests;
