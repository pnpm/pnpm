use super::{FindWorkspaceInventoryError, WorkspaceInventoryExclusion};
use crate::directory_pattern::normalize_directory_pattern;
use std::path::Path;
use wax::{Glob, Program as _};

pub(super) struct DirectoryPattern {
    directory: Glob<'static>,
    subtree: Option<Glob<'static>>,
}

impl DirectoryPattern {
    pub(super) fn excludes_directory(&self, path: &Path) -> bool {
        self.subtree.as_ref().is_some_and(|pattern| pattern.is_match(path))
    }

    pub(super) fn excludes_manifest(&self, parent: &Path) -> bool {
        self.directory.is_match(parent) || self.excludes_directory(parent)
    }
}

pub(super) fn compile_patterns(
    exclusions: &[WorkspaceInventoryExclusion],
) -> Result<Vec<DirectoryPattern>, FindWorkspaceInventoryError> {
    let mut patterns = Vec::new();
    for exclusion in exclusions {
        let WorkspaceInventoryExclusion::Pattern(source) = exclusion else { continue };
        if source.starts_with('/') {
            continue;
        }
        let Some(normalized) = normalize_directory_pattern(source) else { continue };
        let subtree = normalized
            .strip_suffix("/**")
            .or_else(|| (normalized == "**").then_some("**"))
            .map(|pattern| compile_pattern(source, pattern))
            .transpose()?;
        patterns
            .push(DirectoryPattern { directory: compile_pattern(source, &normalized)?, subtree });
    }
    Ok(patterns)
}

fn compile_pattern(
    source: &str,
    normalized: &str,
) -> Result<Glob<'static>, FindWorkspaceInventoryError> {
    Glob::new(normalized).map(Glob::into_owned).map_err(|error| {
        FindWorkspaceInventoryError::InvalidGlob {
            pattern: source.to_string(),
            message: error.to_string(),
        }
    })
}
