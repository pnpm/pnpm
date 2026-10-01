use super::{BuildPhaseError, BuildPhaseInputs};
use pnpm_cmd_shim::{Host, LinkBinsOptions, link_bins};
use pnpm_lockfile::{PackageKey, SnapshotEntry};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    path::PathBuf,
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
) -> Result<(), BuildPhaseError> {
    let options = link_options(inputs.directories.link_options, true);
    let Some(snapshots) = inputs.graph.snapshots else { return Ok(()) };
    let consumers: Vec<_> = snapshots
        .iter()
        .filter(|(key, snapshot)| mutated.contains(*key) || depends_on_mutation(snapshot, mutated))
        .map(|(key, _)| key.clone())
        .collect();
    if inputs.directories.is_hoisted {
        return relink_hoisted_bins(inputs, &options, (&consumers, mutated));
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
) -> Result<(), BuildPhaseError> {
    let Some(roots) = inputs.directories.hoisted_pkg_roots_by_key else { return Ok(()) };
    for modules in hoisted_modules_dirs(roots, selection.0, selection.1) {
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
