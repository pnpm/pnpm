use pnpm_package_manifest::{DependencyGroup, PackageManifest};

/// Whether any selector is a wildcard or negation pattern.
pub fn has_remove_patterns(package_names: &[String]) -> bool {
    package_names
        .iter()
        .any(|name| name.contains('*') || name.starts_with('!'))
}

/// Expands glob and negation selectors against manifest dependencies.
pub fn expand_remove_patterns(
    manifest: &PackageManifest,
    package_names: &[String],
    save_type: Option<DependencyGroup>,
) -> Vec<String> {
    if !has_remove_patterns(package_names) {
        return package_names.to_vec();
    }
    if package_names
        .iter()
        .all(|name| name.starts_with('!'))
    {
        return Vec::new();
    }
    let matcher = pnpm_matcher::create_matcher(package_names);
    let mut expanded = package_names
        .iter()
        .filter(|name| !name.contains('*') && !name.starts_with('!'))
        .cloned()
        .collect::<Vec<_>>();
    for name in manifest
        .available_dependency_names(save_type)
        .into_iter()
        .filter(|name| matcher.matches(name))
    {
        if !expanded.contains(&name) {
            expanded.push(name);
        }
    }
    expanded
}

/// Selectors requiring exact matches that are not present in available dependencies.
pub fn missing_selected_dependencies<'a>(
    package_names: &'a [String],
    available_lookup: &std::collections::HashSet<String>,
) -> Vec<&'a String> {
    let has_patterns = has_remove_patterns(package_names);
    package_names
        .iter()
        .filter(|name| {
            if has_patterns {
                !name.contains('*')
                    && !name.starts_with('!')
                    && !available_lookup.contains(name.as_str())
            } else {
                !available_lookup.contains(name.as_str())
            }
        })
        .collect()
}
