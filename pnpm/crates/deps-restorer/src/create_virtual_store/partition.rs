//! Split the snapshots this install materializes into the warm batch —
//! already in the CAFS, so the prefetch covered them — and the cold
//! batch that still needs downloading, and collect the store-index rows
//! later phases read.
//!
//! Runs immediately after the prefetch, whose results it consumes.

use super::{
    PackageBins, PackageManifests, RemoteSideEffectsQuarantineBySnapshot, RequiresBuildBySnapshot,
    SideEffectsBySnapshot, SideEffectsMapsBySnapshot, SnapshotWithCacheKey,
    StoreIndexKeysBySnapshot, WantedEntries, snapshot_needs_build_marker,
};
use crate::AllowBuildPolicy;
use pnpm_config::{Config, NodeLinker};
use pnpm_lockfile::{PackageKey, ProjectSnapshot, SnapshotEntry};
use pnpm_store_dir::StoreIndexWriter;
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
    pub package_bins: PackageBins,
    pub requires_build_by_snapshot: RequiresBuildBySnapshot,
    pub side_effects: PartitionSideEffects,
}

pub(super) struct PartitionSideEffects {
    pub maps: SideEffectsMapsBySnapshot,
    pub diffs: SideEffectsBySnapshot,
    pub quarantine: RemoteSideEffectsQuarantineBySnapshot,
    pub store_index_keys: StoreIndexKeysBySnapshot,
}

#[derive(Clone, Copy)]
pub(super) struct PartitionSideEffectsContext<'a> {
    pub config: &'static Config,
    pub wanted: WantedEntries<'a>,
    pub allow_build_policy: &'a AllowBuildPolicy,
    pub store_index_writer: &'a std::sync::Arc<StoreIndexWriter>,
    pub importers: &'a HashMap<String, ProjectSnapshot>,
}

impl PartitionSideEffects {
    fn with_capacity_for(prefetch: &PrefetchResult) -> Self {
        Self {
            maps: HashMap::with_capacity(prefetch.side_effects_maps.len()),
            diffs: HashMap::with_capacity(prefetch.side_effects.len()),
            quarantine: HashMap::with_capacity(prefetch.remote_side_effects_quarantine.len()),
            store_index_keys: HashMap::with_capacity(prefetch.cas_paths.len()),
        }
    }

    fn absorb(
        &mut self,
        snapshot_key: &PackageKey,
        cache_key: &str,
        prefetch: &PrefetchResult,
        rebuilds: &Rebuilds<'_>,
    ) {
        self.store_index_keys.insert(snapshot_key.clone(), cache_key.to_string());
        // Peer-variants of the same package share the same store-index
        // row → the same `Arc<_>`. Cheap to share.
        if !rebuilds.requires(snapshot_key)
            && let Some(maps) = prefetch.side_effects_maps.get(cache_key)
        {
            self.maps.insert(snapshot_key.clone(), std::sync::Arc::clone(maps));
        }
        if let Some(diffs) = prefetch.side_effects.get(cache_key) {
            self.diffs.insert(snapshot_key.clone(), std::sync::Arc::clone(diffs));
        }
        if let Some(quarantine) = prefetch.remote_side_effects_quarantine.get(cache_key) {
            self.quarantine.insert(snapshot_key.clone(), std::sync::Arc::clone(quarantine));
        }
    }
}

impl Partition<'_> {
    pub(super) async fn prepare_side_effects(
        &mut self,
        ctx: PartitionSideEffectsContext<'_>,
        base_cas_paths: &crate::shared_side_effects::BaseCasPaths,
    ) -> Option<crate::shared_side_effects::PreparedSharedSideEffects> {
        let cold_snapshots: HashSet<&PackageKey> = self.cold
            .iter()
            .map(|(snapshot_key, _)| *snapshot_key)
            .collect();
        let mut options = crate::shared_side_effects::ApplySharedSideEffectsOptions {
            cached: crate::shared_side_effects::SharedSideEffectsCacheRows {
                base_cas_paths,
                cold_snapshots: Some(&cold_snapshots),
                by_snapshot: &self.side_effects.diffs,
                quarantine_by_snapshot: &self.side_effects.quarantine,
                store_index_keys_by_snapshot: &self.side_effects.store_index_keys,
            },
            config: ctx.config,
            snapshots: ctx.wanted.snapshots,
            packages: ctx.wanted.packages,
            requires_build_by_snapshot: &self.requires_build_by_snapshot,
            allow_build_policy: ctx.allow_build_policy,
            side_effects_maps_by_snapshot: &mut self.side_effects.maps,
            store_index_writer: ctx.store_index_writer,
        };
        crate::shared_side_effects::prepare_shared_side_effects(&mut options, ctx.importers).await
    }

    pub(super) async fn apply_side_effects(
        &mut self,
        ctx: PartitionSideEffectsContext<'_>,
        base_cas_paths: &crate::shared_side_effects::BaseCasPaths,
    ) {
        crate::shared_side_effects::apply_shared_side_effects(
            crate::shared_side_effects::ApplySharedSideEffectsOptions {
                cached: crate::shared_side_effects::SharedSideEffectsCacheRows {
                    base_cas_paths,
                    cold_snapshots: None,
                    by_snapshot: &self.side_effects.diffs,
                    quarantine_by_snapshot: &self.side_effects.quarantine,
                    store_index_keys_by_snapshot: &self.side_effects.store_index_keys,
                },
                config: ctx.config,
                snapshots: ctx.wanted.snapshots,
                packages: ctx.wanted.packages,
                requires_build_by_snapshot: &self.requires_build_by_snapshot,
                allow_build_policy: ctx.allow_build_policy,
                side_effects_maps_by_snapshot: &mut self.side_effects.maps,
                store_index_writer: ctx.store_index_writer,
            },
            ctx.importers,
        )
        .await;
    }

    pub(super) async fn finish_side_effects(
        &mut self,
        resolved: Option<crate::shared_side_effects::ResolvedSharedSideEffects>,
        ctx: PartitionSideEffectsContext<'_>,
        base_cas_paths: &crate::shared_side_effects::BaseCasPaths,
    ) {
        crate::shared_side_effects::finish_shared_side_effects(
            resolved,
            ctx.config,
            base_cas_paths,
            &mut self.side_effects.maps,
            ctx.store_index_writer,
        )
        .await;
    }
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
/// `rebuilds` withholds the side-effects row of a snapshot that must be
/// built afresh — keeping the row would let the `is_built` gate skip the
/// very build that is asked for.
pub(super) fn partition_snapshots<'a>(
    snapshot_entries: &'a [SnapshotWithCacheKey<'a>],
    skipped_entries: &'a [SnapshotWithCacheKey<'a>],
    prefetch: &'a PrefetchResult,
    rebuilds: &Rebuilds<'_>,
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
        rows.absorb(entry, prefetch, rebuilds, false);
    }

    // Second pass: survivors, which additionally take the warm/cold
    // partition that decides which snapshots run the link work.
    for entry in snapshot_entries {
        rows.absorb(entry, prefetch, rebuilds, true);
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

/// The snapshots whose cached build output the partition withholds.
pub(super) struct Rebuilds<'a> {
    /// The slots whose global-virtual-store build marker says they must
    /// be rebuilt.
    pub markers: &'a HashSet<PackageKey>,
    /// Consulted for the packages `sideEffectsCache.exclude` names.
    pub allow_build_policy: &'a AllowBuildPolicy,
    /// Pre-verified `requiresBuild` answers from resolution metadata.
    pub known_requires_build: Option<&'a RequiresBuildBySnapshot>,
    /// Precalculated bin commands and shebang runtimes from resolution metadata.
    pub known_bins: Option<&'a PackageBins>,
}

