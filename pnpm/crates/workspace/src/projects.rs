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

use crate::{
    directory_pattern::normalize_directory_pattern,
    project_manifest::{ReadProjectManifestError, read_exact_project_manifest},
};
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
    Glob,
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

    let (include_patterns, user_negation_globs) = split_include_and_negation(patterns)?;

    let dot_pruning_ignore_template = dot_pruning_ignore_template()?;
    let user_negations = compile_user_negations(&user_negation_globs)?;

    parse_check_walk_patterns(&include_patterns, workspace_root)?;

    // Each pattern's set folds into the shared merge as it completes,
    // so peak memory stays one merged set plus the in-flight patterns —
    // overlapping patterns don't multiply it. Set union commutes and
    // the first error *in pattern-list order* wins, keeping the result
    // and the reported failure a function of the pattern list alone.
    let mut manifest_paths = merge_pattern_manifests(&MergePatterns {
        include_patterns: &include_patterns,
        workspace_root,
        dot_pruning_ignore_template: &dot_pruning_ignore_template,
        user_negations: &user_negations,
    })?;

    for basename in PROJECT_MANIFEST_BASENAMES {
        let root_manifest = workspace_root.join(basename);
        if root_manifest.is_file() {
            manifest_paths.insert(root_manifest);
        }
    }

    read_projects(group_manifests_by_root(manifest_paths, workspace_root))
}

/// wax's `not` takes a single pattern; combine the ignores with
/// `wax::any` so the walk filters them all in one pass.
fn dot_pruning_ignore_template() -> Result<wax::Any<'static>, FindWorkspaceProjectsError> {
    wax::any(IGNORE_PATTERNS.iter().copied().chain([DOT_COMPONENT_IGNORE_PATTERN])).map_err(|err| {
        FindWorkspaceProjectsError::InvalidGlob {
            pattern: "<built-in ignore>".to_string(),
            message: err.to_string(),
        }
    })
}

/// User negations are written relative to the workspace root, while a
/// parent-relative include walks from an ancestor of it, so they are
/// matched against the path each entry has *from the workspace root*
/// rather than handed to `Walk::not` alongside the built-in ignores.
fn compile_user_negations(globs: &[String]) -> Result<wax::Any<'_>, FindWorkspaceProjectsError> {
    wax::any(globs.iter().map(String::as_str)).map_err(|err| {
        FindWorkspaceProjectsError::InvalidGlob {
            pattern: "<negated pattern>".to_string(),
            message: err.to_string(),
        }
    })
}

fn read_projects(
    root_groups: Vec<(PathBuf, Vec<PathBuf>)>,
) -> Result<Vec<Project>, FindWorkspaceProjectsError> {
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

/// Split the configured patterns into the directories to include and the globs
/// to negate.
///
/// `!`-prefixed patterns are negations. wax does not accept `!` inside
/// `Glob::new()`, so they are split out and fed through `.not()` instead.
fn split_include_and_negation(
    patterns: &[String],
) -> Result<(Vec<WorkspacePattern<'_>>, Vec<String>), FindWorkspaceProjectsError> {
    let mut include_patterns = Vec::new();
    let mut user_negation_globs: Vec<String> = Vec::new();
    for pattern in patterns {
        let Some(body) = pattern.strip_prefix('!') else {
            if let Some(normalized) = normalize_directory_pattern(pattern) {
                include_patterns.push(WorkspacePattern { source: pattern, normalized });
            }
            continue;
        };
        collect_negation_globs(pattern, body, &mut user_negation_globs)?;
    }
    Ok((include_patterns, user_negation_globs))
}

/// Parse-check the generic-walk patterns up front, so a malformed glob fails
/// before any pattern pays for a workspace walk. The fast paths accept only
/// meta-character-free patterns, which cannot fail to parse.
fn parse_check_walk_patterns(
    include_patterns: &[WorkspacePattern<'_>],
    workspace_root: &Path,
) -> Result<(), FindWorkspaceProjectsError> {
    for WorkspacePattern { source, normalized: pattern } in include_patterns {
        if specialized_pattern(pattern).is_some() {
            continue;
        }
        for normalized in normalize_manifest_patterns(pattern) {
            let Some((_, normalized)) = split_parent_prefix(workspace_root, &normalized) else {
                continue;
            };
            Glob::new(normalized).map_err(|err| FindWorkspaceProjectsError::InvalidGlob {
                pattern: (*source).to_string(),
                message: err.to_string(),
            })?;
        }
    }
    Ok(())
}

/// Parse-check one `!`-prefixed pattern and add the globs it negates.
///
/// `!/...` remains a no-op: relative workspace paths never match that
/// absolute form.
fn collect_negation_globs(
    pattern: &str,
    body: &str,
    user_negation_globs: &mut Vec<String>,
) -> Result<(), FindWorkspaceProjectsError> {
    if body.starts_with('/') {
        return Ok(());
    }
    let Some(directory) = normalize_directory_pattern(body) else {
        return Ok(());
    };
    for normalized in normalize_manifest_patterns(&directory) {
        Glob::new(&normalized).map_err(|err| FindWorkspaceProjectsError::InvalidGlob {
            pattern: pattern.to_string(),
            message: err.to_string(),
        })?;
        user_negation_globs.push(normalized);
    }
    Ok(())
}

/// The include patterns to expand, and what every expansion filters against.
struct MergePatterns<'a> {
    include_patterns: &'a [WorkspacePattern<'a>],
    workspace_root: &'a Path,
    dot_pruning_ignore_template: &'a wax::Any<'a>,
    user_negations: &'a wax::Any<'a>,
}

