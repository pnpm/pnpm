//! Filter a wanted lockfile down to the "current" shape pacquet
//! writes under `<virtual_store_dir>/lock.yaml`.
//!
//! Rather than re-running the engine + `supportedArchitectures` +
//! `skipped` checks at filter time, reuse the [`SkippedSnapshots`]
//! set produced during install. The full set filters materialized
//! snapshots; the package metadata map only drops user exclusions
//! such as `--no-optional` and `--no-runtime`.
//!
//! The output drives the **next** install's diff. Without this
//! filter, pacquet's current lockfile recorded every snapshot the
//! resolver imagined, including ones that `--no-optional` or
//! installability dropped — so a follow-up install would think
//! those slots were already on disk and skip work that should
//! actually run.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::Path,
};

use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_lockfile::{Lockfile, PackageKey, ProjectSnapshot, ResolvedDependencyMap};
use pacquet_modules_yaml::IncludedDependencies;

use crate::SkippedSnapshots;

pub struct MaterializationClosure {
    pub lockfile: Lockfile,
    pub importer_ids: HashSet<String>,
}

#[derive(Debug, Display, Error, Diagnostic)]
pub enum MergeFilteredWantedLockfileError {
    #[display("fresh lockfile is missing importer {importer_id}")]
    #[diagnostic(code(pacquet_package_manager::missing_fresh_lockfile_importer))]
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
        lockfile: Lockfile {
            lockfile_version: lockfile.lockfile_version,
            settings: lockfile.settings.clone(),
            catalogs: lockfile.catalogs.clone(),
            overrides: lockfile.overrides.clone(),
            package_extensions_checksum: lockfile.package_extensions_checksum.clone(),
            pnpmfile_checksum: lockfile.pnpmfile_checksum.clone(),
            ignored_optional_dependencies: lockfile.ignored_optional_dependencies.clone(),
            patched_dependencies: lockfile.patched_dependencies.clone(),
            importers,
            packages,
            snapshots,
        },
        importer_ids: reachable.importer_ids,
    }
}

pub fn merge_filtered_wanted_lockfile(
    previous_wanted: Option<&Lockfile>,
    mut freshly_resolved: Lockfile,
    real_importer_ids: &HashSet<String>,
    selected_importer_ids: &HashSet<String>,
    workspace_root: &Path,
) -> Result<Lockfile, MergeFilteredWantedLockfileError> {
    // Global resolution inputs describe every importer. If any of them
    // changed, retaining an old unselected importer would pair stale pins
    // with fresh catalogs, overrides, hooks, or lockfile settings.
    let can_reuse_unselected_importers = previous_wanted.is_some_and(|previous| {
        previous.lockfile_version == freshly_resolved.lockfile_version
            && previous.settings == freshly_resolved.settings
            && previous.catalogs == freshly_resolved.catalogs
            && previous.overrides == freshly_resolved.overrides
            && previous.package_extensions_checksum == freshly_resolved.package_extensions_checksum
            && previous.pnpmfile_checksum == freshly_resolved.pnpmfile_checksum
            && previous.ignored_optional_dependencies
                == freshly_resolved.ignored_optional_dependencies
            && previous.patched_dependencies == freshly_resolved.patched_dependencies
    });
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
    let final_importer_ids = freshly_resolved.importers.keys().cloned().collect();
    Ok(materialization_closure(
        &freshly_resolved,
        workspace_root,
        &final_importer_ids,
        all_dependencies(),
        &SkippedSnapshots::new(),
    )
    .lockfile)
}

