//! Answer the workspace-membership question for a single directory.

use super::{
    FindWorkspaceProjectsError, FindWorkspaceProjectsOpts, PROJECT_MANIFEST_BASENAMES, Path,
    WorkspacePattern, dot_pruning_ignore_template,
    managed::{is_under_ignored_directory, resolve_ignored_directories},
    negations::compile_negations,
    split_include_patterns,
    walk::{has_always_ignored_component, manifest_walk_ignores, split_parent_prefix},
};
use wax::{Glob, Program as _};

/// A project that the workspace does not include stands on its own, so pnpm
/// acts on that project alone instead of on the whole workspace: no shared
/// lockfile, no sibling projects, and no `pnpm-workspace.yaml` settings
/// layer, the same standing `--ignore-workspace` gives
/// (<https://github.com/pnpm/pnpm/issues/3561>).
///
/// A directory without a manifest of its own is not such a project. It is
/// some place inside the workspace, like a package's source directory, and a
/// command run from there still acts on the workspace.
///
/// `patterns` is `pnpm-workspace.yaml`'s `packages:`, absent when the key is.
pub fn belongs_to_workspace(
    workspace_dir: &Path,
    dir: &Path,
    patterns: Option<&[String]>,
) -> Result<bool, FindWorkspaceProjectsError> {
    if !needs_package_patterns(workspace_dir, dir) {
        return Ok(true);
    }
    let patterns = patterns.map_or_else(|| vec![".".to_string()], <[String]>::to_vec);
    is_workspace_project_dir(
        workspace_dir,
        dir,
        &FindWorkspaceProjectsOpts { patterns: Some(patterns), ..Default::default() },
    )
}

/// Does answering [`belongs_to_workspace`] for this pair need `packages:` at
/// all? Only a directory below the workspace root that carries a manifest of
/// its own can be a project the workspace leaves out. A caller that would have
/// to read `pnpm-workspace.yaml` to supply the patterns asks this first, so
/// the read happens only where it decides the answer.
#[must_use]
pub fn needs_package_patterns(workspace_dir: &Path, dir: &Path) -> bool {
    workspace_dir != dir && dir_has_project_manifest(dir)
}

fn dir_has_project_manifest(dir: &Path) -> bool {
    PROJECT_MANIFEST_BASENAMES
        .iter()
        .any(|basename| dir.join(basename).is_file())
}

/// Tells for a single directory what [`find_workspace_projects`] tells by
/// walking the whole workspace: whether the workspace rooted at
/// `workspace_root` contains a project in `dir`.
///
/// The workspace root is always a project
/// (<https://github.com/pnpm/pnpm/issues/1986>).
///
/// `dir` is the workspace root or a directory below it, which is what a
/// workspace lookup has in hand; anywhere else answers `false`. A `packages`
/// pattern may also reach above the root, and the walk does return such a
/// project, but nothing asks about one here.
///
/// [`find_workspace_projects`]: super::find_workspace_projects
pub fn is_workspace_project_dir(
    workspace_root: &Path,
    dir: &Path,
    opts: &FindWorkspaceProjectsOpts,
) -> Result<bool, FindWorkspaceProjectsError> {
    let Ok(relative_dir) = dir.strip_prefix(workspace_root) else {
        return Ok(false);
    };
    if relative_dir.as_os_str().is_empty() {
        return Ok(true);
    }
    // A managed directory is never a project, however the patterns read.
    let ignored_directories =
        resolve_ignored_directories(workspace_root, &opts.ignored_directories);
    if is_under_ignored_directory(dir, &ignored_directories)
        || has_always_ignored_component(relative_dir)
    {
        return Ok(false);
    }

    let default_patterns = [".".to_string(), "**".to_string()];
    let patterns: &[String] = opts.patterns.as_deref().unwrap_or(&default_patterns);
    let include_patterns = split_include_patterns(patterns);
    let negations = compile_negations(patterns)?;
    if negations.user.is_match(relative_dir) {
        return Ok(false);
    }

    let dot_pruning_ignore_template = dot_pruning_ignore_template()?;
    for pattern in &include_patterns {
        if pattern_selects(pattern, workspace_root, dir, &dot_pruning_ignore_template)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn pattern_selects(
    pattern: &WorkspacePattern<'_>,
    workspace_root: &Path,
    dir: &Path,
    dot_pruning_ignore_template: &wax::Any<'_>,
) -> Result<bool, FindWorkspaceProjectsError> {
    let invalid_glob = |err: wax::BuildError| FindWorkspaceProjectsError::InvalidGlob {
        pattern: pattern.source.to_string(),
        message: err.to_string(),
    };
    let Some((walk_root, normalized)) = split_parent_prefix(workspace_root, &pattern.normalized)
    else {
        return Ok(false);
    };
    let Ok(candidate) = dir.strip_prefix(walk_root) else {
        return Ok(false);
    };
    if normalized == "." {
        return Ok(candidate.as_os_str().is_empty());
    }
    let glob = Glob::new(normalized).map_err(invalid_glob)?;
    let ignores =
        manifest_walk_ignores(normalized, dot_pruning_ignore_template).map_err(invalid_glob)?;
    Ok(!candidate.as_os_str().is_empty()
        && glob.is_match(candidate)
        && !ignores.is_match(candidate))
}
