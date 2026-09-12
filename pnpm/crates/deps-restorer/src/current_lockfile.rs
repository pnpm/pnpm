//! Filter a wanted lockfile down to the "current" shape pacquet
//! writes under `<virtual_store_dir>/lock.yaml`.
//!
//! Rather than re-running the engine + `supportedArchitectures` +
//! `skipped` checks at filter time, reuse the [`SkippedSnapshots`]
//! set produced during install — its
//! [transient subset][`SkippedSnapshots::transient_only`], since
//! installability skips stay in the file.
//!
//! The output drives the **next** install's diff. Without this
//! filter, pacquet's current lockfile recorded every snapshot the
//! resolver imagined, including ones `--no-optional` or a failed
//! optional fetch dropped — so a follow-up install would think
//! those slots were already on disk and skip work that should
//! actually run.

mod reachability;
use reachability::collect_reachable;

use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_lockfile::{Lockfile, PackageKey, ProjectSnapshot, ResolvedDependencyMap};
use pnpm_modules_yaml::IncludedDependencies;

use crate::SkippedSnapshots;

pub struct MaterializationClosure {
    pub lockfile: Lockfile,
    pub importer_ids: HashSet<String>,
}

#[derive(Debug, Display, Error, Diagnostic)]
pub enum MergeFilteredWantedLockfileError {
    #[display("fresh lockfile is missing importer {importer_id}")]
    #[diagnostic(code(pnpm_package_manager::missing_fresh_lockfile_importer))]
    MissingImporter {
        #[error(not(source))]
        importer_id: String,
    },
}

#[must_use]
pub fn materialization_closure(
    lockfile: &Lockfile,
    workspace_root: &Path,
    initial_importer_ids: &HashSet<String>,
    included: IncludedDependencies,
    skipped: &SkippedSnapshots,
) -> MaterializationClosure {
    let reachable =
        collect_reachable(lockfile, workspace_root, initial_importer_ids, included, |key| {
            skipped.contains(key)
        });
    let metadata_reachable =
        collect_reachable(lockfile, workspace_root, initial_importer_ids, included, |key| {
            skipped.contains_optional_excluded(key)
        });
    let reachable_metadata = metadata_reachable
        .snapshot_keys
        .iter()
        .map(PackageKey::without_peer)
        .collect::<HashSet<_>>();
    let importers = lockfile
        .importers
        .iter()
        .filter(|(id, _)| reachable.importer_ids.contains(*id))
        .map(|(id, importer)| {
            (id.clone(), filter_importer(importer, included, &reachable.snapshot_keys))
        })
        .collect();
    let snapshots = lockfile.snapshots.as_ref().map(|snapshots| {
        snapshots
            .iter()
            .filter(|(key, _)| reachable.snapshot_keys.contains(*key))
            .map(|(key, snapshot)| (key.clone(), snapshot.clone()))
            .collect()
    });
    let packages = lockfile.packages.as_ref().map(|packages| {
        packages
            .iter()
            .filter(|(key, _)| reachable_metadata.contains(*key))
            .map(|(key, package)| (key.clone(), package.clone()))
            .collect()
    });

    MaterializationClosure {
        lockfile: lockfile_with_graph(lockfile, importers, packages, snapshots),
        importer_ids: reachable.importer_ids,
    }
}