/// Expand every include pattern and union what they match.
///
/// Each pattern's set folds into the shared merge as it completes, so peak
/// memory stays one merged set plus the in-flight patterns — overlapping
/// patterns don't multiply it. Set union commutes and the first error *in
/// pattern-list order* wins, keeping the result and the reported failure a
/// function of the pattern list alone.
fn merge_pattern_manifests(
    merge: &MergePatterns<'_>,
) -> Result<BTreeSet<PathBuf>, FindWorkspaceProjectsError> {
    let merged: std::sync::Mutex<BTreeSet<PathBuf>> = std::sync::Mutex::default();
    let pattern_errors: Vec<Option<FindWorkspaceProjectsError>> = merge
        .include_patterns
        .par_iter()
        .map(|pattern| {
            match collect_pattern_manifests(
                pattern,
                merge.workspace_root,
                merge.dot_pruning_ignore_template,
                merge.user_negations,
            ) {
                Ok(set) => {
                    merged.lock().expect("merge lock never poisoned").extend(set);
                    None
                }
                Err(error) => Some(error),
            }
        })
        .collect();
    if let Some(error) = pattern_errors.into_iter().flatten().next() {
        return Err(error);
    }
    Ok(merged.into_inner().expect("merge lock never poisoned"))
}

/// Group the manifests by the root directory they belong to, in `rootDir`
/// order.
///
/// A root's candidates stay in manifest-precedence order — `package.json`
/// before `package.yaml`, because the sort is stable and ties keep the set's
/// full-path order — and share one read task, so "first readable manifest
/// wins" holds under concurrency: a candidate that vanishes mid-run hands its
/// root to the next candidate, never to a skipped root.
fn group_manifests_by_root(
    manifest_paths: BTreeSet<PathBuf>,
    workspace_root: &Path,
) -> Vec<(PathBuf, Vec<PathBuf>)> {
    let mut sorted: Vec<PathBuf> = manifest_paths.into_iter().collect();
    sorted.sort_by(|left, right| {
        let dir_left = left.parent().unwrap_or_else(|| Path::new(""));
        let dir_right = right.parent().unwrap_or_else(|| Path::new(""));
        dir_left.cmp(dir_right)
    });
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
    root_groups
}

/// Expand one include pattern into the manifest paths it matches. The
/// contract [`find_workspace_projects_no_check`] states — which error
/// kinds are absorbed, how the fast paths and the generic walk divide
/// the pattern space — lives there; this is its per-pattern body.
fn collect_pattern_manifests(
    pattern: &WorkspacePattern<'_>,
    workspace_root: &Path,
    dot_pruning_ignore_template: &wax::Any<'_>,
    user_negations: &wax::Any<'_>,
) -> Result<BTreeSet<PathBuf>, FindWorkspaceProjectsError> {
    let mut manifest_paths: BTreeSet<PathBuf> = BTreeSet::new();
    match specialized_pattern(&pattern.normalized) {
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

    collect_glob_manifests(
        pattern,
        workspace_root,
        dot_pruning_ignore_template,
        user_negations,
        &mut manifest_paths,
    )?;

    Ok(manifest_paths)
}

fn collect_glob_manifests(
    pattern: &WorkspacePattern<'_>,
    workspace_root: &Path,
    dot_pruning_ignore_template: &wax::Any<'_>,
    user_negations: &wax::Any<'_>,
    manifest_paths: &mut BTreeSet<PathBuf>,
) -> Result<(), FindWorkspaceProjectsError> {
    for normalized in normalize_manifest_patterns(&pattern.normalized) {
        let Some((walk_root, normalized)) = split_parent_prefix(workspace_root, &normalized) else {
            continue;
        };
        if is_literal_pattern(normalized) && !walk_root.join(normalized).is_file() {
            continue;
        }
        let glob =
            Glob::new(normalized).map_err(|err| FindWorkspaceProjectsError::InvalidGlob {
                pattern: pattern.source.to_string(),
                message: err.to_string(),
            })?;

        let invalid_glob = |err: wax::BuildError| FindWorkspaceProjectsError::InvalidGlob {
            pattern: pattern.source.to_string(),
            message: err.to_string(),
        };
        let ignores =
            manifest_walk_ignores(normalized, dot_pruning_ignore_template).map_err(invalid_glob)?;
        collect_walk_manifests(
            glob.walk(walk_root).not(ignores).map_err(invalid_glob)?,
            walk_root,
            workspace_root,
            user_negations,
            manifest_paths,
        )?;
    }
    Ok(())
}

fn manifest_walk_ignores<'a>(
    normalized: &str,
    dot_pruning_ignore_template: &wax::Any<'a>,
) -> Result<wax::Any<'a>, wax::BuildError> {
    match positional_dot_ignores(normalized) {
        None => Ok(dot_pruning_ignore_template.clone()),
        Some(dot_ignores) => {
            let patterns = IGNORE_PATTERNS
                .iter()
                .copied()
                .chain(dot_ignores.iter().map(String::as_str))
                .map(|pattern| Glob::new(pattern).map(Glob::into_owned))
                .collect::<Result<Vec<_>, _>>()?;
            wax::any(patterns)
        }
    }
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

/// A configured include pattern. `normalized` drives discovery; `source` is
/// the text an invalid-glob diagnostic quotes back to the user.
struct WorkspacePattern<'source> {
    source: &'source str,
    normalized: String,
}

#[cfg(test)]
mod tests;

mod walk;
use walk::{
    SpecializedPattern, collect_literal_manifests_in, collect_manifests_in_children,
    collect_walk_manifests, is_literal_pattern, normalize_manifest_patterns,
    positional_dot_ignores, specialized_pattern, split_parent_prefix,
};
