//! Decide which snapshots this install must materialize, pairing each
//! with the store-index cache key [`super::CasPrefetch::start`]
//! derived for it.

mod children;
use children::{EntryKind, optional_children_match, probe_slot_entry, regular_children_match};

use super::{
    CreateVirtualStoreError, SnapshotCacheKey, SnapshotWithCacheKey, gvs_slot_needs_rebuild,
    integrity_equal, snapshot_deps_equal,
};
use crate::{SkippedSnapshots, VirtualStoreLayout};
use pnpm_lockfile::{
    LockfileEntries, LockfileResolution, PackageKey, PackageMetadata, SnapshotEntry,
};
use pnpm_reporter::{BrokenModulesLog, LogEvent, LogLevel, Reporter};
use std::collections::{HashMap, HashSet};

/// `'a` is the lifetime of the lockfile maps the resulting
/// [`SnapshotPlan`] borrows from; `'b` covers the inputs the plan pass
/// only reads while running.
pub(super) struct SnapshotPlanInputs<'a, 'b> {
    pub snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
    pub packages: &'a HashMap<PackageKey, PackageMetadata>,
    /// What the previous install materialized. Empty on a first
    /// install; when present, a snapshot whose wiring and integrity are
    /// unchanged *and* whose slot is still on disk is left alone.
    pub current_entries: LockfileEntries<'b>,
    pub layout: &'b VirtualStoreLayout,
    pub allow_build_policy: &'b crate::AllowBuildPolicy,
    /// Snapshots the installability pass ruled out on this host.
    pub skipped: &'b SkippedSnapshots,
    pub link_dependencies: bool,
    /// `--force` re-materializes every slot, so both skip paths — the
    /// current-lockfile comparison and the global-virtual-store
    /// existence probe — are disabled here, whether or not the caller
    /// also emptied `current_entries`.
    pub force: bool,
    pub is_hoisted: bool,
    pub include_optional_dependencies: bool,
    /// One derivation `Result` per lockfile snapshot, taken by the
    /// entry that keeps it. See [`super::CasPrefetch::start`], which
    /// guarantees the every-snapshot coverage.
    pub cache_keys: &'b mut HashMap<PackageKey, Result<SnapshotCacheKey, CreateVirtualStoreError>>,
}

/// The snapshots to install, the ones deliberately left alone, and the
/// slots whose global-virtual-store build marker forces a rebuild.
pub(super) struct SnapshotPlan<'a> {
    /// Snapshots this install materializes.
    pub survivors: Vec<SnapshotWithCacheKey<'a>>,
    /// Snapshots the current-lockfile check or the global-virtual-store
    /// existence probe skipped. They contribute no link work, but their
    /// store-index rows still feed the build phase's `is_built` gate,
    /// so a warm reinstall does not re-run approved build scripts.
    pub skipped_entries: Vec<SnapshotWithCacheKey<'a>>,
    pub marker_rebuilds: HashSet<PackageKey>,
    pub has_git_hosted_survivor: bool,
}

impl SnapshotPlan<'_> {
    pub(super) fn materialized_keys(&self) -> Vec<PackageKey> {
        self.survivors.iter().map(|(snapshot_key, _, _)| (*snapshot_key).clone()).collect()
    }
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
pub(super) fn plan_snapshots<'a, Reporter: self::Reporter>(
    inputs: SnapshotPlanInputs<'a, '_>,
) -> Result<SnapshotPlan<'a>, CreateVirtualStoreError> {
    let probe = WarmSlotProbe::of(&inputs);
    let SnapshotPlanInputs { snapshots, cache_keys, .. } = inputs;
    let mut markers = MarkerProbes::default();
    let (survivors, has_git_hosted_survivor) =
        survivors::<Reporter>(snapshots, &probe, &mut markers, cache_keys)?;
    let marker_rebuilds = marker_rebuilds(markers, &survivors, &probe);
    let skipped_entries = skipped_entries(snapshots, &survivors, probe.skipped, cache_keys);
    Ok(SnapshotPlan { survivors, skipped_entries, marker_rebuilds, has_git_hosted_survivor })
}

