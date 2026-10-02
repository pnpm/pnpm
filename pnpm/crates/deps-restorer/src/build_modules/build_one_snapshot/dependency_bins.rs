use super::{BuildModulesError, BuildOneSnapshot, PackageKey};
use crate::build_options::HoistedBinPlans;
use pnpm_cmd_shim::{DirectoryBinPlan, Host, LinkBinsOptions};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

#[cfg(test)]
mod tests;

pub(super) fn refresh(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
) -> Result<(), BuildModulesError> {
    let changed = changed_dependencies(context, snapshot_key);
    if changed.is_empty() {
        return Ok(());
    }
    let mut options = context.directories.link_options.clone();
    options.force = true;
    if context.directories.gather_ancestor_bin_paths {
        return refresh_hoisted(context, &changed, &options);
    }
    crate::LinkVirtualStoreBins {
        layout: context.directories.layout,
        snapshots: Some(context.graph.snapshots),
        selected_snapshots: Some(std::slice::from_ref(snapshot_key)),
        packages: context.graph.packages,
        package_manifests: &crate::PackageManifests::default(),
        skipped: context.graph.skipped,
        link_options: &options,
    }
    .run()
    .map_err(BuildModulesError::BinLink)
}

fn changed_dependencies(context: &BuildOneSnapshot<'_>, key: &PackageKey) -> Vec<PackageKey> {
    let Some(snapshot) = context.graph.snapshots.get(key) else { return Vec::new() };
    let mutations =
        context.progress.slot_mutations.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    snapshot.dependencies
        .iter()
        .flatten()
        .chain(snapshot.optional_dependencies.iter().flatten())
        .filter_map(|(name, reference)| reference.resolve(name))
        .filter(|dependency| mutations.contains(dependency))
        .collect()
}

fn refresh_hoisted(
    context: &BuildOneSnapshot<'_>,
    changed: &[PackageKey],
    options: &LinkBinsOptions,
) -> Result<(), BuildModulesError> {
    let Some(roots) = context.directories.pkg_roots_by_key else { return Ok(()) };
    let mut modules: HashMap<PathBuf, HashSet<PackageKey>> = HashMap::new();
    for key in changed {
        for root in roots.get(key).into_iter().flatten() {
            if let Some(directory) = crate::link_hoisted_modules::containing_modules_dir(root) {
                modules
                    .entry(directory.to_owned())
                    .or_default()
                    .insert(key.clone());
            }
        }
    }
    for (directory, completed) in modules {
        refresh_directory(
            context.progress.refreshed_hoisted_bins,
            &directory,
            &completed,
            |plan, pending| {
                let locations = completed_locations(roots, pending, &directory);
                plan.refresh::<Host>(&locations, &directory.join(".bin"), options)
                    .map_err(crate::LinkVirtualStoreBinsError::LinkBins)
                    .map_err(BuildModulesError::BinLink)
            },
        )?;
    }
    Ok(())
}

fn refresh_directory(
    refreshed: &HoistedBinPlans,
    directory: &Path,
    completed: &HashSet<PackageKey>,
    scan: impl FnOnce(&mut DirectoryBinPlan, &HashSet<PackageKey>) -> Result<(), BuildModulesError>,
) -> Result<(), BuildModulesError> {
    let entry = refreshed.directory(directory);
    let mut entry = entry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if completed.is_subset(&entry.completed) {
        return Ok(());
    }
    let pending = completed
        .difference(&entry.completed)
        .cloned()
        .collect();
    let plan = entry.plan.as_mut().expect("hoisted bin plan seeded before mutation");
    scan(plan, &pending)?;
    // Other packages may still be building; only this consumer's dependencies are complete.
    entry.completed.extend(completed.iter().cloned());
    Ok(())
}

pub(super) fn record_mutation(
    context: &BuildOneSnapshot<'_>,
    key: &PackageKey,
) -> Result<(), BuildModulesError> {
    if context.directories.gather_ancestor_bin_paths {
        for root in context.pkg_roots().all(key) {
            let Some(directory) = crate::link_hoisted_modules::containing_modules_dir(&root) else {
                continue;
            };
            let entry = context.progress.refreshed_hoisted_bins.directory(directory);
            let mut entry = entry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            entry
                .initialize(directory)
                .map_err(crate::LinkVirtualStoreBinsError::LinkBins)
                .map_err(BuildModulesError::BinLink)?;
        }
    }
    context.progress.record_slot_mutation(key);
    Ok(())
}

fn completed_locations(
    roots: &HashMap<PackageKey, Vec<PathBuf>>,
    completed: &HashSet<PackageKey>,
    directory: &Path,
) -> HashSet<PathBuf> {
    completed
        .iter()
        .filter_map(|key| roots.get(key))
        .flatten()
        .filter(|root| crate::link_hoisted_modules::containing_modules_dir(root) == Some(directory))
        .cloned()
        .collect()
}
