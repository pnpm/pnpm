use super::{Path, lexical_normalize};

pub(super) fn is_ancestor_path(parent: &Path, child: &Path) -> bool {
    is_child_path(child, parent)
}

pub(super) fn is_child_path(child: &Path, parent: &Path) -> bool {
    has_path_prefix(child, parent) && !same_path(child, parent)
}

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
