//! Answer the workspace-membership question for a single directory.

use super::{
    FindWorkspaceProjectsError, FindWorkspaceProjectsOpts, PROJECT_MANIFEST_BASENAMES, Path,
    PathBuf, WorkspacePattern, compile_user_negations, dot_pruning_ignore_template,
    manifest_walk_ignores, split_include_and_negation,
    walk::{has_always_ignored_component, normalize_manifest_patterns, split_parent_prefix},
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
    if workspace_dir == dir || !dir_has_project_manifest(dir) {
        return Ok(true);
    }
    let patterns = patterns.map_or_else(|| vec![".".to_string()], <[String]>::to_vec);
    is_workspace_project_dir(
        workspace_dir,
        dir,
        &FindWorkspaceProjectsOpts { patterns: Some(patterns) },
    )
}

fn dir_has_project_manifest(dir: &Path) -> bool {
    PROJECT_MANIFEST_BASENAMES
        .iter()
        .any(|basename| dir.join(basename).is_file())
}

/// Tells for a single directory what [`find_workspace_projects`] tells by
/// walking the whole workspace: whether the workspace rooted at
/// `workspace_root` contains a project in `dir`. Both expand the patterns
/// into the same manifest globs, so a directory this accepts is a directory
/// the walk returns.
///
/// The workspace root is always a project
/// (<https://github.com/pnpm/pnpm/issues/1986>).
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

    let default_patterns = [".".to_string(), "**".to_string()];
    let patterns: &[String] = opts.patterns.as_deref().unwrap_or(&default_patterns);
    let (include_patterns, user_negation_globs) = split_include_and_negation(patterns)?;
    let user_negations = compile_user_negations(&user_negation_globs)?;

    // Which basename the directory actually holds is left open: the caller
    // asks about the directory, and a pattern selects every basename or none.
    let candidates: Vec<PathBuf> = PROJECT_MANIFEST_BASENAMES
        .iter()
        .map(|basename| relative_dir.join(basename))
        .filter(|candidate| {
            !has_always_ignored_component(candidate)
                && !user_negations.is_match(candidate.as_path())
        })
        .collect();
    if candidates.is_empty() {
        return Ok(false);
    }

    let dot_pruning_ignore_template = dot_pruning_ignore_template()?;
    for pattern in &include_patterns {
        if pattern_selects(pattern, workspace_root, &candidates, &dot_pruning_ignore_template)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn pattern_selects(
    pattern: &WorkspacePattern<'_>,
    workspace_root: &Path,
    candidates: &[PathBuf],
    dot_pruning_ignore_template: &wax::Any<'_>,
) -> Result<bool, FindWorkspaceProjectsError> {
    let invalid_glob = |err: wax::BuildError| FindWorkspaceProjectsError::InvalidGlob {
        pattern: pattern.source.to_string(),
        message: err.to_string(),
    };
    for normalized in normalize_manifest_patterns(&pattern.normalized) {
        let Some((walk_root, normalized)) = split_parent_prefix(workspace_root, &normalized) else {
            continue;
        };
        let glob = Glob::new(normalized).map_err(invalid_glob)?;
        let ignores =
            manifest_walk_ignores(normalized, dot_pruning_ignore_template).map_err(invalid_glob)?;
        for candidate in candidates {
            // A `../` prefix moves the directory the glob is anchored at
            // above the workspace root, so the candidate is re-expressed
            // against that directory before it is matched.
            let absolute = workspace_root.join(candidate);
            let Ok(candidate) = absolute.strip_prefix(walk_root) else {
                continue;
            };
            if glob.is_match(candidate) && !ignores.is_match(candidate) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
