use super::super::{HashSet, Lockfile, Modules, VersionPart};

/// The `pendingBuilds` list for this install: the builds still owed,
/// carried-over entries first, then the ones this install deferred.
///
/// A build stays owed until something runs it, so a carried-over entry
/// survives unless its subject left the current lockfile or this run is
/// the `pnpm rebuild` that discharged it.
pub(in super::super) fn merge_pending_builds<Deferred>(
    previous: &[String],
    deferred: Deferred,
    current: Option<&Lockfile>,
    rebuild: Option<&crate::RebuildOptions>,
    rebuild_build_policy: Option<&crate::AllowBuildPolicy>,
) -> Vec<String>
where
    Deferred: IntoIterator<Item = String>,
{
    // An importer id and a dep path are both plain strings on disk, so
    // the current lockfile's `importers` — not the shape of the string —
    // decides which one an entry is.
    //
    // Only dependencies are settled here: the build phase has already
    // run by the time this file is written, while a project's scripts
    // run after it. `drain_settled_projects` discharges those once they
    // have actually succeeded. A dependency is settled only when the
    // rebuild both selected it and was allowed to build it — a selected
    // package the policy still blocks stays owed, matching pnpm's "drop
    // only what was actually rebuilt".
    let settled = |entry: &str| {
        let (Some(rebuild), Some(policy)) = (rebuild, rebuild_build_policy) else { return false };
        !current.is_some_and(|current| current.importers.contains_key(entry))
            && rebuild.settles_dependency(entry)
            && policy.check(pnpm_deps_path::remove_suffix(entry)) == Some(true)
    };
    let retained = previous.iter().filter(|entry| {
        current.is_some_and(|current| current_contains_dep_path(current, entry)) && !settled(entry)
    });
    let mut seen = HashSet::new();
    retained.cloned().chain(deferred).filter(|entry| seen.insert(entry.clone())).collect()
}
pub(in super::super) fn merge_filtered_modules_metadata(
    next: &mut Modules,
    previous: &Modules,
    current: &Lockfile,
    selected: &Lockfile,
) {
    merge_hoisted_dependencies(next, previous, current, selected);
    merge_hoisted_locations(next, previous, current, selected);
    merge_retained_pending_builds(next, previous, current, selected);
    merge_ignored_builds(next, previous, current, selected);
    merge_skipped(next, previous, current, selected);
    merge_injected_deps(next, previous, current, selected);
}
pub(super) fn merge_hoisted_dependencies(
    next: &mut Modules,
    previous: &Modules,
    current: &Lockfile,
    selected: &Lockfile,
) {
    for (dep_path, aliases) in &previous.hoisted_dependencies {
        if !retained_only_dep_path(current, selected, dep_path) {
            continue;
        }
        let retained_aliases = next.hoisted_dependencies.entry(dep_path.clone()).or_default();
        for (alias, kind) in aliases {
            retained_aliases.entry(alias.clone()).or_insert(*kind);
        }
    }
}
pub(super) fn merge_hoisted_locations(
    next: &mut Modules,
    previous: &Modules,
    current: &Lockfile,
    selected: &Lockfile,
) {
    let Some(previous_locations) = previous.hoisted_locations.as_ref() else { return };
    for (dep_path, locations) in previous_locations {
        if !retained_only_dep_path(current, selected, dep_path) {
            continue;
        }
        let retained_locations = next.hoisted_locations.get_or_insert_default();
        let retained = retained_locations.entry(dep_path.clone()).or_default();
        for location in locations {
            if !retained.contains(location) {
                retained.push(location.clone());
            }
        }
    }
}
pub(super) fn merge_retained_pending_builds(
    next: &mut Modules,
    previous: &Modules,
    current: &Lockfile,
    selected: &Lockfile,
) {
    let new_pending_builds = std::mem::take(&mut next.pending_builds);
    for dep_path in &previous.pending_builds {
        if retained_only_dep_path(current, selected, dep_path)
            && !next.pending_builds.contains(dep_path)
        {
            next.pending_builds.push(dep_path.clone());
        }
    }
    for dep_path in new_pending_builds {
        if !next.pending_builds.contains(&dep_path) {
            next.pending_builds.push(dep_path);
        }
    }
}
pub(super) fn merge_ignored_builds(
    next: &mut Modules,
    previous: &Modules,
    current: &Lockfile,
    selected: &Lockfile,
) {
    let new_ignored_builds = next.ignored_builds.take();
    if let Some(previous_ignored) = previous.ignored_builds.as_ref() {
        for dep_path in previous_ignored {
            if retained_only_dep_path(current, selected, dep_path.as_str()) {
                let retained_ignored = next.ignored_builds.get_or_insert_default();
                retained_ignored.insert(dep_path.clone());
            }
        }
    }
    if let Some(new_ignored_builds) = new_ignored_builds
        && !new_ignored_builds.is_empty()
    {
        next.ignored_builds.get_or_insert_default().extend(new_ignored_builds);
    }
}
pub(super) fn merge_skipped(
    next: &mut Modules,
    previous: &Modules,
    current: &Lockfile,
    selected: &Lockfile,
) {
    let new_skipped = std::mem::take(&mut next.skipped);
    for dep_path in &previous.skipped {
        if retained_only_dep_path(current, selected, dep_path) && !next.skipped.contains(dep_path) {
            next.skipped.push(dep_path.clone());
        }
    }
    for dep_path in new_skipped {
        if !next.skipped.contains(&dep_path) {
            next.skipped.push(dep_path);
        }
    }
}
/// A source the selected install re-materialized has its targets recomputed
/// in `next`, so the previous file's targets for it are stale — a bumped
/// injected dep moves to a new virtual-store slot and the old one is gone.
/// Only sources no selected importer touched carry their previous targets
/// forward.
pub(super) fn merge_injected_deps(
    next: &mut Modules,
    previous: &Modules,
    current: &Lockfile,
    selected: &Lockfile,
) {
    let current_injected_sources = injected_source_paths(current);
    let selected_injected_sources = injected_source_paths(selected);
    let Some(previous_injected) = previous.injected_deps.as_ref() else { return };
    for (source, targets) in previous_injected {
        if current_injected_sources.contains(source) && !selected_injected_sources.contains(source)
        {
            let retained_injected = next.injected_deps.get_or_insert_default();
            retained_injected.entry(source.clone()).or_insert_with(|| targets.clone());
        }
    }
}
pub(in super::super) fn retained_only_dep_path(
    current: &Lockfile,
    selected: &Lockfile,
    dep_path: &str,
) -> bool {
    current_contains_dep_path(current, dep_path) && !current_contains_dep_path(selected, dep_path)
}
pub(in super::super) fn injected_source_paths(lockfile: &Lockfile) -> HashSet<String> {
    lockfile
        .snapshots
        .iter()
        .flat_map(|snapshots| snapshots.keys())
        .chain(lockfile.packages.iter().flat_map(|packages| packages.keys()))
        .filter_map(|key| match key.suffix.version() {
            VersionPart::File(path) => Some(path.strip_prefix("./").unwrap_or(path).to_string()),
            VersionPart::Semver(_)
            | VersionPart::NonSemver(_)
            | VersionPart::RegistryQualified { .. } => None,
        })
        .collect()
}
pub(in super::super) fn current_contains_dep_path(current: &Lockfile, dep_path: &str) -> bool {
    if current.importers.contains_key(dep_path) {
        return true;
    }
    let Ok(key) = dep_path.parse::<pnpm_lockfile::PackageKey>() else { return false };
    current.snapshots.as_ref().is_some_and(|snapshots| snapshots.contains_key(&key))
        || current
            .packages
            .as_ref()
            .is_some_and(|packages| packages.contains_key(&key.without_peer()))
}
