//! Split the snapshots this install materializes into the warm batch —
//! already in the CAFS, so the prefetch covered them — and the cold
//! batch that still needs downloading, and collect the store-index rows
//! later phases read.
//!
//! Runs immediately after the prefetch, whose results it consumes.

use super::{
    PackageManifests, RequiresBuildBySnapshot, SideEffectsMapsBySnapshot, SnapshotWithCacheKey,
    snapshot_needs_build_marker,
};
use pacquet_config::NodeLinker;
use pacquet_lockfile::{PackageKey, SnapshotEntry};
use pacquet_tarball::PrefetchResult;
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
    let PrefetchResult {
        cas_paths: prefetched,
        manifests: prefetched_manifests,
        side_effects_maps: prefetched_side_effects,
        requires_build: prefetched_requires_build,
    } = prefetch;

    // Partition snapshots by whether the prefetch covered them. The
    // warm batch — every snapshot whose tarball is already in the
    // CAFS — runs entirely on rayon: no tokio futures, no
    // `try_join_all` polling overhead, no `spawn_blocking` round-trip
    // per snapshot. The cold batch (cache miss → download needed)
    // keeps the existing `try_join_all` + download path.
    //
    // **Why this beats per-snapshot tokio futures:** profiling at
    // 1352 prefetched / 0 cold on a 10-core Mac showed `sum-of-link
    // ≈ wall` (~10 s sum on a 10 s wall, i.e. effectively 1×
    // parallelism) even though `try_join_all` was meant to fan
    // futures across tokio's 10 worker threads. Each future's sync
    // `rayon::join` pinned one tokio worker; with up to 10 such
    // futures progressing concurrently, each one's inner par_iter
    // saturated rayon's pool, and the pool ended up processing one
    // snapshot at a time. Going straight to rayon via a single
    // `par_iter` lets the pool schedule across all 1352 snapshots
    // as one work-stealing graph — the shape pnpm's piscina pool
    // gives implicitly. On the same benchmark, wall dropped from
    // ~10 s to ~6.5 s.
    //
    // The `par_iter` blocks the calling thread for the duration of
    // the warm batch. The cold-batch fetches run *after* this
    // returns; that ordering is intentional — warm-cache work has
    // no network dependency, so we'd be racing a cold download
    // against a CPU/syscall-bound rayon batch for nothing.
    // Element types are inferred from the push calls below — no
    // explicit alias, so the warm tuple's third field stays bound
    // to whatever value type `pacquet_tarball::PrefetchedCasPaths`
    // exposes. A future change there propagates here without a
    // local alias drifting (Copilot review on <https://github.com/pnpm/pacquet/pull/292>).
    let mut warm = Vec::with_capacity(snapshot_entries.len());
    let mut cold: Vec<(&PackageKey, &SnapshotEntry)> = Vec::new();
    // Build a `metadata_key -> manifest` lookup from the prefetched
    // index rows. Snapshot keys differ across peer-resolved
    // variants of the same package (`react-dom@17.0.2(react@...)`),
    // but the bundled manifest is identical across variants
    // because every variant resolves to the same tarball. Keying
    // by [`PkgNameVerPeer::without_peer`] collapses the variants
    // to one entry: same shape as
    // [`pacquet_lockfile::Lockfile::packages`], which is what the
    // bin linker already looks up by.
    let mut package_manifests: PackageManifests =
        HashMap::with_capacity(prefetched_manifests.len());
    let mut side_effects_maps_by_snapshot: SideEffectsMapsBySnapshot =
        HashMap::with_capacity(prefetched_side_effects.len());
    let mut requires_build_by_snapshot: RequiresBuildBySnapshot =
        HashMap::with_capacity(prefetched_requires_build.len());

    // First pass: process *skipped* snapshots into the bin-
    // manifest cache and the side-effects map. They don't enter
    // the warm/cold partition (no link work to do), but their
    // store-index rows are needed downstream so
    // [`crate::BuildModules`]'s `is_built` gate can fire — without
    // these entries, packages with `allowBuilds: true` would
    // re-execute their lifecycle scripts on every warm reinstall.
    for (snapshot_key, _snapshot, cache_key) in skipped_entries {
        if let Some(cache_key) = cache_key.as_deref()
            && let Some(manifest) = prefetched_manifests.get(cache_key)
        {
            package_manifests
                .entry(snapshot_key.without_peer())
                .or_insert_with(|| std::sync::Arc::clone(manifest));
        }
        if !marker_rebuilds.contains(*snapshot_key)
            && let Some(cache_key) = cache_key.as_deref()
            && let Some(maps) = prefetched_side_effects.get(cache_key)
        {
            side_effects_maps_by_snapshot
                .insert((*snapshot_key).clone(), std::sync::Arc::clone(maps));
        }
        if let Some(cache_key) = cache_key.as_deref()
            && let Some(&requires_build) = prefetched_requires_build.get(cache_key)
        {
            requires_build_by_snapshot.insert((*snapshot_key).clone(), requires_build);
        }
    }

    // Second pass: survivors. Same loop as above plus the
    // warm/cold partition that decides which snapshots run the
    // link work.
    for (snapshot_key, snapshot, cache_key) in snapshot_entries {
        if let Some(cache_key) = cache_key.as_deref()
            && let Some(manifest) = prefetched_manifests.get(cache_key)
        {
            package_manifests
                .entry(snapshot_key.without_peer())
                .or_insert_with(|| std::sync::Arc::clone(manifest));
        }
        // Peer-variants of the same package share the same
        // store-index row → the same `Arc<_>`. Cheap to share.
        if !marker_rebuilds.contains(*snapshot_key)
            && let Some(cache_key) = cache_key.as_deref()
            && let Some(maps) = prefetched_side_effects.get(cache_key)
        {
            side_effects_maps_by_snapshot
                .insert((*snapshot_key).clone(), std::sync::Arc::clone(maps));
        }
        if let Some(cache_key) = cache_key.as_deref()
            && let Some(&requires_build) = prefetched_requires_build.get(cache_key)
        {
            requires_build_by_snapshot.insert((*snapshot_key).clone(), requires_build);
        }
        // Carry the cache key alongside the warm entry so the
        // reporter can skip a duplicate package-status event when
        // a resolve-time prefetch already emitted it.
        match cache_key.as_deref().and_then(|key| prefetched.get(key).map(|paths| (key, paths))) {
            Some((key, cas_paths)) => warm.push((
                *snapshot_key,
                *snapshot,
                cas_paths,
                key,
                snapshot_needs_build_marker(
                    snapshot_key,
                    requires_build_by_snapshot.get(*snapshot_key).copied().unwrap_or(false),
                ),
            )),
            None => cold.push((*snapshot_key, *snapshot)),
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
    Partition {
        warm,
        cold,
        package_manifests,
        side_effects_maps_by_snapshot,
        requires_build_by_snapshot,
    }
}
