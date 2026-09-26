//! The multi-pass loop that hoists missing peers into the importer's
//! direct deps until no required peer is missing and no optional peer
//! is satisfiable from the in-flight preferred-versions map.
//!
//! Two nested fixed-point loops:
//!
//! 1. **Inner / required pass.** Run a peer-hoist discovery pass (a
//!    graph-free peer walk through the shared
//!    [`crate::resolve_peers::PeerHoistDiscovery`] engine) over the
//!    growing tree, collect peers that are required (not optional) and
//!    not already direct deps, pick a specifier per peer via
//!    [`fn@crate::hoist_peers`], and extend the tree with those picks.
//!    Repeat until the picker proposes nothing new.
//! 2. **Outer / optional pass.** Aggregate the optional missing peers
//!    seen across the inner-loop iterations, ask
//!    [`fn@crate::get_hoistable_optional_peers`] which of them have a
//!    preferred version already in scope, and extend the tree with
//!    those. Re-enter the inner loop if any landed.
//!
//! Per-importer slice. The workspace-wide orchestrator
//! [`fn@crate::resolve_workspace`] loops this function for every
//! importer, then runs a single multi-importer
//! [`fn@crate::resolve_peers_workspace`] pass that shares the peer
//! walker's caches across importers and applies `dedupeInjectedDeps`.

pub use options::{
    ImporterPeerOptions, ImporterResolutionInputs, ManifestTransformHooks, PeerLinkOptions,
    ResolveImporterError, ResolveImporterOptions, ResolveImporterResult,
};

mod direct_seeds;
mod hoist_rounds;
mod hoist_state;
mod local_targets;
mod locked_peers;
mod missing_peers;
mod options;

use direct_seeds::DirectSeeds;
use hoist_state::{
    ImporterHoistDependencies, ImporterHoistPolicy, ImporterHoistProgress, ImporterHoistSelection,
};
use local_targets::build_workspace_root_deps;
use locked_peers::LockedPeers;
use missing_peers::partition_missing_peers;
use options::HoistSettings;

use crate::{
    DirectDep,
    dependencies_graph::MissingPeer,
    hoist_peers::{
        DependencyOverrider, HoistPeersOptions, MissingPeerInfo, WorkspaceRootDep,
        get_hoistable_optional_peers_with_locked_versions, hoist_peers,
    },
    parent_pkg_aliases::ParentPkgAliases,
    resolve_dependency_tree::{
        TreeCtx, WantedSpec, WorkspaceTreeCtx, extend_tree, record_changed_direct_deps,
        unwrap_package_name,
    },
    resolve_peers::{
        CandidatePeerRanges, HoistMissingScope, PeerDiscoveryResult, PeerHoistDiscovery,
        ResolvePeersOptions, apply_hoist_missing_scope, index_missing_names,
        peers_accept_provided_versions, resolve_peers, resolved_name_and_version,
    },
    resolved_tree::ResolvedTree,
};
use pnpm_package_manifest::{DependencyGroup, PackageManifest, safe_read_package_json_from_dir};
use pnpm_resolving_resolver_base::{PreferredVersions, Resolver};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::Path,
    sync::Arc,
};

/// Resolve an importer's full dependency graph with auto-install-peers
/// hoisting. See the module-level doc for the algorithm.
pub async fn resolve_importer<DependencyGroupList, Chain>(
    resolver: &Chain,
    manifest: &PackageManifest,
    dependency_groups: DependencyGroupList,
    opts: ResolveImporterOptions,
) -> Result<ResolveImporterResult, ResolveImporterError>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
    Chain: Resolver + ?Sized,
{
    // Both `manifest_hook` and `pnpmfile_hook` live on the workspace ctx
    // (they're workspace-wide, not per-importer). Apply them before
    // sharing the `Arc` — `resolve_importer_with_workspace` reads through
    // the shared ctx and can't mutate it after the fact.
    let workspace = Arc::new(
        WorkspaceTreeCtx::default()
            .with_manifest_hook(opts.hooks.manifest_hook.clone())
            .with_overrides_hook(opts.hooks.overrides_hook.clone())
            .with_pnpmfile_hook(opts.hooks.pnpmfile_hook.clone())
            .with_auto_install_peers(opts.peers.auto_install_peers),
    );
    resolve_importer_with_workspace(
        resolver,
        pnpm_lockfile::Lockfile::ROOT_IMPORTER_KEY,
        0,
        manifest,
        dependency_groups,
        opts,
        workspace,
    )
    .await
}