/// The snapshots this install materializes, and whether any of them is
/// git-hosted.
///
/// The slot probe goes through `layout.slot_dir` because under GVS the
/// slot lives at `<global_virtual_store_dir>/...`, and probing
/// `<virtual_store_dir>/<flat-name>` would find nothing and report
/// every warm slot as broken.
fn survivors<'a, Reporter: self::Reporter>(
    snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
    probe: &WarmSlotProbe<'a, '_>,
    markers: &mut MarkerProbes,
    cache_keys: &mut HashMap<PackageKey, Result<SnapshotCacheKey, CreateVirtualStoreError>>,
) -> Result<(Vec<SnapshotWithCacheKey<'a>>, bool), CreateVirtualStoreError> {
    let mut has_git_hosted_survivor = false;
    let entries = snapshots
        .iter()
        // Reason 1: installability skip. Drop entirely.
        .filter(|(snapshot_key, _)| !probe.skipped.contains(snapshot_key))
        // Reason 2: warm-slot skip. Drop survivors that already match
        // the previous install, or whose content-addressed global-
        // virtual-store slot already exists. This is a fallible fold
        // because a warm-slot lstat error must abort the install rather
        // than quietly converting the slot into a rebuild on every run.
        .try_fold(Vec::new(), |mut entries, (snapshot_key, snapshot)| {
            if !warm_slot_is_current::<Reporter>(probe, snapshot_key, snapshot, markers)? {
                let cache_key = cache_keys
                    .remove(snapshot_key)
                    .expect("CasPrefetch::start derived a cache key for every lockfile snapshot")?;
                has_git_hosted_survivor |= cache_key.is_git_hosted;
                entries.push((snapshot_key, snapshot, cache_key.value));
            }
            Ok::<_, CreateVirtualStoreError>(entries)
        })?;
    Ok((entries, has_git_hosted_survivor))
}

/// The probe's own rebuilds, joined by the survivors it never reached
/// whose global-virtual-store build marker forces one anyway.
fn marker_rebuilds(
    markers: MarkerProbes,
    survivors: &[SnapshotWithCacheKey<'_>],
    probe: &WarmSlotProbe<'_, '_>,
) -> HashSet<PackageKey> {
    let MarkerProbes { keys: probed, mut rebuilds } = markers;
    if !probe.is_hoisted {
        rebuilds.extend(
            survivors
                .iter()
                .filter(|(snapshot_key, _, _)| !probed.contains(*snapshot_key))
                .filter(|(snapshot_key, _, _)| {
                    gvs_slot_needs_rebuild(probe.layout, probe.allow_build_policy, snapshot_key)
                })
                .map(|(snapshot_key, _, _)| (*snapshot_key).clone()),
        );
    }
    rebuilds
}

/// The snapshots the warm-slot probe left alone, with the lenient
/// cache-key pass. Installability-skipped snapshots are excluded: they
/// were never installed, so there is no store-index row to keep warm
/// for the build-cache lookup.
fn skipped_entries<'a>(
    snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
    survivors: &[SnapshotWithCacheKey<'_>],
    skipped: &SkippedSnapshots,
    cache_keys: &mut HashMap<PackageKey, Result<SnapshotCacheKey, CreateVirtualStoreError>>,
) -> Vec<SnapshotWithCacheKey<'a>> {
    // A parallel `Vec` rather than a filter later: the partition's
    // manifest and side-effects loop has to see the full snapshot set,
    // not just survivors.
    let survivor_keys: HashSet<&PackageKey> = survivors.iter().map(|(key, _, _)| *key).collect();
    snapshots
        .iter()
        .filter(|(snapshot_key, _)| !survivor_keys.contains(snapshot_key))
        .filter(|(snapshot_key, _)| !skipped.contains(snapshot_key))
        .map(|(snapshot_key, snapshot)| {
            let cache_key = cache_keys
                .remove(snapshot_key)
                .and_then(Result::ok)
                .and_then(|cache_key| cache_key.value);
            (snapshot_key, snapshot, cache_key)
        })
        .collect()
}

