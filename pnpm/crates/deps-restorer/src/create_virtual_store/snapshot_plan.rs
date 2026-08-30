//! Decide which snapshots this install must materialize, pairing each
//! with the store-index cache key [`super::CasPrefetch::start`]
//! derived for it.

use super::{
    CreateVirtualStoreError, SnapshotCacheKey, SnapshotWithCacheKey, gvs_slot_needs_rebuild,
    integrity_equal, snapshot_deps_equal,
};
use crate::{SkippedSnapshots, VirtualStoreLayout};
use pnpm_lockfile::{LockfileResolution, PackageKey, PackageMetadata, SnapshotEntry};
use pnpm_reporter::{BrokenModulesLog, LogEvent, LogLevel, Reporter};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

pub(super) struct SnapshotPlanInputs<'a> {
    pub snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
    pub packages: &'a HashMap<PackageKey, PackageMetadata>,
    /// What the previous install materialized. `None` on a first
    /// install; when present, a snapshot whose wiring and integrity are
    /// unchanged *and* whose slot is still on disk is left alone.
    pub current_snapshots: Option<&'a HashMap<PackageKey, SnapshotEntry>>,
    pub current_packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    pub layout: &'a VirtualStoreLayout,
    pub allow_build_policy: &'a crate::AllowBuildPolicy,
    /// Snapshots the installability pass ruled out on this host.
    pub skipped: &'a SkippedSnapshots,
    pub link_dependencies: bool,
    pub is_hoisted: bool,
    pub include_optional_dependencies: bool,
    /// One derivation `Result` per lockfile snapshot, taken by the
    /// entry that keeps it. See [`super::CasPrefetch::start`], which
    /// guarantees the every-snapshot coverage.
    pub cache_keys: &'a mut HashMap<PackageKey, Result<SnapshotCacheKey, CreateVirtualStoreError>>,
}

/// The snapshots to install, the ones deliberately left alone, and the
/// slots whose global-virtual-store build marker forces a rebuild.
pub(super) struct SnapshotPlan<'a> {
    /// Snapshots this install materializes.
    pub survivors: Vec<SnapshotWithCacheKey<'a>>,
    /// Snapshots the current-lockfile check skipped. They contribute no
    /// link work, but their store-index rows still feed the build
    /// phase's `is_built` gate, so a warm reinstall does not re-run
    /// approved build scripts.
    pub skipped_entries: Vec<SnapshotWithCacheKey<'a>>,
    pub marker_rebuilds: HashSet<PackageKey>,
    pub has_git_hosted_survivor: bool,
}