/// Same as [`fn@resolve_importer`] but reuses a shared
/// [`WorkspaceTreeCtx`] so the resolver's per-`pkgIdWithPatchHash`
/// dedup carries across importers in a workspace install.
pub async fn resolve_importer_with_workspace<DependencyGroupList, Chain>(
    resolver: &Chain,
    importer_id: &str,
    importer_order: usize,
    manifest: &PackageManifest,
    dependency_groups: DependencyGroupList,
    opts: ResolveImporterOptions,
    workspace: Arc<WorkspaceTreeCtx>,
) -> Result<ResolveImporterResult, ResolveImporterError>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
    Chain: Resolver + ?Sized,
{
    let mut state = ImporterHoistState::init(
        resolver,
        importer_id,
        importer_order,
        manifest,
        dependency_groups,
        opts,
        workspace,
    )
    .await?;
    // Single importer, so it *is* the workspace root.
    let root_deps = Arc::new(state.hoistable_root_deps()?);
    let root_dep_versions = Arc::new(state.direct_dep_versions());
    state.set_workspace_root_deps(root_deps, root_dep_versions);
    let mut peer_discovery = PeerHoistDiscovery::new();
    loop {
        state.run_required_round(resolver, &mut peer_discovery).await?;
        if !state.hoist_optional_round(resolver).await? {
            break;
        }
    }
    Ok(state.into_result())
}

/// One importer's resolution state across the workspace's hoist
/// rounds. The multi-importer orchestrator
/// [`fn@crate::resolve_workspace`] initializes every importer before
/// running any hoist round, forming a barrier: every importer's
/// initial wave completes first, then per-round required-peer loops
/// and one optional-peer hoist per round repeat across all importers
/// until no importer hoists — so an optional-peer pick sees every
/// importer's resolved versions, not just the importers processed so
/// far.
pub(crate) struct ImporterHoistState {
    importer_id: String,
    ctx: TreeCtx,
    project_dir: std::path::PathBuf,
    policy: ImporterHoistPolicy,
    links: crate::PeerLinkOptions,
    progress: ImporterHoistProgress,
    dependencies: ImporterHoistDependencies,
    selection: ImporterHoistSelection,
}

pub(crate) struct RequiredRound {
    /// Peer providers that existed before this pass began. Retaining only
    /// this compact lookup lets the importer-scoped tree be dropped after
    /// the peer walk instead of keeping one full tree per workspace importer.
    provider_pkg_ids: HashMap<crate::NodeId, String>,
    discovery: PeerDiscoveryResult,
    /// Whether the round walked the whole direct forest (as opposed to
    /// only the direct deps added since the previous walk). A full walk
    /// resets [`hoist_state::ImporterHoistProgress::merged_missing`] before merging.
    walk_was_full: bool,
}

impl ImporterHoistState {
    /// Resolve the importer's initial direct-dependency wave and set
    /// up the hoist-round state.
    pub(crate) async fn init<DependencyGroupList, Chain>(
        resolver: &Chain,
        importer_id: &str,
        importer_order: usize,
        manifest: &PackageManifest,
        dependency_groups: DependencyGroupList,
        opts: ResolveImporterOptions,
        workspace: Arc<WorkspaceTreeCtx>,
    ) -> Result<Self, ResolveImporterError>
    where
        DependencyGroupList: IntoIterator<Item = DependencyGroup>,
        Chain: Resolver + ?Sized,
    {
        let mut seeds = DirectSeeds::of(manifest, dependency_groups, &opts)?;
        let (mut ctx, settings) = opts.into_tree_ctx(importer_id, importer_order, workspace);
        let locked = LockedPeers::of(&ctx, importer_id);
        record_changed_direct_deps(&ctx, importer_id, &seeds.initial_wanted);
        let direct = extend_tree(
            &ctx,
            resolver,
            std::mem::take(&mut seeds.initial_wanted),
            importer_id,
            &ParentPkgAliases::root(seeds.parent_pkg_aliases.clone()),
        )
        .await?;
        seeds.parent_pkg_aliases.extend(direct.iter().map(|dep| dep.alias.clone()));
        ctx.resolve_new_direct_deps_as_subdeps();
        Ok(Self::assemble(importer_id, ctx, direct, seeds, locked, settings))
    }

