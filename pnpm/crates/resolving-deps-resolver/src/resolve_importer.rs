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

pub use options::ResolveImporterOptions;

mod hoist_rounds;

mod options;

mod local_targets;
use local_targets::build_workspace_root_deps;

mod missing_peers;
use missing_peers::partition_missing_peers;

mod locked_peers;
use locked_peers::LockedPeers;

use crate::{
    DirectDep,
    dependencies_graph::MissingPeer,
    hoist_peers::{
        DependencyOverrider, HoistPeersOptions, MissingPeerInfo, WorkspaceRootDep,
        get_hoistable_optional_peers_with_locked_versions, hoist_peers,
    },
    parent_pkg_aliases::ParentPkgAliases,
    resolve_dependency_tree::{
        ImporterSlot, ResolveDependencyTreeError, TreeCtx, WantedSpec, WorkspaceHooks,
        WorkspaceTreeCtx, WorkspaceWiring, extend_tree, importer_direct_wanted_specs,
        record_changed_direct_deps, unwrap_package_name,
    },
    resolve_peers::{
        HoistMissingScope, PeerDiscoveryResult, PeerHoistDiscovery, ResolvePeersOptions,
        ResolvePeersResult, apply_hoist_missing_scope, index_missing_names, resolve_peers,
    },
    resolved_tree::ResolvedTree,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use node_semver::{Range, Version};
use pnpm_lockfile::PkgName;
use pnpm_package_manifest::{
    DependencyGroup, PackageManifest, PackageManifestError, safe_read_package_json_from_dir,
};
use pnpm_resolving_resolver_base::{PreferredVersions, Resolver};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::Path,
    sync::Arc,
};

/// Result of [`fn@resolve_importer`] — the fully-walked tree plus the
/// peer-resolution output the install layer consumes.
#[derive(Debug)]
pub struct ResolveImporterResult {
    pub resolved_tree: ResolvedTree,
    pub peers_result: ResolvePeersResult,
}

