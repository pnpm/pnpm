use pnpm_package_manifest::{
    DependencyGroup,
    PackageManifest,
};
use std::collections::HashSet;

fn is_remove_pattern(name: &str) -> bool {
    name.starts_with('!') || name.contains(['*', '?'])
}

/// Whether any selector is a wildcard or negation pattern.
pub fn has_remove_patterns(package_names: &[String]) -> bool {
    package_names.iter().any(|name| is_remove_pattern(name))
}

/// Dependency names a selected-project removal may target: the `save_type`
/// field, or every field including `peerDependencies` when `save_type` is
/// `None`.
pub fn selected_removable_names(
    manifest: &PackageManifest,
    save_type: Option<DependencyGroup>,
) -> Vec<String> {
    let mut names = manifest.available_dependency_names(save_type);
    if save_type.is_none() {
        let mut seen: HashSet<String> = names.iter().cloned().collect();
        names.extend(
            manifest
                .dependencies([DependencyGroup::Peer])
                .map(|(name, _)| name.to_string())
                .filter(|name| seen.insert(name.clone())),
        );
    }
    names
}

/// Expands glob and negation selectors against `available` dependency names.
///
/// The matcher decides which available names are removed, so an exclusion
/// also overrides an exact name. Exact names missing from `available` and
/// not excluded are kept, so validation reports them. Selectors without an include rule
/// expand to nothing.
pub fn expand_remove_patterns(available: &[String], package_names: &[String]) -> Vec<String> {
    if !has_remove_patterns(package_names) {
        return package_names.to_vec();
    }
    if !pnpm_matcher::has_include_pattern(package_names) {
        return Vec::new();
    }
    let matcher = pnpm_matcher::create_matcher(package_names);
    let missing_exact = package_names
        .iter()
        .filter(|name| !is_remove_pattern(name))
        .filter(|name| !available.contains(name))
        .filter(|name| matcher.matches(name));
    let matched = available
        .iter()
        .filter(|name| matcher.matches(name));
    missing_exact
        .chain(matched)
        .cloned()
        .collect()
}

/// Exact selectors, not excluded by a `!` pattern, that are missing from
/// `available_lookup`.
pub fn missing_selected_dependencies<'a>(
    package_names: &'a [String],
    available_lookup: &HashSet<String>,
) -> Vec<&'a String> {
    let matcher =
        has_remove_patterns(package_names).then(|| pnpm_matcher::create_matcher(package_names));
    package_names
        .iter()
        .filter(|name| !is_remove_pattern(name))
        .filter(|name| !available_lookup.contains(name.as_str()))
        .filter(|name| {
            matcher
                .as_ref()
                .is_none_or(|matcher| matcher.matches(name))
        })
        .collect()
}