pub(crate) fn merge_filtered_current_lockfile(
    previous_current: Option<&Lockfile>,
    wanted: &Lockfile,
    requested_importer_ids: &HashSet<String>,
    included: IncludedDependencies,
    skipped: &SkippedSnapshots,
    workspace_root: &Path,
) -> Lockfile {
    let selected =
        materialization_closure(wanted, workspace_root, requested_importer_ids, included, skipped);
    let Some(previous_current) = previous_current else {
        return selected.lockfile;
    };
    let retained_importers = previous_current
        .importers
        .iter()
        .filter(|(importer_id, _)| !selected.importer_ids.contains(*importer_id))
        .map(|(importer_id, importer)| (importer_id.clone(), importer.clone()))
        .collect::<HashMap<_, _>>();
    let retained_importer_ids = retained_importers.keys().cloned().collect();
    let retained_source = lockfile_with_graph(
        previous_current,
        retained_importers,
        previous_current.packages.clone(),
        previous_current.snapshots.clone(),
    );
    let retained = materialization_closure(
        &retained_source,
        workspace_root,
        &retained_importer_ids,
        all_dependencies(),
        &SkippedSnapshots::new(),
    );
    let mut importers = retained.lockfile.importers;
    importers.extend(selected.lockfile.importers);
    let selected_packages = selected.lockfile.packages;
    let packages =
        overlay_package_maps(previous_current.packages.as_ref(), selected_packages.clone());
    let snapshots =
        overlay_package_maps(retained.lockfile.snapshots.as_ref(), selected.lockfile.snapshots);
    let merged = lockfile_with_graph(wanted, importers, packages, snapshots);
    let final_importer_ids = merged.importers.keys().cloned().collect();
    let mut final_lockfile = materialization_closure(
        &merged,
        workspace_root,
        &final_importer_ids,
        all_dependencies(),
        &SkippedSnapshots::new(),
    )
    .lockfile;
    if let Some(selected_packages) = selected_packages {
        final_lockfile.packages.get_or_insert_default().extend(selected_packages);
    }
    if let (Some(merged_packages), Some(final_packages)) =
        (merged.packages.as_ref(), final_lockfile.packages.as_mut())
    {
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
    final_lockfile
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

fn lockfile_with_graph(
    source: &Lockfile,
    importers: HashMap<String, ProjectSnapshot>,
    packages: Option<HashMap<PackageKey, pacquet_lockfile::PackageMetadata>>,
    snapshots: Option<HashMap<PackageKey, pacquet_lockfile::SnapshotEntry>>,
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
    }
}

/// Build the "current lockfile" shape from the wanted lockfile by
/// applying the install-time `include` set and skip set.
///
/// Importers lose dep maps whose `include` flag is false; importer
/// `optionalDependencies` lose entries whose resolved snapshot got
/// skipped; the snapshot map is pruned to the transitive closure
/// reachable from the surviving importer roots. The package metadata
/// map preserves installability-skipped entries, matching pnpm's
/// current-lockfile shape while `.modules.yaml.skipped` records the
/// materialization skip.
#[must_use]
pub fn filter_lockfile_for_current(
    lockfile: &Lockfile,
    included: IncludedDependencies,
    skipped: &SkippedSnapshots,
) -> Lockfile {
    let all_importer_ids = lockfile.importers.keys().cloned().collect();
    materialization_closure(lockfile, Path::new(""), &all_importer_ids, included, skipped).lockfile
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

struct ReachableLockfileGraph {
    importer_ids: HashSet<String>,
    snapshot_keys: HashSet<PackageKey>,
}

fn collect_reachable<ShouldSkip>(
    lockfile: &Lockfile,
    workspace_root: &Path,
    initial_importer_ids: &HashSet<String>,
    included: IncludedDependencies,
    should_skip: ShouldSkip,
) -> ReachableLockfileGraph
where
    ShouldSkip: Fn(&PackageKey) -> bool,
{
    let snapshots = lockfile.snapshots.as_ref();
    let mut known_importer_ids = lockfile.importers.keys().cloned().collect::<Vec<_>>();
    known_importer_ids.sort();
    let known_importers = known_importer_ids
        .into_iter()
        .map(|id| {
            (pacquet_fs::lexical_normalize(&crate::importer_root_dir(workspace_root, &id)), id)
        })
        .collect::<HashMap<_, _>>();
    let mut importer_ids = HashSet::new();
    let mut snapshot_keys = HashSet::new();
    let mut importer_queue = initial_importer_ids.iter().cloned().collect::<VecDeque<_>>();
    let mut snapshot_queue = VecDeque::new();

    while !importer_queue.is_empty() || !snapshot_queue.is_empty() {
        while let Some(importer_id) = importer_queue.pop_front() {
            if importer_ids.contains(&importer_id) {
                continue;
            }
            let Some(importer) = lockfile.importers.get(&importer_id) else { continue };
            importer_ids.insert(importer_id.clone());
            let importer_dir = crate::importer_root_dir(workspace_root, &importer_id);
            for map in [
                included.dependencies.then_some(importer.dependencies.as_ref()).flatten(),
                included.dev_dependencies.then_some(importer.dev_dependencies.as_ref()).flatten(),
                included
                    .optional_dependencies
                    .then_some(importer.optional_dependencies.as_ref())
                    .flatten(),
            ]
            .into_iter()
            .flatten()
            {
                for (name, spec) in map {
                    if let Some(target) = spec.version.as_link_target() {
                        enqueue_linked_importer(
                            &importer_dir,
                            target,
                            &known_importers,
                            &mut importer_queue,
                        );
                    } else if let Some(key) = spec.version.resolved_key(name)
                        && !should_skip(&key)
                        && snapshots.is_some_and(|snapshots| snapshots.contains_key(&key))
                    {
                        snapshot_queue.push_back(key);
                    }
                }
            }
        }

        while let Some(key) = snapshot_queue.pop_front() {
            if !snapshot_keys.insert(key.clone()) {
                continue;
            }
            let Some(snapshot) = snapshots.and_then(|snapshots| snapshots.get(&key)) else {
                continue;
            };
            for map in [
                snapshot.dependencies.as_ref(),
                included
                    .optional_dependencies
                    .then_some(snapshot.optional_dependencies.as_ref())
                    .flatten(),
            ]
            .into_iter()
            .flatten()
            {
                for (alias, dep_ref) in map {
                    if let Some(target) = dep_ref.as_link_target() {
                        enqueue_linked_importer(
                            workspace_root,
                            target,
                            &known_importers,
                            &mut importer_queue,
                        );
                    } else if let Some(child) = dep_ref.resolve(alias)
                        && !should_skip(&child)
                        && snapshots.is_some_and(|snapshots| snapshots.contains_key(&child))
                    {
                        snapshot_queue.push_back(child);
                    }
                }
            }
        }
    }

    ReachableLockfileGraph { importer_ids, snapshot_keys }
}

fn enqueue_linked_importer(
    base: &Path,
    target: &str,
    known_importers: &HashMap<std::path::PathBuf, String>,
    importer_queue: &mut VecDeque<String>,
) {
    if let Some(importer_id) = linked_importer_id(base, target, known_importers) {
        importer_queue.push_back(importer_id);
    }
}

fn linked_importer_id(
    base: &Path,
    target: &str,
    known_importers: &HashMap<std::path::PathBuf, String>,
) -> Option<String> {
    let target = Path::new(target);
    let resolved = if target.is_absolute() {
        pacquet_fs::lexical_normalize(target)
    } else {
        pacquet_fs::lexical_normalize(&base.join(target))
    };
    known_importers.get(&resolved).cloned()
}

#[cfg(test)]
mod tests;