/// Error envelope for [`fn@resolve_importer`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum ResolveImporterError {
    Resolve(#[error(source)] ResolveDependencyTreeError),

    /// Reading the manifest of a workspace-root `link:` / `file:`
    /// dependency, whose version stands in for the peer it may satisfy.
    RootDepManifest(#[error(source)] PackageManifestError),
}

impl From<ResolveDependencyTreeError> for ResolveImporterError {
    fn from(err: ResolveDependencyTreeError) -> Self {
        ResolveImporterError::Resolve(err)
    }
}

impl From<PackageManifestError> for ResolveImporterError {
    fn from(err: PackageManifestError) -> Self {
        ResolveImporterError::RootDepManifest(err)
    }
}

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
    let hooks = WorkspaceHooks {
        manifest_hook: opts.manifest_hook.clone(),
        overrides_hook: opts.overrides_hook.clone(),
        pnpmfile_hook: opts.pnpmfile_hook.clone(),
        read_package_log: None,
    };
    let wiring = WorkspaceWiring::for_importer(hooks, opts.auto_install_peers);
    let workspace = Arc::new(WorkspaceTreeCtx::from_wiring(wiring));
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
    state.set_workspace_root_deps(root_deps);
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
    direct: Vec<DirectDep>,
    /// Empty until the caller assigns it; see
    /// [`ResolveImporterOptions::resolve_peers_from_workspace_root`].
    workspace_root_deps: Arc<Vec<WorkspaceRootDep>>,
    /// `alias → bare_specifier` as declared. Stands in wherever the
    /// resolver reports no normalized form — a plain `"19.2.0"` arrives
    /// as `None`.
    wanted_specifier_by_alias: BTreeMap<String, String>,
    /// `NodeIds` appended to `direct` by
    /// [`Self::append_resolved_peer_providers`]. Threaded into
    /// [`ResolvePeersOptions::hoisted_peer_provider_node_ids`] so the
    /// peer walk resolves them at their tree position instead of the
    /// importer root.
    hoisted_peer_provider_node_ids: HashSet<crate::NodeId>,
    /// Whether the last required round converged with no missing
    /// required peers left. A converged importer's next required round
    /// is a no-op unless its inputs changed since: an optional hoist
    /// extended its direct set, or a workspace children-ownership
    /// rewrite restructured shared subtrees. A round that broke off
    /// with unhoistable misses is *not* converged — another importer's
    /// resolutions can extend the run-resolved preferred versions and
    /// make those misses hoistable, so it must re-discover every round.
    discovery_converged: bool,
    /// [`crate::WorkspaceTreeCtx::children_rewrites`] at the moment
    /// [`Self::discovery_converged`] was set.
    converged_children_rewrites: u64,
    /// How many entries of [`Self::direct`] previous rounds' discovery
    /// walks covered. A later round walks only the direct deps added
    /// since — the earlier entries' subtrees are unchanged, so their
    /// (scope-filtered) missing reports are replayed from
    /// [`Self::merged_missing`] instead of re-walked.
    walked_direct_len: usize,
    /// [`crate::WorkspaceTreeCtx::children_rewrites`] at the last
    /// discovery walk. A rewrite restructures shared subtrees, so the
    /// next walk covers the whole direct forest again.
    walked_children_rewrites: u64,
    /// Scope-filtered missing-peer issues accumulated across this
    /// importer's discovery walks since its last full walk. Entries
    /// whose peer was hoisted stay behind and are filtered by
    /// [`fn@partition_missing_peers`]'s alias check.
    merged_missing: HashMap<String, Vec<MissingPeer>>,
    parent_pkg_aliases: HashSet<String>,
    all_missing_optional_peers: BTreeMap<String, Vec<String>>,
    /// The lockfile + manifest preferred-versions seed. The hoist
    /// pickers merge it per lookup with the workspace-wide
    /// run-resolved versions — see
    /// [`TreeCtx::preferred_versions_for_names`] — instead of
    /// maintaining a per-importer copy of the whole run history.
    preferred_versions_seed: Arc<PreferredVersions>,
    locked_peer_names: Arc<HashSet<String>>,
    locked_peer_versions: Arc<HashMap<String, HashSet<String>>>,
    override_bare_specifier: Option<Arc<DependencyOverrider>>,
    /// `auto_install_peers || dedupe_peer_dependents` — upstream's
    /// `hoistPeers`. Both hoist rounds no-op when it is `false`, so a
    /// missing peer stays missing and the packages that declare it keep
    /// an unsuffixed snapshot.
    hoist_peers: bool,
    auto_install_peers: bool,
    auto_install_peers_from_highest_match: bool,
    resolve_peers_from_workspace_root: bool,
    peers_suffix_max_length: usize,
    dedupe_peers: bool,
    exclude_links_from_lockfile: bool,
    lockfile_dir: Option<std::path::PathBuf>,
    project_dir: std::path::PathBuf,
    modules_dir: Option<std::path::PathBuf>,
}

pub(crate) struct RequiredRound {
    /// Peer providers that existed before this pass began. Retaining only
    /// this compact lookup lets the importer-scoped tree be dropped after
    /// the peer walk instead of keeping one full tree per workspace importer.
    provider_pkg_ids: HashMap<crate::NodeId, String>,
    discovery: PeerDiscoveryResult,
    /// Whether the round walked the whole direct forest (as opposed to
    /// only the direct deps added since the previous walk). A full walk
    /// resets [`ImporterHoistState::merged_missing`] before merging.
    walk_was_full: bool,
}

