use super::{BuildModulesError, BuildOneSnapshot, PackageKey};
use pnpm_cmd_shim::{Host, LinkBinsOptions, link_bins};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Mutex,
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
        refresh_directory(context.progress.refreshed_hoisted_bins, &directory, &completed, || {
            link_bins::<Host>(&directory, &directory.join(".bin"), options)
                .map_err(crate::LinkVirtualStoreBinsError::LinkBins)
                .map_err(BuildModulesError::BinLink)
        })?;
    }
    Ok(())
}

fn refresh_directory(
    refreshed: &Mutex<HashMap<PathBuf, HashSet<PackageKey>>>,
    directory: &Path,
    completed: &HashSet<PackageKey>,
    scan: impl FnOnce() -> Result<(), BuildModulesError>,
) -> Result<(), BuildModulesError> {
    let mut refreshed = refreshed.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let recorded = refreshed.entry(directory.to_owned()).or_default();
    if completed.is_subset(recorded) {
        return Ok(());
    }
    scan()?;
    // Other packages in this directory may still be building. Only the current
    // consumer's scheduled dependencies are known to have completed their writes.
    recorded.extend(completed.iter().cloned());
    Ok(())
}
