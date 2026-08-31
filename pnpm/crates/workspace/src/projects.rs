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
    let ignore_template = build_ignores(&mut IGNORE_PATTERNS.iter().copied())?;
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

    // `NotFound` is the one error kind this enumeration absorbs, in both
    // the walk below and the manifest read after it: a pattern whose
    // directory is absent matches nothing, and a file may vanish between
    // being listed and being read. Every other kind — parse failures,
    // permission denials, "is a directory" — must propagate, so a
    // malformed `package.json` surfaces as a diagnostic instead of being
    // silently dropped from the workspace.
    let mut manifest_paths: BTreeSet<PathBuf> = BTreeSet::new();
    for pattern in include_patterns {
        match specialized_pattern(pattern) {
            Some(SpecializedPattern::ChildrenOf(parent)) => {
                collect_manifests_in_children(
                    &workspace_root.join(parent),
                    workspace_root,
                    &ignore_template,
                    &user_negations,
                    &mut manifest_paths,
                )?;
                continue;
            }
            Some(SpecializedPattern::Literal(directory)) => {
                collect_literal_manifests_in(
                    &workspace_root.join(directory),
                    workspace_root,
                    &ignore_template,
                    &user_negations,
                    &mut manifest_paths,
                );
                continue;
            }
            None => {}
        }

        for normalized in normalize_manifest_patterns(pattern) {
            let Some((walk_root, normalized)) = split_parent_prefix(workspace_root, &normalized)
            else {
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

            let dot_access = dot_component_access(pattern);
            let ignores = match dot_access {
                DotComponentAccess::Pruned => &dot_pruning_ignore_template,
                DotComponentAccess::Named(_) | DotComponentAccess::Unrestricted => &ignore_template,
            };
            let walk = glob.walk(walk_root).not(ignores.clone()).map_err(|err| {
                FindWorkspaceProjectsError::InvalidGlob {
                    pattern: pattern.to_string(),
                    message: err.to_string(),
                }
            })?;

            for entry in walk {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(err) => {
                        // Converting rather than restringifying keeps the
                        // underlying `io::ErrorKind`, which the skip below
                        // needs.
                        let err = std::io::Error::from(err);
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
                if let DotComponentAccess::Named(named) = &dot_access
                    && matched_a_dot_component_by_wildcard(manifest_path, walk_root, named)
                {
                    continue;
                }
                if pathdiff::diff_paths(manifest_path, workspace_root)
                    .is_some_and(|relative| user_negations.is_match(relative.as_path()))
                {
                    continue;
                }
                manifest_paths.insert(manifest_path.to_path_buf());
            }
        }
    }

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

    let mut projects = Vec::with_capacity(sorted.len());
    let mut seen_roots = BTreeSet::new();
    for manifest_path in sorted {
        let root_dir = manifest_path.parent().unwrap_or(workspace_root).to_path_buf();
        if seen_roots.contains(&root_dir) {
            continue;
        }
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
        seen_roots.insert(root_dir.clone());
        projects.push(Project { root_dir, manifest, dependency_manifest: None });
    }

    Ok(projects)
}

/// Hardcoded ignore patterns. Enumerating a real workspace excludes
/// only `node_modules` and `bower_components`, not the `**/test/**` /
/// `**/tests/**` directories that the lower-level package-finding path
/// excludes.
const IGNORE_PATTERNS: &[&str] = &["**/node_modules/**", "**/bower_components/**"];

/// Prunes every path with a dot-prefixed component, so a wildcard cannot
/// descend into `.git`, `.cache`, and friends. Applied only to patterns that
/// do not name a dot component themselves — see [`DotComponentAccess`].
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
    built_in_ignores: &wax::Any<'_>,
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
        collect_manifests_in_child(
            &entry.path(),
            workspace_root,
            built_in_ignores,
            user_negations,
            manifest_paths,
        )
    })
}

fn collect_literal_manifests_in(
    directory: &Path,
    workspace_root: &Path,
    built_in_ignores: &wax::Any<'_>,
    user_negations: &wax::Any<'_>,
    manifest_paths: &mut BTreeSet<PathBuf>,
) {
    for basename in PROJECT_MANIFEST_BASENAMES {
        let manifest_path = directory.join(basename);
        if manifest_path.is_file()
            && !is_ignored_manifest(
                &manifest_path,
                workspace_root,
                built_in_ignores,
                user_negations,
            )
        {
            manifest_paths.insert(manifest_path);
        }
    }
}

fn collect_manifests_in_child(
    directory: &Path,
    workspace_root: &Path,
    built_in_ignores: &wax::Any<'_>,
    user_negations: &wax::Any<'_>,
    manifest_paths: &mut BTreeSet<PathBuf>,
) -> Result<(), FindWorkspaceProjectsError> {
    for_each_directory_entry(directory, workspace_root, |entry| {
        if !PROJECT_MANIFEST_BASENAMES.iter().any(|basename| entry.file_name() == *basename) {
            return Ok(());
        }
        let manifest_path = entry.path();
        if !is_ignored_manifest(&manifest_path, workspace_root, built_in_ignores, user_negations) {
            manifest_paths.insert(manifest_path);
        }
        Ok(())
    })
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
    built_in_ignores: &wax::Any<'_>,
    user_negations: &wax::Any<'_>,
) -> bool {
    let relative = manifest_path.strip_prefix(workspace_root).unwrap_or(manifest_path);
    built_in_ignores.is_match(relative) || user_negations.is_match(relative)
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

/// How `pattern` is allowed to reach dot-prefixed path components.
///
/// A wildcard must never match a dot-prefixed component, but a pattern that
/// spells one out must still reach it, and only it: in `packages/.cache/*/lib`
/// the `*` stays subject to the usual rule.
enum DotComponentAccess<'pattern> {
    /// No segment names one, so they can be pruned during the walk.
    Pruned,
    /// Only these are reachable. Any other dot-prefixed component was matched
    /// by a wildcard and has to be dropped from the results.
    Named(Vec<&'pattern str>),
    /// A dotted wildcard such as `.*` names components it cannot enumerate.
    Unrestricted,
}

fn dot_component_access(pattern: &str) -> DotComponentAccess<'_> {
    let mut named = Vec::new();
    for segment in pattern.split('/') {
        if !segment.starts_with('.') || segment == "." || segment == ".." {
            continue;
        }
        if !is_literal_pattern(segment) {
            return DotComponentAccess::Unrestricted;
        }
        named.push(segment);
    }
    if named.is_empty() { DotComponentAccess::Pruned } else { DotComponentAccess::Named(named) }
}

fn matched_a_dot_component_by_wildcard(
    manifest_path: &Path,
    walk_root: &Path,
    named: &[&str],
) -> bool {
    manifest_path.strip_prefix(walk_root).unwrap_or(manifest_path).components().any(|component| {
        let component = component.as_os_str();
        starts_with_dot(component)
            && !named.iter().any(|named| std::ffi::OsStr::new(named) == component)
    })
}

fn starts_with_dot(name: &std::ffi::OsStr) -> bool {
    name.as_encoded_bytes().first() == Some(&b'.')
}

fn is_literal_pattern(pattern: &str) -> bool {
    !pattern.chars().any(|ch| matches!(ch, '*' | '?' | '[' | ']' | '{' | '}'))
}

#[cfg(test)]
mod tests;