/// The importer's wanted direct deps and what the hoist state seeds from
/// them. pnpm seeds `parentPkgAliases` from the importer's wanted
/// dependencies, before any of them resolve, so a direct dep's own
/// peer-shadowed dependency is dropped even when the shadowing sibling
/// is still resolving — and stays seeded even when the sibling drops
/// out (a skipped optional).
struct DirectSeeds {
    initial_wanted: Vec<WantedSpec>,
    wanted_specifier_by_alias: BTreeMap<String, String>,
    parent_pkg_aliases: HashSet<String>,
}

impl DirectSeeds {
    fn of<DependencyGroupList>(
        manifest: &PackageManifest,
        dependency_groups: DependencyGroupList,
        opts: &ResolveImporterOptions,
    ) -> Result<Self, ResolveImporterError>
    where
        DependencyGroupList: IntoIterator<Item = DependencyGroup>,
    {
        let initial_wanted = importer_direct_wanted_specs(
            manifest,
            dependency_groups,
            opts.auto_install_peers,
            &opts.catalogs,
        )?;
        Ok(Self {
            wanted_specifier_by_alias: initial_wanted
                .iter()
                .map(|(alias, range, ..)| (alias.clone(), range.clone()))
                .collect(),
            parent_pkg_aliases: initial_wanted.iter().map(|(alias, ..)| alias.clone()).collect(),
            initial_wanted,
        })
    }
}

/// What the hoist state keeps of the importer's options once the tree
/// context has taken the rest.
struct HoistSettings {
    auto_install_peers: bool,
    auto_install_peers_from_highest_match: bool,
    resolve_peers_from_workspace_root: bool,
    dedupe_peers: bool,
    dedupe_peer_dependents: bool,
    all_preferred_versions: Arc<PreferredVersions>,
    override_bare_specifier: Option<Arc<DependencyOverrider>>,
    exclude_links_from_lockfile: bool,
    lockfile_dir: Option<std::path::PathBuf>,
    project_dir: std::path::PathBuf,
    modules_dir: Option<std::path::PathBuf>,
    peers_suffix_max_length: usize,
}

impl ResolveImporterOptions {
    /// The importer's tree context, and the settings the hoist state
    /// keeps. The manifest hooks are workspace-wide; they live on the
    /// shared [`WorkspaceTreeCtx`] and the caller ([`resolve_importer`]
    /// or `resolve_workspace`) is responsible for setting them there
    /// before handing the `Arc` over.
    fn into_tree_ctx(
        self,
        importer_id: &str,
        importer_order: usize,
        workspace: Arc<WorkspaceTreeCtx>,
    ) -> (TreeCtx, HoistSettings) {
        let project_dir = self.base_opts.project_dir.clone();
        let tree_lockfile_dir = self.lockfile_dir.clone().unwrap_or_else(|| project_dir.clone());
        let slot = ImporterSlot { lockfile_dir: &tree_lockfile_dir, importer_id, importer_order };
        let ctx = TreeCtx::with_workspace(workspace, self.base_opts)
            .with_importer(slot)
            .with_patched_dependencies(self.patched_dependencies)
            .with_resolution_mode(self.pick_lowest_direct, self.subdep_published_by)
            .with_catalogs(self.catalogs);
        let settings = HoistSettings {
            auto_install_peers: self.auto_install_peers,
            auto_install_peers_from_highest_match: self.auto_install_peers_from_highest_match,
            resolve_peers_from_workspace_root: self.resolve_peers_from_workspace_root,
            dedupe_peers: self.dedupe_peers,
            dedupe_peer_dependents: self.dedupe_peer_dependents,
            all_preferred_versions: self.all_preferred_versions,
            override_bare_specifier: self.override_bare_specifier,
            exclude_links_from_lockfile: self.exclude_links_from_lockfile,
            lockfile_dir: self.lockfile_dir,
            project_dir,
            modules_dir: self.modules_dir,
            peers_suffix_max_length: self.peers_suffix_max_length,
        };
        (ctx, settings)
    }
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

