//! Decide which snapshots this install must materialize, and derive the
//! store-index cache key each one is looked up by.
//!
//! Runs before the prefetch, whose input is the cache keys produced
//! here.

use super::{
    CreateVirtualStoreError, SnapshotWithCacheKey, gvs_slot_needs_rebuild, integrity_equal,
    snapshot_cache_key, snapshot_deps_equal,
};
use crate::{SkippedSnapshots, VirtualStoreLayout};
use pacquet_lockfile::{
    LockfileResolution, PackageKey, PackageMetadata, PlatformSelector, SnapshotEntry,
};
use pacquet_reporter::{BrokenModulesLog, LogEvent, LogLevel, Reporter};
use std::collections::{HashMap, HashSet};

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
    pub ignore_scripts: bool,
    pub runtime_platform_selector: &'a PlatformSelector,
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
}

/// Partition the lockfile's snapshots into what this install must do
/// and what it may leave alone.
///
/// Validation is deliberately asymmetric: survivors go through the
/// strict cache-key derivation, because the install will actually fetch
/// and link them, so a malformed resolution must fail before the warm
/// batch starts rather than several seconds into it. Skipped snapshots
/// get a lenient pass — they are not being installed, and swallowing a
/// per-snapshot error there costs only a prefetch row.
pub(super) fn plan_snapshots<'a, Reporter: self::Reporter>(
    inputs: &SnapshotPlanInputs<'a>,
) -> Result<SnapshotPlan<'a>, CreateVirtualStoreError> {
    let &SnapshotPlanInputs {
        snapshots,
        packages,
        current_snapshots,
        current_packages,
        layout,
        allow_build_policy,
        skipped,
        ignore_scripts,
        runtime_platform_selector,
    } = inputs;

    // Per-snapshot skip pass: drop snapshots that don't need
    // installing.
    //
    // Two reasons a snapshot can be dropped from the install graph:
    //
    // 1. **Installability skip (this PR)** — `SkippedSnapshots`
    //    contains it because the host's `engines` / `cpu` / `os`
    //    / `libc` don't satisfy the package's constraints and the
    //    snapshot is `optional`. A skipped depPath is dropped from
    //    the install graph. These snapshots also stay out of the
    //    `skipped_entries` cache-key pass — they were never
    //    supposed to be installed, so there are no store-index
    //    rows to keep alive.
    //
    // 2. **Current-lockfile skip (main <https://github.com/pnpm/pacquet/pull/442>)** — the previous
    //    install also installed this snapshot (`current_snapshots`)
    //    with the same dependency wiring + integrity, AND its
    //    virtual-store slot still exists on disk. These DO land in
    //    `skipped_entries` so `BuildModules`'s `is_built` cache
    //    lookup can short-circuit re-runs of allowed-build scripts
    //    on warm reinstalls.
    //
    // Run this *before* deriving cache keys so unchanged
    // directory-backed snapshots aren't tripped by
    // `snapshot_cache_key`'s `UnsupportedResolution`.
    //
    // Route the slot-existence probe through `layout.slot_dir` so
    // GVS-on installs check the correct path. Under GVS, slots live
    // at `<global_virtual_store_dir>/<scope>/<name>/<ver>/<hash>`,
    // not `<config.virtual_store_dir>/<flat-name>`; probing the
    // latter would find an empty path, so the skip gate would
    // incorrectly mark every warm slot as "broken" and emit
    // `BrokenModules` for the wrong path.
    let mut marker_probe_keys = HashSet::new();
    let mut marker_rebuilds = HashSet::new();
    let survivors = snapshots
        .iter()
        // Reason 1: installability skip. Drop entirely.
        .filter(|(snapshot_key, _)| !skipped.contains(snapshot_key))
        // Reason 2: current-lockfile skip. Drop survivors that
        // already match the previous install.
        .filter(|(snapshot_key, snapshot)| {
            let Some(current_snapshots) = current_snapshots else { return true };
            let Some(current_snapshot) = current_snapshots.get(*snapshot_key) else {
                return true;
            };
            let wanted_metadata = packages.get(&snapshot_key.without_peer());
            // Directory-typed snapshots carry mutable local
            // source: the user can edit `file:./local-pkg` files
            // between installs and pacquet must re-walk them on
            // every install, otherwise the slot drifts. Directory
            // snapshots are forced through the cold path for this
            // reason. Without this carve-out
            // both `current` and `wanted` resolutions report
            // `integrity() == None`, `integrity_equal` returns
            // true, the slot directory check passes, and the
            // directory-fetcher never runs on the second install.
            if matches!(
                wanted_metadata.map(|meta| &meta.resolution),
                Some(LockfileResolution::Directory(_)),
            ) {
                return true;
            }
            if !snapshot_deps_equal(current_snapshot, snapshot) {
                return true;
            }
            let current_metadata =
                current_packages.and_then(|p| p.get(&snapshot_key.without_peer()));
            if !integrity_equal(current_metadata, wanted_metadata) {
                return true;
            }
            let dir = layout
                .slot_dir(snapshot_key)
                .join("node_modules")
                .join(snapshot_key.name.to_string());
            if dir.is_dir() {
                let needs_rebuild =
                    gvs_slot_needs_rebuild(layout, allow_build_policy, snapshot_key);
                marker_probe_keys.insert((*snapshot_key).clone());
                if needs_rebuild {
                    marker_rebuilds.insert((*snapshot_key).clone());
                }
                needs_rebuild
            } else {
                Reporter::emit(&LogEvent::BrokenModules(BrokenModulesLog {
                    level: LogLevel::Debug,
                    missing: dir.to_string_lossy().into_owned(),
                }));
                true
            }
        });
    // Validate every surviving snapshot upfront so a malformed
    // lockfile (missing metadata, missing tarball integrity,
    // currently-unsupported directory / git resolution) errors
    // out *before* we start the warm batch — otherwise the warm
    // rayon batch runs to completion (~6 s on `alot7`) before the
    // actual error fires.
    //
    // Cache-key derivation runs in two passes:
    //
    // - *Survivors* go through the strict path (this `?`). Their
    //   resolutions have to be valid because the install will
    //   actually fetch + link them.
    // - *Skipped* snapshots get a lenient pass below: cache keys
    //   are derived if possible, and any per-snapshot error is
    //   swallowed. Reason: skipped snapshots aren't being
    //   re-installed, but their store-index rows still need to
    //   land in `side_effects_maps_by_snapshot` so
    //   [`crate::BuildModules`]'s `is_built` gate can skip
    //   re-running build scripts on warm reinstalls (review on
    //   <https://github.com/pnpm/pacquet/pull/442> — without this, allowed-build packages re-execute
    //   their scripts every install, costing seconds on the
    //   warm-reinstall path).
    let snapshot_entries: Vec<SnapshotWithCacheKey<'_>> = survivors
        .map(|(snapshot_key, snapshot)| {
            snapshot_cache_key(snapshot_key, packages, ignore_scripts, runtime_platform_selector)
                .map(|key| (snapshot_key, snapshot, key))
        })
        .collect::<Result<_, _>>()?;
    marker_rebuilds.extend(
        snapshot_entries
            .iter()
            .filter(|(snapshot_key, _, _)| !marker_probe_keys.contains(*snapshot_key))
            .filter(|(snapshot_key, _, _)| {
                gvs_slot_needs_rebuild(layout, allow_build_policy, snapshot_key)
            })
            .map(|(snapshot_key, _, _)| (*snapshot_key).clone()),
    );

    // Cache keys for the *skipped* snapshots (i.e. snapshots
    // present in `snapshots` but absent from `snapshot_entries`).
    // Derived leniently so an unsupported / malformed skipped
    // entry doesn't fail the install — it just contributes no
    // prefetch row, which is the same outcome as if the skip
    // filter had not engaged. Built as a parallel `Vec` so the
    // downstream `package_manifests` /
    // `side_effects_maps_by_snapshot` loop sees the full snapshot
    // set, not just survivors.
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
            let cache_key = snapshot_cache_key(
                snapshot_key,
                packages,
                ignore_scripts,
                runtime_platform_selector,
            )
            .ok()
            .flatten();
            (snapshot_key, snapshot, cache_key)
        })
        .collect();
    Ok(SnapshotPlan { survivors: snapshot_entries, skipped_entries, marker_rebuilds })
}
