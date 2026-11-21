use super::{
    RemoveValidationError,
    patterns::{
        missing_selected_dependencies,
        selected_removable_names,
    },
};
use pnpm_package_manifest::{
    DependencyGroup,
    PackageManifest,
};
use std::{
    collections::HashSet,
    fmt::Write as _,
};

fn collect_selected_available_dependencies(
    projects: &[pnpm_workspace::Project],
    selected_indices: &[usize],
    save_type: Option<DependencyGroup>,
) -> (HashSet<String>, Vec<String>) {
    let mut available_lookup = HashSet::new();
    let mut available_dependencies = Vec::new();
    for &index in selected_indices {
        for dep in selected_removable_names(&projects[index].manifest, save_type) {
            if available_lookup.insert(dep.clone()) {
                available_dependencies.push(dep);
            }
        }
    }
    available_dependencies.sort();
    (available_lookup, available_dependencies)
}

pub fn validate_selected_remove(
    projects: &[pnpm_workspace::Project],
    selected_indices: &[usize],
    package_names: &[String],
    save_type: Option<DependencyGroup>,
) -> Result<(), RemoveValidationError> {
    if package_names.is_empty() {
        return Err(RemoveValidationError::MustRemoveSomething);
    }
    let (available_lookup, available_dependencies) =
        collect_selected_available_dependencies(projects, selected_indices, save_type);
    let non_matched_dependencies = missing_selected_dependencies(package_names, &available_lookup);
    if non_matched_dependencies.is_empty() {
        return Ok(());
    }
    Err(cannot_remove_missing_deps(&available_dependencies, &non_matched_dependencies, save_type))
}

/// The up-front guards `pacquet remove` applies before mutating the
/// manifest or running any install; both fail fast.
pub fn validate_removable(
    manifest: &PackageManifest,
    package_names: &[String],
    save_type: Option<DependencyGroup>,
) -> Result<(), RemoveValidationError> {
    if package_names.is_empty() {
        return Err(RemoveValidationError::MustRemoveSomething);
    }
    let available_dependencies = manifest.available_dependency_names(save_type);
    let available_lookup: HashSet<&str> = available_dependencies
        .iter()
        .map(String::as_str)
        .collect();
    let non_matched_dependencies: Vec<&String> = package_names
        .iter()
        .filter(|name| !available_lookup.contains(name.as_str()))
        .collect();
    if non_matched_dependencies.is_empty() {
        return Ok(());
    }
    Err(cannot_remove_missing_deps(&available_dependencies, &non_matched_dependencies, save_type))
}

/// Build the `ERR_PNPM_CANNOT_REMOVE_MISSING_DEPS` error, with its
/// message and hint.
fn cannot_remove_missing_deps(
    available_dependencies: &[String],
    non_matched_dependencies: &[&String],
    target_dependencies_field: Option<DependencyGroup>,
) -> RemoveValidationError {
    let quoted = non_matched_dependencies
        .iter()
        .map(|dep| format!("'{dep}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut message = format!("Cannot remove {quoted}: ");
    if available_dependencies.is_empty() {
        match target_dependencies_field {
            Some(field) => {
                write!(message, "project has no '{}'", <&str>::from(field)).unwrap();
            }
            None => message.push_str("project has no dependencies of any kind"),
        }
        return RemoveValidationError::CannotRemoveMissingDeps { message, hint: None };
    }
    let noun = if non_matched_dependencies.len() > 1 { "dependencies" } else { "dependency" };
    let in_field = target_dependencies_field
        .map(|field| format!(" in '{}'", <&str>::from(field)))
        .unwrap_or_default();
    write!(message, "no such {noun} found{in_field}").unwrap();
    let hint = format!("Available dependencies: {}", available_dependencies.join(", "));
    RemoveValidationError::CannotRemoveMissingDeps { message, hint: Some(hint) }
}
