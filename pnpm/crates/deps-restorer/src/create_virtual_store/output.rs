use super::{
    CasPathsByPkgId, HashSet, PackageBins, PackageKey, PackageManifests, RequiresBuildBySnapshot,
    SideEffectsBySnapshot, SideEffectsMapsBySnapshot,
};

/// Output of [`super::CreateVirtualStore::run`]. Bundles the bin-link
/// manifest cache, the per-snapshot side-effects-cache overlays the
/// build-phase needs, and the per-install fetch-failure set.
///
/// `fetch_failed` is the set of `optional: true` snapshots whose
/// tarball / metadata / extract step blew up during this install.
/// The caller (`crate::InstallFrozenLockfile::run`) folds these into its
/// own [`crate::SkippedSnapshots`] so downstream consumers
/// (`build_graph`, `link_bins`, hoisting, etc.) treat them as
/// absent — a failed-fetch optional snapshot is not present
/// in the install graph.
pub struct CreateVirtualStoreOutput {
    pub package_manifests: PackageManifests,
    pub package_bins: PackageBins,
    pub side_effects: SideEffectsOutput,
    pub requires_build_by_snapshot: RequiresBuildBySnapshot,
    /// Snapshot keys whose package directories this run materialized.
    /// The build phase uses this list to record only newly deferred
    /// builds under `ignoreScripts`; earlier debt is retained from
    /// `.modules.yaml` by the install orchestrator.
    pub snapshot_selection: SnapshotSelection,
    pub fetch_failed: HashSet<PackageKey>,
    /// Per-package CAS index, populated only when
    /// [`crate::ModuleLinkerContext::kind`] is
    /// [`NodeLinker::Hoisted`](pnpm_modules_yaml::NodeLinker::Hoisted). Threaded into
    /// [`crate::link_hoisted_modules()`] which materializes the
    /// hoisted `node_modules/` tree directly from these CAS paths
    /// — there is no virtual store under hoisted, so this is the
    /// only output that survives into the link phase. `None` for
    /// the isolated and pnp linkers (their slot directories are
    /// the bridge into the link phase instead). Pacquet decouples
    /// fetch and walk, so the index is built here at fetch time.
    pub cas_paths_by_pkg_id: Option<CasPathsByPkgId>,
    pub cas_paths_by_snapshot: Option<crate::CasPathsBySnapshot>,
}

pub struct SnapshotSelection {
    pub materialized: Vec<PackageKey>,
    pub loaded: Option<Vec<PackageKey>>,
}

impl SnapshotSelection {
    pub(super) fn materialized(materialized: Vec<PackageKey>) -> Self {
        Self { materialized, loaded: None }
    }
}

impl CreateVirtualStoreOutput {
    pub fn build_cache<'a>(
        &'a self,
        engine_name: Option<&'a str>,
        store_index_writer: &'a std::sync::Arc<pnpm_store_dir::StoreIndexWriter>,
    ) -> crate::BuildPhaseCache<'a> {
        crate::BuildPhaseCache {
            maps_by_snapshot: &self.side_effects.maps,
            diffs_by_snapshot: &self.side_effects.diffs,
            requires_build_by_snapshot: &self.requires_build_by_snapshot,
            engine_name,
            store_index_writer,
        }
    }
}

#[derive(Default)]
pub struct SideEffectsOutput {
    pub maps: SideEffectsMapsBySnapshot,
    pub diffs: SideEffectsBySnapshot,
}

impl From<super::partition::PartitionSideEffects> for SideEffectsOutput {
    fn from(value: super::partition::PartitionSideEffects) -> Self {
        Self { maps: value.maps, diffs: value.diffs }
    }
}