impl Rebuilds<'_> {
    fn requires(&self, snapshot_key: &PackageKey) -> bool {
        self.markers.contains(snapshot_key) || !self.allow_build_policy.caches_build(snapshot_key)
    }
}

/// The store-index rows the build and bin phases read, accumulated
/// across both partition passes.
struct IndexRows {
    package_manifests: PackageManifests,
    package_bins: PackageBins,
    requires_build_by_snapshot: RequiresBuildBySnapshot,
    side_effects: PartitionSideEffects,
}

impl IndexRows {
    fn with_capacity_for(prefetch: &PrefetchResult) -> Self {
        IndexRows {
            package_manifests: HashMap::with_capacity(prefetch.manifests.len()),
            package_bins: HashMap::new(),
            requires_build_by_snapshot: HashMap::with_capacity(prefetch.requires_build.len()),
            side_effects: PartitionSideEffects::with_capacity_for(prefetch),
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
        rebuilds: &Rebuilds<'_>,
        seed_known_requires_build: bool,
    ) {
        let snapshot_key = entry.0;
        if seed_known_requires_build
            && let Some(requires_build) =
                super::lookup_known_requires_build(rebuilds.known_requires_build, snapshot_key)
        {
            self.requires_build_by_snapshot.insert(snapshot_key.clone(), requires_build);
        }
        let Some(cache_key) = entry.2.as_deref() else { return };
        self.side_effects.absorb(snapshot_key, cache_key, prefetch, rebuilds);
        if let Some(manifest) = prefetch.manifests.get(cache_key) {
            self.package_manifests
                .entry(snapshot_key.without_peer())
                .or_insert_with(|| std::sync::Arc::clone(manifest));
        }
        if let Some(&requires_build) = prefetch.requires_build.get(cache_key) {
            self.requires_build_by_snapshot.insert(snapshot_key.clone(), requires_build);
        }
        self.absorb_bins(snapshot_key, cache_key, prefetch, rebuilds, seed_known_requires_build);
    }

    fn absorb_bins(
        &mut self,
        snapshot_key: &PackageKey,
        cache_key: &str,
        prefetch: &PrefetchResult,
        rebuilds: &Rebuilds<'_>,
        seed_known_requires_build: bool,
    ) {
        if self.requires_build_by_snapshot.get(snapshot_key) != Some(&false)
            || (!seed_known_requires_build && rebuilds.known_bins.is_none())
        {
            return;
        }
        let peer_stripped = snapshot_key.without_peer();
        if self.package_bins.contains_key(&peer_stripped) {
            return;
        }
        let (Some(manifest), Some(cas_paths)) =
            (prefetch.manifests.get(cache_key), prefetch.cas_paths.get(cache_key))
        else {
            return;
        };
        let Some(resolved) =
            pnpm_package_manifest::resolve_package_bins_from_cas_paths(manifest, cas_paths)
        else {
            return;
        };
        let verified = match super::lookup_known_bins(rebuilds.known_bins, snapshot_key) {
            Some(known) if known.as_ref() == &resolved => std::sync::Arc::clone(known),
            Some(_) | None => std::sync::Arc::new(resolved),
        };
        self.package_bins.insert(peer_stripped, verified);
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
        let requires_build = self.requires_build_by_snapshot
            .get(*snapshot_key)
            .copied()
            .unwrap_or(false);
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
            package_bins: self.package_bins,
            requires_build_by_snapshot: self.requires_build_by_snapshot,
            side_effects: self.side_effects,
        }
    }
}
