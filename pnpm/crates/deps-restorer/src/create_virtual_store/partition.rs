//! Split the snapshots this install materializes into the warm batch —
//! already in the CAFS, so the prefetch covered them — and the cold
//! batch that still needs downloading, and collect the store-index rows
//! later phases read.
//!
//! Runs immediately after the prefetch, whose results it consumes.

use super::{
    PackageManifests, RemoteSideEffectsQuarantineBySnapshot, RequiresBuildBySnapshot,
    SideEffectsBySnapshot, SideEffectsMapsBySnapshot, SnapshotWithCacheKey,
    StoreIndexKeysBySnapshot, snapshot_needs_build_marker,
};
use pnpm_config::NodeLinker;
use pnpm_lockfile::{PackageKey, SnapshotEntry};
use pnpm_tarball::PrefetchResult;
use std::collections::{HashMap, HashSet};

/// One warm entry: the snapshot, its prefetched CAS paths, the cache key
/// that found them, and whether its slot needs a build marker.
pub(super) type WarmEntry<'a> = (
    &'a PackageKey,
    &'a SnapshotEntry,
    &'a std::sync::Arc<HashMap<String, std::path::PathBuf>>,
    &'a str,
    bool,
);

pub(super) struct Partition<'a> {
    pub warm: Vec<WarmEntry<'a>>,
    pub cold: Vec<(&'a PackageKey, &'a SnapshotEntry)>,
    /// Bundled manifests recovered from the store index, so the bin
    /// linker need not re-read each child's `package.json`.
    pub package_manifests: PackageManifests,
    pub side_effects_maps_by_snapshot: SideEffectsMapsBySnapshot,
    pub side_effects_by_snapshot: SideEffectsBySnapshot,
    pub remote_side_effects_quarantine_by_snapshot: RemoteSideEffectsQuarantineBySnapshot,
    pub store_index_keys_by_snapshot: StoreIndexKeysBySnapshot,
    pub requires_build_by_snapshot: RequiresBuildBySnapshot,
}

/// Assign every snapshot to the warm or cold batch, and fold the
/// prefetched store-index rows into the maps the build and bin phases
/// read.
///
/// Skipped snapshots are walked too, and contribute their rows without
/// entering either batch: they have no link work, but omitting their
/// side-effects entries would make the build phase re-run approved
/// scripts on every warm reinstall.
///
/// `marker_rebuilds` withholds the side-effects row for a slot whose
/// global-virtual-store build marker says it must be rebuilt — keeping
/// the row would let the `is_built` gate skip the very build the marker
/// is asking for.
pub(super) fn partition_snapshots<'a>(
    snapshot_entries: &'a [SnapshotWithCacheKey<'a>],
    skipped_entries: &'a [SnapshotWithCacheKey<'a>],
    prefetch: &'a PrefetchResult,
    marker_rebuilds: &HashSet<PackageKey>,
    node_linker: NodeLinker,
) -> Partition<'a> {
    // The warm batch runs on rayon rather than per-snapshot tokio
    // futures. Profiled at 1352 prefetched / 0 cold on a 10-core Mac:
    // each future's sync `rayon::join` pinned a tokio worker and
    // saturated the pool, so `sum-of-link ~= wall` — effectively 1x
    // parallelism. One `par_iter` over every snapshot lets the pool
    // work-steal across all of them; wall dropped ~10 s to ~6.5 s.
    //
    // Cold fetches deliberately run after this returns rather than
    // alongside: racing a network download against a CPU-bound rayon
    // batch buys nothing.
    let mut warm = Vec::with_capacity(snapshot_entries.len());
    let mut cold: Vec<(&PackageKey, &SnapshotEntry)> = Vec::new();
    // Keyed peer-stripped: every peer variant of a package resolves to
    // the same tarball, so they share one bundled manifest, and this is
    // the shape the bin linker looks up by.
    let mut rows = IndexRows::with_capacity_for(prefetch);

    for entry in skipped_entries {
        rows.absorb(entry, prefetch, marker_rebuilds);
    }

    // Second pass: survivors, which additionally take the warm/cold
    // partition that decides which snapshots run the link work.
    for entry in snapshot_entries {
        rows.absorb(entry, prefetch, marker_rebuilds);
        match rows.warm_entry(entry, prefetch) {
            Some(warm_entry) => warm.push(warm_entry),
            None => cold.push((entry.0, entry.1)),
        }
    }
    tracing::info!(
        target: "pacquet::install::phase",
        phase = "create_virtual_store_partition",
        warm = warm.len(),
        cold = cold.len(),
        skipped = skipped_entries.len(),
        total = snapshot_entries.len(),
        node_linker = ?node_linker,
        "phase complete",
    );
    rows.into_partition(warm, cold)
}