/// Build the complete wanted lockfile for a filtered install: the
/// selected importers take their freshly-resolved entries, and every other
/// importer in `real_importer_ids` keeps the pins `previous_wanted`
/// recorded. The wanted lockfile stays workspace-global no matter how
/// narrow the filter is, so `pnpm install --filter` never truncates the
/// entries of the projects it did not touch.
///
/// `freshly_resolved` must contain an entry for every id in
/// `real_importer_ids`, not just the selected ones: when a global
/// resolution input (settings, catalogs, overrides, a pnpmfile, ...) drifts
/// from `previous_wanted`, the previous entries cannot be reused and every
/// importer falls back to its fresh entry. Resolving only the selected
/// subset therefore fails with [`MergeFilteredWantedLockfileError::MissingImporter`]
/// exactly when those inputs changed.
pub fn merge_filtered_wanted_lockfile(
    previous_wanted: Option<&Lockfile>,
    mut freshly_resolved: Lockfile,
    real_importer_ids: &HashSet<String>,
    selected_importer_ids: &HashSet<String>,
    workspace_root: &Path,
) -> Result<Lockfile, MergeFilteredWantedLockfileError> {
    let can_reuse_unselected_importers = previous_wanted
        .is_some_and(|previous| resolution_inputs_match(previous, &freshly_resolved));
    let mut fresh_importers = std::mem::take(&mut freshly_resolved.importers);
    let fresh_packages = freshly_resolved.packages.take();
    let fresh_snapshots = freshly_resolved.snapshots.take();
    let mut importer_ids = real_importer_ids.iter().collect::<Vec<_>>();
    importer_ids.sort();
    freshly_resolved.importers = importer_ids
        .into_iter()
        .map(|importer_id| {
            let previous_importer =
                previous_wanted.and_then(|lockfile| lockfile.importers.get(importer_id));
            let importer = match previous_importer {
                Some(previous_importer)
                    if can_reuse_unselected_importers
                        && !selected_importer_ids.contains(importer_id) =>
                {
                    previous_importer.clone()
                }
                _ => fresh_importers.remove(importer_id).ok_or_else(|| {
                    MergeFilteredWantedLockfileError::MissingImporter {
                        importer_id: importer_id.clone(),
                    }
                })?,
            };
            Ok((importer_id.clone(), importer))
        })
        .collect::<Result<_, MergeFilteredWantedLockfileError>>()?;
    freshly_resolved.packages = overlay_package_maps(
        previous_wanted.and_then(|lockfile| lockfile.packages.as_ref()),
        fresh_packages,
    );
    freshly_resolved.snapshots = overlay_package_maps(
        previous_wanted.and_then(|lockfile| lockfile.snapshots.as_ref()),
        fresh_snapshots,
    );
    Ok(full_closure(&freshly_resolved, workspace_root))
}

// Retaining old importer pins is safe only while the workspace-wide resolution inputs match.
fn resolution_inputs_match(previous: &Lockfile, fresh: &Lockfile) -> bool {
    previous.lockfile_version == fresh.lockfile_version
        && previous.settings == fresh.settings
        && previous.catalogs == fresh.catalogs
        && previous.overrides == fresh.overrides
        && previous.package_extensions_checksum == fresh.package_extensions_checksum
        && previous.pnpmfile_checksum == fresh.pnpmfile_checksum
        && previous.ignored_optional_dependencies == fresh.ignored_optional_dependencies
        && previous.patched_dependencies == fresh.patched_dependencies
}

#[must_use]
pub fn merge_filtered_current_lockfile(
    previous_current: Option<&Lockfile>,
    wanted: &Lockfile,
    requested_importer_ids: &HashSet<String>,
    included: IncludedDependencies,
    skipped: &SkippedSnapshots,
    workspace_root: &Path,
) -> Lockfile {
    let selected = materialization_closure(
        wanted,
        workspace_root,
        requested_importer_ids,
        included,
        &skipped.transient_only(),
    );
    let Some(previous_current) = previous_current else {
        return selected.lockfile;
    };
    let retained = retained_closure(previous_current, &selected.importer_ids, workspace_root);
    let selected_packages = selected.lockfile.packages.clone();
    let merged = overlay_lockfiles(wanted, previous_current, retained, selected.lockfile);
    let mut final_lockfile = full_closure(&merged, workspace_root);
    if let Some(selected_packages) = selected_packages {
        final_lockfile.packages.get_or_insert_default().extend(selected_packages);
    }
    restore_skipped_package_metadata(&mut final_lockfile, &merged, skipped);
    final_lockfile
}