    pub(crate) fn importer_id(&self) -> &str {
        &self.importer_id
    }

    pub(crate) fn hoistable_root_deps(
        &self,
    ) -> Result<Vec<WorkspaceRootDep>, ResolveImporterError> {
        build_workspace_root_deps(
            &self.dependencies.direct,
            &self.ctx.snapshot_reachable_from(self.dependencies.direct.clone()),
            &self.dependencies.wanted_specifier_by_alias,
            &self.project_dir,
        )
        .map_err(ResolveImporterError::from)
    }

    pub(crate) fn set_workspace_root_deps(
        &mut self,
        deps: Arc<Vec<WorkspaceRootDep>>,
        dep_versions: Arc<HashMap<String, String>>,
    ) {
        self.dependencies.workspace_root_deps = deps;
        self.dependencies.workspace_root_dep_versions = dep_versions;
    }

    pub(crate) fn set_workspace_root_dep_versions(
        &mut self,
        dep_versions: Arc<HashMap<String, String>>,
    ) {
        self.dependencies.workspace_root_dep_versions = dep_versions;
    }

    /// `alias → version` of the importer's direct dependencies, including
    /// the real package name as fallback when aliased.
    pub(crate) fn direct_dep_versions(&self) -> HashMap<String, String> {
        let mut versions = HashMap::default();
        let resolved: Vec<_> = self.dependencies.direct
            .iter()
            .filter_map(|dep| {
                let (real_name, version) =
                    self.ctx.workspace().inspect_package(&dep.id, resolved_name_and_version)?;
                Some((&dep.alias, real_name, version))
            })
            .collect();
        for (alias, _, version) in &resolved {
            versions.entry((*alias).clone()).or_insert_with(|| (*version).clone());
        }
        for (alias, real_name, version) in resolved {
            if *alias != real_name {
                versions.entry(real_name).or_insert(version);
            }
        }
        versions
    }

    fn peers_opts(&self) -> ResolvePeersOptions {
        ResolvePeersOptions {
            peers_suffix_max_length: self.policy.peers_suffix_max_length,
            dedupe_peers: self.policy.peers.dedupe_peers,
            project_dir: Some(self.project_dir.clone()),
            links: crate::PeerLinkOptions {
                exclude_links_from_lockfile: self.links.exclude_links_from_lockfile,
                lockfile_dir: self.links.lockfile_dir.clone(),
                modules_dir: self.links.modules_dir.clone(),
            },
            scope: crate::PeerResolutionScope {
                hoist_missing_scope: None,
                hoisted_peer_provider_node_ids: self.dependencies
                    .hoisted_peer_provider_node_ids
                    .clone(),
                ..Default::default()
            },
        }
    }

    /// The importer's direct-dep envelopes for the workspace-wide peer
    /// pass (which recomputes peers across importers; the per-importer
    /// pass would be discarded), together with the `NodeIds` of the peer
    /// providers among them (see
    /// [`crate::PeerResolutionScope::hoisted_peer_provider_node_ids`]).
    pub(crate) fn into_direct(self) -> (Vec<DirectDep>, HashSet<crate::NodeId>) {
        (self.dependencies.direct, self.dependencies.hoisted_peer_provider_node_ids)
    }

    /// Run the final per-importer peer pass and emit the result. Used
    /// by the single-importer entry points.
    fn into_result(self) -> ResolveImporterResult {
        let peers_opts = self.peers_opts();
        let mut resolved_tree = self.ctx.into_resolved_tree(self.dependencies.direct);
        let peers_result = resolve_peers(&mut resolved_tree, peers_opts);
        ResolveImporterResult { resolved_tree, peers_result }
    }
}

#[cfg(test)]
mod tests;
