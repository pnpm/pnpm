use crate::{
    fast_update_compose::Drift,
    fast_update_lockfile::{DroppedEdgeTarget, GraphEdits},
};
use pnpm_config::matcher::create_matcher;
use pnpm_lockfile::{Lockfile, PkgName};
use std::collections::{BTreeSet, HashMap, HashSet};

/// Whether `ignoredOptionalDependencies` drifted from what the lockfile
/// records, and whether the drift is a pure widening this rewrite can
/// absorb. Anything else — a dropped pattern, a new exclusion, a
/// previous configuration that ignores by default — is
/// [`Drift::Resolve`], because only a resolution can bring packages
/// back.
pub(crate) fn detect_ignored_optional_drift(
    lockfile: &Lockfile,
    ignored_optional_dependencies: &[String],
) -> Drift<()> {
    let previous: BTreeSet<_> = lockfile
        .ignored_optional_dependencies
        .as_deref()
        .unwrap_or_default()
        .iter()
        .cloned()
        .collect();
    let current: BTreeSet<_> = ignored_optional_dependencies.iter().cloned().collect();
    if previous == current {
        return Drift::Clean;
    }
    let added_exclusion = current.difference(&previous).any(|pattern| pattern.starts_with('!'));
    let previous_ignores_by_default =
        !previous.is_empty() && previous.iter().all(|pattern| pattern.starts_with('!'));
    if !previous.is_subset(&current) || added_exclusion || previous_ignores_by_default {
        return Drift::Resolve;
    }
    Drift::Absorb(())
}

/// Remove every optional dependency the widened pattern list now
/// ignores, from importers and snapshots alike, recording the severed
/// edges in `edits` for the shared epilogue.
pub(crate) fn apply_ignored_optional_update(
    candidate: &mut Lockfile,
    ignored_optional_dependencies: &[String],
    edits: &mut GraphEdits,
) {
    let matcher = create_matcher(ignored_optional_dependencies);
    for importer in candidate.importers.values_mut() {
        let removed = remove_ignored_optional_dependencies(
            &mut importer.optional_dependencies,
            &mut importer.dependencies,
            &matcher,
            edits,
        );
        if let Some(specifiers) = importer.specifiers.as_mut() {
            let removed_specifiers: HashSet<_> =
                removed.iter().map(std::string::ToString::to_string).collect();
            specifiers.retain(|name, _| !removed_specifiers.contains(name));
        }
    }
    if let Some(snapshots) = candidate.snapshots.as_mut() {
        for snapshot in snapshots.values_mut() {
            remove_ignored_optional_dependencies(
                &mut snapshot.optional_dependencies,
                &mut snapshot.dependencies,
                &matcher,
                edits,
            );
        }
    }
    let mut recorded: Vec<_> = ignored_optional_dependencies.to_vec();
    recorded.sort();
    recorded.dedup();
    candidate.ignored_optional_dependencies = Some(recorded);
}

fn remove_ignored_optional_dependencies<
    OptionalValue: DroppedEdgeTarget,
    DependencyValue: DroppedEdgeTarget,
>(
    optional_dependencies: &mut Option<HashMap<PkgName, OptionalValue>>,
    dependencies: &mut Option<HashMap<PkgName, DependencyValue>>,
    matcher: &pnpm_config::matcher::Matcher,
    edits: &mut GraphEdits,
) -> HashSet<PkgName> {
    let removed: HashSet<_> = optional_dependencies
        .as_ref()
        .into_iter()
        .flatten()
        .filter(|(name, _)| matches_package_name(matcher, name))
        .map(|(name, dependency)| {
            edits.dropped.record(name, dependency);
            name.clone()
        })
        .collect();
    for (name, dependency) in dependencies.as_ref().into_iter().flatten() {
        if removed.contains(name) {
            edits.dropped.record(name, dependency);
        }
    }
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

fn matches_package_name(matcher: &pnpm_config::matcher::Matcher, name: &PkgName) -> bool {
    if name.scope.is_some() {
        matcher.matches(&name.to_string())
    } else {
        matcher.matches(&name.bare)
    }
}

#[cfg(test)]
mod tests;
