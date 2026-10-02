use super::{BuildPhaseError, BuildPhaseInputs};
use crate::build_options::HoistedBinPlans;
use pnpm_cmd_shim::{Host, LinkBinsOptions, link_bins};
use pnpm_lockfile::{PackageKey, SnapshotEntry};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

#[cfg(all(test, unix))]
mod tests;

pub(super) fn link_options(
    options: &LinkBinsOptions,
    mutated_slots: bool,
) -> Cow<'_, LinkBinsOptions> {
    if !mutated_slots {
        return Cow::Borrowed(options);
    }
    let mut refreshed = options.clone();
    refreshed.force = true;
    Cow::Owned(refreshed)
}

pub(super) fn relink_dependency_bins(
    inputs: &BuildPhaseInputs<'_>,
    mutated: &HashSet<PackageKey>,
    plans: &HoistedBinPlans,
) -> Result<(), BuildPhaseError> {
    let options = link_options(inputs.directories.link_options, true);
    let Some(snapshots) = inputs.graph.snapshots else { return Ok(()) };
    let consumers: Vec<_> = snapshots
        .iter()
        .filter(|(key, snapshot)| mutated.contains(*key) || depends_on_mutation(snapshot, mutated))
        .map(|(key, _)| key.clone())
        .collect();
    if inputs.directories.is_hoisted {
        return relink_hoisted_bins(inputs, &options, (&consumers, mutated), plans);
    }
    crate::LinkVirtualStoreBins {
        layout: inputs.directories.layout,
        snapshots: inputs.graph.snapshots,
        selected_snapshots: Some(&consumers),
        packages: inputs.graph.packages,
        package_manifests: &crate::PackageManifests::default(),
        skipped: inputs.skipped,
        link_options: &options,
    }
    .run()
    .map_err(BuildPhaseError::DependencyBinLink)
}

fn relink_hoisted_bins(
    inputs: &BuildPhaseInputs<'_>,
    options: &LinkBinsOptions,
    selection: (&[PackageKey], &HashSet<PackageKey>),
    plans: &HoistedBinPlans,
) -> Result<(), BuildPhaseError> {
    let Some(roots) = inputs.directories.hoisted_pkg_roots_by_key else { return Ok(()) };
    for modules in hoisted_modules_dirs(roots, selection.0, selection.1) {
        if refresh_cached_directory(plans, &modules, roots, selection.1, options)? {
            continue;
        }
        link_bins::<Host>(&modules, &modules.join(".bin"), options)
            .map_err(BuildPhaseError::TopLevelBinLink)?;
    }
    Ok(())
}

pub(super) fn hoisted_modules_dirs(
    roots: &HashMap<PackageKey, Vec<PathBuf>>,
    consumers: &[PackageKey],
    mutated: &HashSet<PackageKey>,
) -> HashSet<PathBuf> {
    let own_modules = consumers
        .iter()
        .filter_map(|key| roots.get(key))
        .flatten()
        .map(|root| root.join("node_modules"));
    let physical_modules = mutated
        .iter()
        .filter_map(|key| roots.get(key))
        .flatten()
        .filter_map(|root| crate::link_hoisted_modules::containing_modules_dir(root))
        .map(PathBuf::from);
    own_modules.chain(physical_modules).collect()
}

fn depends_on_mutation(snapshot: &SnapshotEntry, mutated: &HashSet<PackageKey>) -> bool {
    snapshot.dependencies
        .iter()
        .chain(snapshot.optional_dependencies.iter())
        .flat_map(|dependencies| dependencies.iter())
        .filter_map(|(name, reference)| reference.resolve(name))
        .any(|key| mutated.contains(&key))
}

fn refresh_cached_directory(
    plans: &HoistedBinPlans,
    directory: &Path,
    roots: &HashMap<PackageKey, Vec<PathBuf>>,
    mutated: &HashSet<PackageKey>,
    options: &LinkBinsOptions,
) -> Result<bool, BuildPhaseError> {
    let Some(entry) = plans.existing_directory(directory) else { return Ok(false) };
    let mut entry = entry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let pending: HashSet<_> = mutated
        .difference(&entry.completed)
        .filter(|key| {
            roots
                .get(*key)
                .is_some_and(|locations| {
                    locations
                        .iter()
                        .any(|root| {
                            crate::link_hoisted_modules::containing_modules_dir(root)
                                == Some(directory)
                        })
                })
        })
        .cloned()
        .collect();
    let locations = pending
        .iter()
        .filter_map(|key| roots.get(key))
        .flatten()
        .filter(|root| crate::link_hoisted_modules::containing_modules_dir(root) == Some(directory))
        .cloned()
        .collect();
    let Some(plan) = entry.plan.as_mut() else { return Ok(false) };
    plan.refresh::<Host>(&locations, &directory.join(".bin"), options)
        .map_err(BuildPhaseError::TopLevelBinLink)?;
    entry.completed.extend(pending);
    Ok(true)
}

pub(super) fn importer_sources(
    plans: &HoistedBinPlans,
    directory: &Path,
    direct: &[String],
    hoisted: &[String],
) -> Option<HashMap<PathBuf, pnpm_cmd_shim::PreparedPackageBins>> {
    let entry = plans.existing_directory(directory)?;
    let entry = entry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let plan = entry.plan.as_ref()?;
    Some(
        direct
            .iter()
            .chain(hoisted)
            .filter_map(|name| {
                let location = directory.join(name);
                plan.package_bins(&location)
                    .map(|prepared| (location, prepared.with_build_pending(false)))
            })
            .collect(),
    )
}