/// The lockfile-independent inputs of the warm-slot probe: everything
/// [`plan_snapshots`] reads to decide whether a snapshot's virtual-store
/// slot may be left alone.
struct WarmSlotProbe<'a, 'b> {
    packages: &'a HashMap<PackageKey, PackageMetadata>,
    current_entries: LockfileEntries<'b>,
    layout: &'b VirtualStoreLayout,
    allow_build_policy: &'b crate::AllowBuildPolicy,
    skipped: &'b SkippedSnapshots,
    link_dependencies: bool,
    force: bool,
    is_hoisted: bool,
    include_optional_dependencies: bool,
}

impl<'a, 'b> WarmSlotProbe<'a, 'b> {
    fn of(inputs: &SnapshotPlanInputs<'a, 'b>) -> Self {
        Self {
            packages: inputs.packages,
            current_entries: inputs.current_entries,
            layout: inputs.layout,
            allow_build_policy: inputs.allow_build_policy,
            skipped: inputs.skipped,
            link_dependencies: inputs.link_dependencies,
            force: inputs.force,
            is_hoisted: inputs.is_hoisted,
            include_optional_dependencies: inputs.include_optional_dependencies,
        }
    }
}

/// Slots the warm-slot probe reached a verdict on, split by what the
/// build-marker rescan after the probe still owes them.
#[derive(Default)]
struct MarkerProbes {
    /// Slots whose build marker the probe already accounted for, so the
    /// rescan need not stat under them again.
    keys: HashSet<PackageKey>,
    /// Warm slots whose build marker forces a rebuild anyway.
    rebuilds: HashSet<PackageKey>,
}

/// Whether the snapshot's virtual-store slot already holds what this
/// install would materialize, so the install may skip it.
///
/// Records the snapshot in `markers` whenever the probe settled its
/// build-marker state.
fn warm_slot_is_current<Reporter: self::Reporter>(
    probe: &WarmSlotProbe<'_, '_>,
    snapshot_key: &PackageKey,
    snapshot: &SnapshotEntry,
    markers: &mut MarkerProbes,
) -> Result<bool, CreateVirtualStoreError> {
    if !slot_probe_applies(probe, snapshot_key) {
        return Ok(false);
    }
    let current_entry_unchanged = current_entry_unchanged(probe, snapshot_key, snapshot);
    // A global-virtual-store slot path is content-addressed: the graph
    // hash covers the snapshot's wiring, integrity, and engine, so an
    // existing slot is current even when no current lockfile survives —
    // a wiped `node_modules` takes `<virtual_store_dir>/lock.yaml` with
    // it, and without this probe such a restore re-links every slot the
    // store already holds (pnpm/pnpm#14510). Mirrors the GVS fast path
    // in pnpm's `lockfileToDepGraph`.
    let gvs_slot_is_authoritative = probe.layout.enable_global_virtual_store() && !probe.force;
    if !current_entry_unchanged && !gvs_slot_is_authoritative {
        return Ok(false);
    }
    if !slot_contents_complete::<Reporter>(
        probe,
        snapshot_key,
        snapshot,
        current_entry_unchanged,
        markers,
    )? {
        return Ok(false);
    }
    let needs_rebuild =
        gvs_slot_needs_rebuild(probe.layout, probe.allow_build_policy, snapshot_key);
    markers.keys.insert(snapshot_key.clone());
    if needs_rebuild {
        markers.rebuilds.insert(snapshot_key.clone());
    }
    Ok(!needs_rebuild)
}

/// Whether a slot probe can judge this snapshot at all. The hoisted
/// linker writes no virtual-store slot (pnpm/pnpm#14001), and a `file:`
/// dependency's source is mutable, so for those neither an unchanged
/// lockfile nor an existing slot is evidence the copy is current.
fn slot_probe_applies(probe: &WarmSlotProbe<'_, '_>, snapshot_key: &PackageKey) -> bool {
    !probe.is_hoisted
        && !matches!(
            probe.packages.get(&snapshot_key.without_peer()).map(|meta| &meta.resolution),
            Some(LockfileResolution::Directory(_)),
        )
}