/// The closure of the previous install's importers this install left
/// alone, so their entries survive the merge.
fn retained_closure(
    previous_current: &Lockfile,
    selected_importer_ids: &HashSet<String>,
    workspace_root: &Path,
) -> Lockfile {
    let retained_importers = previous_current
        .importers
        .iter()
        .filter(|(importer_id, _)| !selected_importer_ids.contains(*importer_id))
        .map(|(importer_id, importer)| (importer_id.clone(), importer.clone()))
        .collect::<HashMap<_, _>>();
    let retained_importer_ids = retained_importers.keys().cloned().collect();
    let retained_source = lockfile_with_graph(
        previous_current,
        retained_importers,
        previous_current.packages.clone(),
        previous_current.snapshots.clone(),
    );
    materialization_closure(
        &retained_source,
        workspace_root,
        &retained_importer_ids,
        all_dependencies(),
        &SkippedSnapshots::new(),
    )
    .lockfile
}

/// The retained importers and graph under the selected ones, over the
/// previous install's package metadata.
fn overlay_lockfiles(
    wanted: &Lockfile,
    previous_current: &Lockfile,
    retained: Lockfile,
    selected: Lockfile,
) -> Lockfile {
    let mut importers = retained.importers;
    importers.extend(selected.importers);
    let packages = overlay_package_maps(previous_current.packages.as_ref(), selected.packages);
    let snapshots = overlay_package_maps(retained.snapshots.as_ref(), selected.snapshots);
    lockfile_with_graph(wanted, importers, packages, snapshots)
}

/// The closure over every importer and dependency group of `lockfile`.
fn full_closure(lockfile: &Lockfile, workspace_root: &Path) -> Lockfile {
    let importer_ids = lockfile.importers.keys().cloned().collect();
    materialization_closure(
        lockfile,
        workspace_root,
        &importer_ids,
        all_dependencies(),
        &SkippedSnapshots::new(),
    )
    .lockfile
}

/// Put back the `packages` rows of snapshots the install skipped. The
/// reachability walk drops them along with their snapshots, but a
/// skipped snapshot an optional-excluded parent did not cut is still
/// part of the graph the next install diffs against.
fn restore_skipped_package_metadata(
    final_lockfile: &mut Lockfile,
    merged: &Lockfile,
    skipped: &SkippedSnapshots,
) {
    let (Some(merged_packages), Some(final_packages)) =
        (merged.packages.as_ref(), final_lockfile.packages.as_mut())
    else {
        return;
    };
    for skipped_key in skipped.iter() {
        if skipped.contains_optional_excluded(skipped_key) {
            continue;
        }
        let metadata_key = skipped_key.without_peer();
        if let Some(metadata) = merged_packages.get(&metadata_key) {
            final_packages.insert(metadata_key, metadata.clone());
        }
    }
}

/// Extend `skipped` with every snapshot the importers can only reach
/// through an installability-skipped snapshot. pnpm's `.modules.yaml`
/// `skipped` list records this full closure: when a platform-incompatible
/// optional package is skipped, its own dependency subtree is not
/// materialized either, and both stacks must record the same set.
///
/// Only the persisted `installability` subset seeds the closure — and
/// only that subset receives the additions. Transient skips
/// (`--no-optional`, `--no-runtime`, a failed optional fetch) must not
/// leak into `.modules.yaml`, or a later install's seed would keep their
/// subtrees skipped after the transient condition is gone; their
/// subtrees are already cut from materialization by the reachability
/// walks that consume the full skip-set union.
pub fn extend_skipped_with_dependency_closure(
    skipped: &mut SkippedSnapshots,
    lockfile: &Lockfile,
    workspace_root: &Path,
    importer_ids: &HashSet<String>,
    included: IncludedDependencies,
) {
    if skipped.iter_installability().next().is_none() {
        return;
    }
    let full = collect_reachable(lockfile, workspace_root, importer_ids, included, |_| false);
    let kept = collect_reachable(lockfile, workspace_root, importer_ids, included, |key| {
        skipped.contains_installability(key)
    });
    for key in full.snapshot_keys {
        if !kept.snapshot_keys.contains(&key) && !skipped.contains(&key) {
            skipped.insert_installability(key);
        }
    }
}

fn all_dependencies() -> IncludedDependencies {
    IncludedDependencies { dependencies: true, dev_dependencies: true, optional_dependencies: true }
}