    fn assemble(
        importer_id: &str,
        ctx: TreeCtx,
        direct: Vec<DirectDep>,
        seeds: DirectSeeds,
        locked: LockedPeers,
        settings: HoistSettings,
    ) -> Self {
        ImporterHoistState {
            importer_id: importer_id.to_string(),
            ctx,
            direct,
            workspace_root_deps: Arc::default(),
            wanted_specifier_by_alias: seeds.wanted_specifier_by_alias,
            hoisted_peer_provider_node_ids: HashSet::default(),
            discovery_converged: false,
            converged_children_rewrites: 0,
            walked_direct_len: 0,
            walked_children_rewrites: 0,
            merged_missing: HashMap::default(),
            parent_pkg_aliases: seeds.parent_pkg_aliases,
            all_missing_optional_peers: BTreeMap::new(),
            preferred_versions_seed: settings.all_preferred_versions,
            locked_peer_names: locked.names,
            locked_peer_versions: locked.versions,
            override_bare_specifier: settings.override_bare_specifier,
            hoist_peers: settings.auto_install_peers || settings.dedupe_peer_dependents,
            auto_install_peers: settings.auto_install_peers,
            auto_install_peers_from_highest_match: settings.auto_install_peers_from_highest_match,
            resolve_peers_from_workspace_root: settings.resolve_peers_from_workspace_root,
            peers_suffix_max_length: settings.peers_suffix_max_length,
            dedupe_peers: settings.dedupe_peers,
            exclude_links_from_lockfile: settings.exclude_links_from_lockfile,
            lockfile_dir: settings.lockfile_dir,
            project_dir: settings.project_dir,
            modules_dir: settings.modules_dir,
        }
    }

    pub(crate) fn importer_id(&self) -> &str {
        &self.importer_id
    }

    pub(crate) fn hoistable_root_deps(
        &self,
    ) -> Result<Vec<WorkspaceRootDep>, ResolveImporterError> {
        build_workspace_root_deps(
            &self.direct,
            &self.ctx.snapshot_reachable_from(self.direct.clone()),
            &self.wanted_specifier_by_alias,
            &self.project_dir,
        )
        .map_err(ResolveImporterError::from)
    }

    pub(crate) fn set_workspace_root_deps(&mut self, deps: Arc<Vec<WorkspaceRootDep>>) {
        self.workspace_root_deps = deps;
    }

    fn peers_opts(&self) -> ResolvePeersOptions {
        ResolvePeersOptions {
            peers_suffix_max_length: self.peers_suffix_max_length,
            dedupe_peers: self.dedupe_peers,
            exclude_links_from_lockfile: self.exclude_links_from_lockfile,
            lockfile_dir: self.lockfile_dir.clone(),
            project_dir: Some(self.project_dir.clone()),
            modules_dir: self.modules_dir.clone(),
            hoist_missing_scope: None,
            hoisted_peer_provider_node_ids: self.hoisted_peer_provider_node_ids.clone(),
            ..ResolvePeersOptions::default()
        }
    }

    /// The importer's direct-dep envelopes for the workspace-wide peer
    /// pass (which recomputes peers across importers; the per-importer
    /// pass would be discarded), together with the `NodeIds` of the peer
    /// providers among them (see
    /// [`ResolvePeersOptions::hoisted_peer_provider_node_ids`]).
    pub(crate) fn into_direct(self) -> (Vec<DirectDep>, HashSet<crate::NodeId>) {
        (self.direct, self.hoisted_peer_provider_node_ids)
    }

    /// Run the final per-importer peer pass and emit the result. Used
    /// by the single-importer entry points.
    fn into_result(self) -> ResolveImporterResult {
        let peers_opts = self.peers_opts();
        let mut resolved_tree = self.ctx.into_resolved_tree(self.direct);
        let peers_result = resolve_peers(&mut resolved_tree, peers_opts);
        ResolveImporterResult { resolved_tree, peers_result }
    }
}

#[cfg(test)]
mod tests;
