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
        package_bins: None,
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
    let mut entry = entry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let plan = entry.plan_or_seeded()?;
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

pub(super) fn finish_importer_top_level_bins(
    inputs: &BuildPhaseInputs<'_>,
    mutated_slots: bool,
    plans: &HoistedBinPlans,
) -> Result<(), BuildPhaseError> {
    let mut claimed_importer_bins = inputs.pending_bins
        .map(|pending| pending.claimed_importer_bins.clone())
        .unwrap_or_default();
    let modules_dir_name = inputs.policy.config.modules_dir_name();
    for (importer_id, importer_snapshot) in inputs.graph.importers {
        if let Some((project_dir, modules_dir, linked)) = super::link_importer_top_level_bins(
            inputs,
            mutated_slots,
            modules_dir_name,
            importer_id,
            importer_snapshot,
            plans,
        )? {
            extend_claimed_importer_bins(
                &mut claimed_importer_bins,
                &project_dir,
                &modules_dir,
                modules_dir_name,
                linked,
            );
        }
    }
    for (modules_dir, claimed) in &claimed_importer_bins {
        pnpm_cmd_shim::prune_stale_bins(&modules_dir.join(".bin"), claimed)
            .map_err(BuildPhaseError::TopLevelBinLink)?;
    }
    Ok(())
}

fn extend_claimed_importer_bins(
    claimed_importer_bins: &mut HashMap<PathBuf, HashSet<String>>,
    project_dir: &Path,
    modules_dir: &Path,
    modules_dir_name: &std::ffi::OsStr,
    linked: HashSet<String>,
) {
    let key = pnpm_fs::lexical_normalize(modules_dir);
    if let Some(claimed) = claimed_importer_bins.get_mut(&key) {
        claimed.extend(linked);
    } else if let Ok(real_project_dir) = std::fs::canonicalize(project_dir)
        && let Some(claimed) = claimed_importer_bins.get_mut(&pnpm_fs::lexical_normalize(
            &real_project_dir.join(modules_dir_name),
        ))
    {
        claimed.extend(linked);
    }
}