/// Partition the lockfile's snapshots into what this install must do
/// and what it may leave alone.
///
/// Validation is deliberately asymmetric: survivors keep the strict
/// cache-key derivation `Result`, because the install will actually
/// fetch and link them, so a malformed resolution must fail before the
/// warm batch starts rather than several seconds into it. Skipped
/// snapshots get a lenient pass — they are not being installed, and
/// swallowing a per-snapshot error there costs only a prefetch row.
pub(super) fn plan_snapshots<Reporter: self::Reporter>(
    inputs: SnapshotPlanInputs<'_>,
) -> Result<SnapshotPlan<'_>, CreateVirtualStoreError> {
    let SnapshotPlanInputs {
        snapshots,
        packages,
        current_snapshots,
        current_packages,
        layout,
        allow_build_policy,
        skipped,
        link_dependencies,
        is_hoisted,
        include_optional_dependencies,
        cache_keys,
    } = inputs;

    // The slot probe goes through `layout.slot_dir` because under GVS
    // the slot lives at `<global_virtual_store_dir>/...`, and probing
    // `<virtual_store_dir>/<flat-name>` would find nothing and report
    // every warm slot as broken. See pnpm/pacquet#442 for why the
    // current-lockfile skip keeps its store-index rows.
    let mut marker_probe_keys = HashSet::new();
    let mut marker_rebuilds = HashSet::new();
    let mut has_git_hosted_survivor = false;
    let snapshot_entries = snapshots
        .iter()
        // Reason 1: installability skip. Drop entirely.
        .filter(|(snapshot_key, _)| !skipped.contains(snapshot_key))
        // Reason 2: current-lockfile skip. Drop survivors that already
        // match the previous install. This is a fallible fold because a
        // warm-slot lstat error must abort the install rather than quietly
        // converting the slot into a rebuild on every run.
        .try_fold(Vec::new(), |mut entries, (snapshot_key, snapshot)| {
            let current_slot_matches = (|| -> Result<bool, CreateVirtualStoreError> {
                // The hoisted linker writes no virtual-store slot, so
                // this probe cannot judge it (pnpm/pnpm#14001).
                if is_hoisted {
                    return Ok(false);
                }
                let Some(current_snapshots) = current_snapshots else { return Ok(false) };
                let Some(current_snapshot) = current_snapshots.get(snapshot_key) else {
                    return Ok(false);
                };
                let wanted_metadata = packages.get(&snapshot_key.without_peer());
                // A `file:` dependency's source is mutable, so an
                // unchanged lockfile is no evidence its slot is current.
                if matches!(
                    wanted_metadata.map(|meta| &meta.resolution),
                    Some(LockfileResolution::Directory(_)),
                ) {
                    return Ok(false);
                }
                if !snapshot_deps_equal(current_snapshot, snapshot) {
                    return Ok(false);
                }
                let current_metadata =
                    current_packages.and_then(|p| p.get(&snapshot_key.without_peer()));
                if !integrity_equal(current_metadata, wanted_metadata) {
                    return Ok(false);
                }
                let dir = layout
                    .slot_dir(snapshot_key)
                    .join("node_modules")
                    .join(snapshot_key.name.to_string());
                if !dir.is_dir() {
                    Reporter::emit(&LogEvent::BrokenModules(BrokenModulesLog {
                        level: LogLevel::Debug,
                        missing: dir.to_string_lossy().into_owned(),
                    }));
                    return Ok(false);
                }
                if !optional_children_match(
                    snapshot_key,
                    snapshot,
                    layout,
                    skipped,
                    link_dependencies,
                    include_optional_dependencies,
                )? {
                    return Ok(false);
                }
                let needs_rebuild =
                    gvs_slot_needs_rebuild(layout, allow_build_policy, snapshot_key);
                marker_probe_keys.insert(snapshot_key.clone());
                if needs_rebuild {
                    marker_rebuilds.insert(snapshot_key.clone());
                }
                Ok(!needs_rebuild)
            })()?;
            if !current_slot_matches {
                let cache_key = cache_keys
                    .remove(snapshot_key)
                    .expect("CasPrefetch::start derived a cache key for every lockfile snapshot")?;
                has_git_hosted_survivor |= cache_key.is_git_hosted;
                entries.push((snapshot_key, snapshot, cache_key.value));
            }
            Ok::<_, CreateVirtualStoreError>(entries)
        })?;
    if !is_hoisted {
        marker_rebuilds.extend(
            snapshot_entries
                .iter()
                .filter(|(snapshot_key, _, _)| !marker_probe_keys.contains(*snapshot_key))
                .filter(|(snapshot_key, _, _)| {
                    gvs_slot_needs_rebuild(layout, allow_build_policy, snapshot_key)
                })
                .map(|(snapshot_key, _, _)| (*snapshot_key).clone()),
        );
    }

    // A parallel `Vec` rather than a filter later: the partition's
    // manifest and side-effects loop has to see the full snapshot set,
    // not just survivors.
    let survivor_keys: std::collections::HashSet<&PackageKey> =
        snapshot_entries.iter().map(|(k, _, _)| *k).collect();
    let skipped_entries: Vec<SnapshotWithCacheKey<'_>> = snapshots
        .iter()
        .filter(|(snapshot_key, _)| !survivor_keys.contains(snapshot_key))
        // Installability-skipped snapshots are excluded from
        // `skipped_entries` too — they were never installed, so
        // there's no store-index row to keep warm for the
        // build-cache lookup. Only the current-lockfile-skip
        // path (`snapshot_entries` filtered above) should contribute
        // here.
        .filter(|(snapshot_key, _)| !skipped.contains(snapshot_key))
        .map(|(snapshot_key, snapshot)| {
            let cache_key = cache_keys
                .remove(snapshot_key)
                .and_then(Result::ok)
                .and_then(|cache_key| cache_key.value);
            (snapshot_key, snapshot, cache_key)
        })
        .collect();
    Ok(SnapshotPlan {
        survivors: snapshot_entries,
        skipped_entries,
        marker_rebuilds,
        has_git_hosted_survivor,
    })
}

fn optional_children_match(
    snapshot_key: &PackageKey,
    snapshot: &SnapshotEntry,
    layout: &VirtualStoreLayout,
    skipped: &SkippedSnapshots,
    link_dependencies: bool,
    include_optional_dependencies: bool,
) -> Result<bool, CreateVirtualStoreError> {
    optional_children_match_with(
        snapshot_key,
        snapshot,
        layout,
        skipped,
        link_dependencies,
        include_optional_dependencies,
        optional_child_matches,
    )
}

fn optional_child_matches(child_path: &Path, should_exist: bool) -> std::io::Result<bool> {
    if should_exist {
        return match std::fs::metadata(child_path) {
            Ok(metadata) => Ok(metadata.is_dir()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        };
    }
    match std::fs::symlink_metadata(child_path) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error),
    }
}

fn optional_children_match_with(
    snapshot_key: &PackageKey,
    snapshot: &SnapshotEntry,
    layout: &VirtualStoreLayout,
    skipped: &SkippedSnapshots,
    link_dependencies: bool,
    include_optional_dependencies: bool,
    mut child_matches: impl FnMut(&Path, bool) -> std::io::Result<bool>,
) -> Result<bool, CreateVirtualStoreError> {
    let Some(optional_dependencies) = snapshot.optional_dependencies.as_ref() else {
        return Ok(true);
    };
    let modules_dir = layout.slot_dir(snapshot_key).join("node_modules");
    for (alias, dep_ref) in optional_dependencies {
        if alias == &snapshot_key.name {
            continue;
        }
        let Ok(child_path) =
            crate::safe_join_modules_dir::safe_join_modules_dir(&modules_dir, &alias.to_string())
        else {
            return Ok(false);
        };
        let should_exist = link_dependencies
            && include_optional_dependencies
            && if let Some(target) = dep_ref.resolve(alias) {
                !skipped.contains(&target)
            } else {
                dep_ref.as_link_target().is_some() && layout.lockfile_dir().is_some()
            };
        let matches = child_matches(&child_path, should_exist).map_err(|error| {
            CreateVirtualStoreError::InspectOptionalDependency { path: child_path.clone(), error }
        })?;
        if !matches {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests;