/// The store-index rows the build and bin phases read, accumulated
/// across both partition passes.
struct IndexRows {
    package_manifests: PackageManifests,
    side_effects_maps_by_snapshot: SideEffectsMapsBySnapshot,
    side_effects_by_snapshot: SideEffectsBySnapshot,
    remote_side_effects_quarantine_by_snapshot: RemoteSideEffectsQuarantineBySnapshot,
    store_index_keys_by_snapshot: StoreIndexKeysBySnapshot,
    requires_build_by_snapshot: RequiresBuildBySnapshot,
}

impl IndexRows {
    fn with_capacity_for(prefetch: &PrefetchResult) -> Self {
        IndexRows {
            package_manifests: HashMap::with_capacity(prefetch.manifests.len()),
            side_effects_maps_by_snapshot: HashMap::with_capacity(prefetch.side_effects_maps.len()),
            side_effects_by_snapshot: HashMap::with_capacity(prefetch.side_effects.len()),
            remote_side_effects_quarantine_by_snapshot: HashMap::with_capacity(
                prefetch.remote_side_effects_quarantine.len(),
            ),
            store_index_keys_by_snapshot: HashMap::with_capacity(prefetch.cas_paths.len()),
            requires_build_by_snapshot: HashMap::with_capacity(prefetch.requires_build.len()),
        }
    }

    /// Fold one snapshot's prefetched rows in.
    ///
    /// Both passes share this so a skipped snapshot and a survivor can
    /// never diverge in what they contribute.
    fn absorb(
        &mut self,
        entry: &SnapshotWithCacheKey<'_>,
        prefetch: &PrefetchResult,
        marker_rebuilds: &HashSet<PackageKey>,
    ) {
        let snapshot_key = entry.0;
        let Some(cache_key) = entry.2.as_deref() else { return };
        self.store_index_keys_by_snapshot.insert(snapshot_key.clone(), cache_key.to_string());
        if let Some(manifest) = prefetch.manifests.get(cache_key) {
            self.package_manifests
                .entry(snapshot_key.without_peer())
                .or_insert_with(|| std::sync::Arc::clone(manifest));
        }
        // Peer-variants of the same package share the same store-index
        // row → the same `Arc<_>`. Cheap to share.
        if !marker_rebuilds.contains(snapshot_key)
            && let Some(maps) = prefetch.side_effects_maps.get(cache_key)
        {
            self.side_effects_maps_by_snapshot
                .insert(snapshot_key.clone(), std::sync::Arc::clone(maps));
        }
        if let Some(diffs) = prefetch.side_effects.get(cache_key) {
            self.side_effects_by_snapshot
                .insert(snapshot_key.clone(), std::sync::Arc::clone(diffs));
        }
        if let Some(quarantine) = prefetch.remote_side_effects_quarantine.get(cache_key) {
            self.remote_side_effects_quarantine_by_snapshot
                .insert(snapshot_key.clone(), std::sync::Arc::clone(quarantine));
        }
        if let Some(&requires_build) = prefetch.requires_build.get(cache_key) {
            self.requires_build_by_snapshot.insert(snapshot_key.clone(), requires_build);
        }
    }

    /// The warm entry of a survivor whose cache key the prefetch found.
    /// The key rides along so the reporter can skip a duplicate
    /// package-status event when a resolve-time prefetch already emitted
    /// it.
    fn warm_entry<'a>(
        &self,
        entry: &'a SnapshotWithCacheKey<'a>,
        prefetch: &'a PrefetchResult,
    ) -> Option<WarmEntry<'a>> {
        let (snapshot_key, snapshot, cache_key) = entry;
        let key = cache_key.as_deref()?;
        let cas_paths = prefetch.cas_paths.get(key)?;
        let requires_build =
            self.requires_build_by_snapshot.get(*snapshot_key).copied().unwrap_or(false);
        Some((
            *snapshot_key,
            *snapshot,
            cas_paths,
            key,
            snapshot_needs_build_marker(snapshot_key, requires_build),
        ))
    }

    fn into_partition<'a>(
        self,
        warm: Vec<WarmEntry<'a>>,
        cold: Vec<(&'a PackageKey, &'a SnapshotEntry)>,
    ) -> Partition<'a> {
        Partition {
            warm,
            cold,
            package_manifests: self.package_manifests,
            side_effects_maps_by_snapshot: self.side_effects_maps_by_snapshot,
            side_effects_by_snapshot: self.side_effects_by_snapshot,
            remote_side_effects_quarantine_by_snapshot: self
                .remote_side_effects_quarantine_by_snapshot,
            store_index_keys_by_snapshot: self.store_index_keys_by_snapshot,
            requires_build_by_snapshot: self.requires_build_by_snapshot,
        }
    }
}
