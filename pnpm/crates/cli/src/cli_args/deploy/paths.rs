//! Lexical comparison of deploy paths and the relative paths the deployed
//! manifest records.

use super::{IntoDiagnostic, Path, PathBuf, lexical_normalize};

pub(super) fn same_path(left: &Path, right: &Path) -> bool {
    let left = lexical_normalize(left);
    let right = lexical_normalize(right);
    path_components_match(&left, &right)
}

/// Hash key under which two paths collide exactly when [`same_path`] equates
/// them.
#[derive(PartialEq, Eq, Hash)]
pub(super) struct ProjectPathKey(Vec<String>);

impl ProjectPathKey {
    pub(super) fn new(path: &Path) -> Self {
        Self(comparable_path_components(&lexical_normalize(path)))
    }
}

pub(super) fn has_path_prefix(child: &Path, parent: &Path) -> bool {
    let child = lexical_normalize(child);
    let parent = lexical_normalize(parent);
    let child_components = comparable_path_components(&child);
    let parent_components = comparable_path_components(&parent);
    child_components.len() >= parent_components.len()
        && child_components
            .iter()
            .zip(parent_components.iter())
            .all(|(child, parent)| child == parent)
}

fn path_components_match(left: &Path, right: &Path) -> bool {
    comparable_path_components(left) == comparable_path_components(right)
}

fn comparable_path_components(path: &Path) -> Vec<String> {
    path.components()
        .map(|component| {
            comparison_component(
                component
                    .as_os_str()
                    .to_string_lossy()
                    .as_ref(),
            )
        })
        .collect()
}

#[cfg(windows)]
fn comparison_component(component: &str) -> String {
    component.to_lowercase()
}

#[cfg(not(windows))]
fn comparison_component(component: &str) -> String {
    component.to_string()
}

pub(super) fn relative_components_from_child(
    parent: &Path,
    child: &Path,
) -> miette::Result<Vec<PathBuf>> {
    let parent = lexical_normalize(parent);
    let child = lexical_normalize(child);
    if !has_path_prefix(&child, &parent) {
        child.strip_prefix(&parent).into_diagnostic()?;
    }
    Ok(child
        .components()
        .skip(parent.components().count())
        .map(|component| PathBuf::from(component.as_os_str()))
        .collect())
}

pub(super) fn relative_path(from: &Path, to: &Path) -> String {
    let relative = pathdiff::diff_paths(to, from).unwrap_or_else(|| to.to_path_buf());
    relative.to_string_lossy().replace('\\', "/")
}
