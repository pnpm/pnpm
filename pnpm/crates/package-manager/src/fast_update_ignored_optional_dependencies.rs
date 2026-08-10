use pacquet_config::matcher::create_matcher;
use pacquet_lockfile::{Lockfile, PkgName};
use std::collections::{BTreeSet, HashMap, HashSet};

pub(crate) fn try_fast_update_ignored_optional_dependencies(
    lockfile: &Lockfile,
    ignored_optional_dependencies: &[String],
) -> Option<Lockfile> {
    let previous: BTreeSet<_> = lockfile
        .ignored_optional_dependencies
        .as_deref()
        .unwrap_or_default()
        .iter()
        .cloned()
        .collect();
    let current: BTreeSet<_> = ignored_optional_dependencies.iter().cloned().collect();
    let added_exclusion = current.difference(&previous).any(|pattern| pattern.starts_with('!'));
    let previous_ignores_by_default =
        !previous.is_empty() && previous.iter().all(|pattern| pattern.starts_with('!'));
    if previous == current
        || !previous.is_subset(&current)
        || added_exclusion
        || previous_ignores_by_default
    {
        return None;
    }

    let matcher = create_matcher(ignored_optional_dependencies);
    let mut candidate = lockfile.clone();
    for importer in candidate.importers.values_mut() {
        let removed = remove_ignored_optional_dependencies(
            &mut importer.optional_dependencies,
            &mut importer.dependencies,
            &matcher,
        );
        if let Some(specifiers) = importer.specifiers.as_mut() {
            let removed_specifiers: HashSet<_> =
                removed.into_iter().map(|name| name.to_string()).collect();
            specifiers.retain(|name, _| !removed_specifiers.contains(name));
        }
    }
    if let Some(snapshots) = candidate.snapshots.as_mut() {
        for snapshot in snapshots.values_mut() {
            remove_ignored_optional_dependencies(
                &mut snapshot.optional_dependencies,
                &mut snapshot.dependencies,
                &matcher,
            );
        }
    }
    candidate.ignored_optional_dependencies = Some(current.into_iter().collect());
    crate::fast_update_lockfile::prune_unreachable_packages(&mut candidate);
    crate::fast_update_lockfile::prune_unreferenced_catalog_entries(&mut candidate);
    Some(candidate)
}

fn remove_ignored_optional_dependencies<OptionalValue, DependencyValue>(
    optional_dependencies: &mut Option<HashMap<PkgName, OptionalValue>>,
    dependencies: &mut Option<HashMap<PkgName, DependencyValue>>,
    matcher: &pacquet_config::matcher::Matcher,
) -> HashSet<PkgName> {
    let removed: HashSet<_> = optional_dependencies
        .as_ref()
        .into_iter()
        .flatten()
        .filter(|(name, _)| matches_package_name(matcher, name))
        .map(|(name, _)| name.clone())
        .collect();
    if let Some(optional_dependencies) = optional_dependencies.as_mut() {
        optional_dependencies.retain(|name, _| !removed.contains(name));
    }
    if optional_dependencies.as_ref().is_some_and(HashMap::is_empty) {
        *optional_dependencies = None;
    }
    if let Some(dependencies) = dependencies.as_mut() {
        dependencies.retain(|name, _| !removed.contains(name));
    }
    if dependencies.as_ref().is_some_and(HashMap::is_empty) {
        *dependencies = None;
    }
    removed
}

fn matches_package_name(matcher: &pacquet_config::matcher::Matcher, name: &PkgName) -> bool {
    if name.scope.is_some() {
        matcher.matches(&name.to_string())
    } else {
        matcher.matches(&name.bare)
    }
}

#[cfg(test)]
mod tests;
