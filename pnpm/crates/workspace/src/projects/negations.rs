use super::{FindWorkspaceProjectsError, Path};
use crate::directory_patterns::negated_directory_pattern;
use wax::{Glob, Program as _};

pub(super) struct CompiledNegations {
    pub(super) user: wax::Any<'static>,
    pub(super) subtree: wax::Any<'static>,
}

pub(super) fn compile_negations(
    patterns: &[String],
) -> Result<CompiledNegations, FindWorkspaceProjectsError> {
    let mut user_globs = Vec::new();
    let mut subtree_globs = Vec::new();
    for pattern in patterns {
        let Some(directory) = negated_directory_pattern(pattern)? else {
            continue;
        };
        let glob = Glob::new(&directory)
            .map(Glob::into_owned)
            .map_err(|err| FindWorkspaceProjectsError::InvalidGlob {
                pattern: pattern.clone(),
                message: err.to_string(),
            })?;
        if directory == "**" || directory.ends_with("/**") {
            subtree_globs.push(glob.clone());
        }
        user_globs.push(glob);
    }
    Ok(CompiledNegations { user: compile_any(user_globs)?, subtree: compile_any(subtree_globs)? })
}

fn compile_any(globs: Vec<Glob<'static>>) -> Result<wax::Any<'static>, FindWorkspaceProjectsError> {
    wax::any(globs)
        .map_err(|err| FindWorkspaceProjectsError::InvalidGlob {
            pattern: "<negated pattern>".to_string(),
            message: err.to_string(),
        })
}

pub(super) fn is_negated_directory(
    dir: &Path,
    workspace_root: &Path,
    user_negations: &wax::Any<'_>,
) -> bool {
    if let Ok(relative) = dir.strip_prefix(workspace_root) {
        return !relative.as_os_str().is_empty() && user_negations.is_match(relative);
    }
    pathdiff::diff_paths(dir, workspace_root)
        .is_some_and(|relative| user_negations.is_match(relative.as_path()))
}

pub(super) fn is_pruned_by_subtree_negation(
    dir: &Path,
    workspace_root: &Path,
    subtree_negations: &wax::Any<'_>,
) -> bool {
    if let Ok(relative) = dir.strip_prefix(workspace_root) {
        return !relative.as_os_str().is_empty() && subtree_negations.is_match(relative);
    }
    if workspace_root.starts_with(dir) {
        return false;
    }
    pathdiff::diff_paths(dir, workspace_root)
        .is_some_and(|relative| subtree_negations.is_match(relative.as_path()))
}