fn overlay_package_maps<Value: Clone>(
    previous: Option<&HashMap<PackageKey, Value>>,
    fresh: Option<HashMap<PackageKey, Value>>,
) -> Option<HashMap<PackageKey, Value>> {
    if previous.is_none() && fresh.is_none() {
        return None;
    }
    let mut merged = previous.cloned().unwrap_or_default();
    if let Some(fresh) = fresh {
        merged.extend(fresh);
    }
    Some(merged)
}

// Host top-level blocks describe the project, not materialization, and stay in the wanted lockfile.
fn lockfile_with_graph(
    source: &Lockfile,
    importers: HashMap<String, ProjectSnapshot>,
    packages: Option<HashMap<PackageKey, pnpm_lockfile::PackageMetadata>>,
    snapshots: Option<HashMap<PackageKey, pnpm_lockfile::SnapshotEntry>>,
) -> Lockfile {
    Lockfile {
        lockfile_version: source.lockfile_version,
        settings: source.settings.clone(),
        catalogs: source.catalogs.clone(),
        overrides: source.overrides.clone(),
        package_extensions_checksum: source.package_extensions_checksum.clone(),
        pnpmfile_checksum: source.pnpmfile_checksum.clone(),
        ignored_optional_dependencies: source.ignored_optional_dependencies.clone(),
        patched_dependencies: source.patched_dependencies.clone(),
        importers,
        packages,
        snapshots,
        time: source.time.clone(),
        extra: pnpm_lockfile::LockfileExtra::default(),
    }
}

/// Build the "current lockfile" shape from the wanted lockfile by
/// applying the install-time `include` set and skip set.
///
/// Importers lose dep maps whose `include` flag is false; importer
/// `optionalDependencies` lose entries whose resolved snapshot got
/// skipped; the snapshot map is pruned to the transitive closure
/// reachable from the surviving importer roots. Only the transient
/// skips prune: installability-skipped entries survive in both maps,
/// matching pnpm's current-lockfile shape while `.modules.yaml.skipped`
/// records the materialization skip.
#[must_use]
pub fn filter_lockfile_for_current(
    lockfile: &Lockfile,
    included: IncludedDependencies,
    skipped: &SkippedSnapshots,
) -> Lockfile {
    let all_importer_ids = lockfile.importers.keys().cloned().collect();
    // Every importer is a root here, so no importer has to be *discovered*
    // through a `link:` dep and the walk needs no real workspace root to
    // resolve those links against. The empty root keeps the walk in the
    // lockfile-relative space importer IDs already use — `importer_root_dir("", id)`
    // is `id` — so link resolution stays correct rather than merely unused.
    materialization_closure(
        lockfile,
        Path::new(""),
        &all_importer_ids,
        included,
        &skipped.transient_only(),
    )
    .lockfile
}

/// Per-importer filter: drop dep maps whose `include` flag is
/// false; further trim `optional_dependencies` to entries whose
/// resolved snapshot survived the reachability walk.
///
/// Two steps: first clear the excluded dep sections, then
/// post-filter `optionalDependencies` against the surviving
/// packages set.
fn filter_importer(
    importer: &ProjectSnapshot,
    included: IncludedDependencies,
    reachable: &HashSet<PackageKey>,
) -> ProjectSnapshot {
    let mut out = importer.clone();
    if !included.dependencies {
        out.dependencies = None;
    }
    if !included.dev_dependencies {
        out.dev_dependencies = None;
    }
    if !included.optional_dependencies {
        out.optional_dependencies = None;
    } else if let Some(opt) = out.optional_dependencies.as_mut() {
        retain_reachable(opt, reachable);
    }
    out
}

/// Drop importer-level optional-dep entries whose resolved
/// snapshot key isn't in `reachable`. `link:` entries (workspace
/// siblings) survive — they don't live in the snapshot graph.
fn retain_reachable(map: &mut ResolvedDependencyMap, reachable: &HashSet<PackageKey>) {
    map.retain(|name, spec| {
        let Some(key) = spec.version.resolved_key(name) else {
            // Workspace `link:<path>` — no snapshot to check.
            return true;
        };
        reachable.contains(&key)
    });
}

#[cfg(test)]
mod tests;