/// Whether the current lockfile records this snapshot with the same
/// wiring and integrity the install is about to write.
fn current_entry_unchanged(
    probe: &WarmSlotProbe<'_, '_>,
    snapshot_key: &PackageKey,
    snapshot: &SnapshotEntry,
) -> bool {
    !probe.force
        && probe
            .current_entries
            .snapshots
            .and_then(|current_snapshots| current_snapshots.get(snapshot_key))
            .is_some_and(|current_snapshot| {
                snapshot_deps_equal(current_snapshot, snapshot)
                    && integrity_equal(
                        probe
                            .current_entries
                            .packages
                            .and_then(|packages| packages.get(&snapshot_key.without_peer())),
                        probe.packages.get(&snapshot_key.without_peer()),
                    )
            })
}

/// Whether the slot on disk holds a finished import of the snapshot,
/// child links included.
///
/// The slot probe goes through [`VirtualStoreLayout::slot_dir`] because
/// under GVS the slot lives at `<global_virtual_store_dir>/...`, and
/// probing `<virtual_store_dir>/<flat-name>` would find nothing and
/// report every warm slot as broken. See pnpm/pacquet#442 for why the
/// current-lockfile skip keeps its store-index rows.
fn slot_contents_complete<Reporter: self::Reporter>(
    probe: &WarmSlotProbe<'_, '_>,
    snapshot_key: &PackageKey,
    snapshot: &SnapshotEntry,
    current_entry_unchanged: bool,
    markers: &mut MarkerProbes,
) -> Result<bool, CreateVirtualStoreError> {
    let dir = probe
        .layout
        .slot_dir(snapshot_key)
        .join("node_modules")
        .join(snapshot_key.name.to_string());
    if !probe_slot_entry(&dir, EntryKind::Dir)? {
        // Only a slot the current lockfile vouches for is "broken" when
        // missing; a mere GVS-existence miss is a fresh materialization.
        if current_entry_unchanged {
            Reporter::emit(&LogEvent::BrokenModules(BrokenModulesLog {
                level: LogLevel::Debug,
                missing: dir.to_string_lossy().into_owned(),
            }));
        }
        // A missing slot has no build marker either.
        markers.keys.insert(snapshot_key.clone());
        return Ok(false);
    }
    // The importer populates shared GVS slots in place, so an existing
    // directory may be an import another install is still filling or
    // died halfway through (see `import_into_shared_dir`). Without a
    // current-lockfile record vouching that a previous install completed
    // the slot, require the importer's own completion invariant —
    // pnpm's `pkgExistsAtTargetDir` probes `package.json`, which the
    // import places last. A rare package whose file map lacks
    // `package.json` merely re-materializes, and the import then
    // short-circuits on its actual marker.
    if !current_entry_unchanged && !probe_slot_entry(&dir.join("package.json"), EntryKind::File)? {
        return Ok(false);
    }
    if !optional_children_match(
        snapshot_key,
        snapshot,
        probe.layout,
        probe.skipped,
        probe.link_dependencies,
        probe.include_optional_dependencies,
    )? {
        return Ok(false);
    }
    // The completion marker only covers the file import: the slot's
    // child symlinks are written concurrently with it (`rayon::join` in
    // `CreateVirtualDirBySnapshot::run`), so a crash can leave a
    // marker-complete slot with links missing. A current-lockfile record
    // is only written by a completed install and so vouches for the
    // links too; without one, probe every child the symlink layout would
    // have created.
    if current_entry_unchanged {
        return Ok(true);
    }
    regular_children_match(
        snapshot_key,
        snapshot,
        probe.layout,
        probe.skipped,
        probe.link_dependencies,
    )
}

#[cfg(test)]
mod tests;
