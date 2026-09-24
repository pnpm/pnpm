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

mod hoist_state;
use hoist_state::{
    ImporterHoistDependencies, ImporterHoistPolicy, ImporterHoistProgress, ImporterHoistSelection,
};
mod hoist_rounds;

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
        ResolveDependencyTreeError, TreeCtx, WantedSpec, WorkspaceTreeCtx, extend_tree,
        importer_direct_wanted_specs, record_changed_direct_deps, unwrap_package_name,
    },
    resolve_peers::{
        HoistMissingScope, PeerDiscoveryResult, PeerHoistDiscovery, ResolvePeersOptions,
        ResolvePeersResult, apply_hoist_missing_scope, index_missing_names,
        peers_accept_provided_versions, resolve_peers, resolved_version,
    },
    resolved_tree::ResolvedTree,
};
use chrono::{DateTime, Utc};
use derive_more::{Display, Error};
use miette::Diagnostic;
use node_semver::{Range, Version};
use pnpm_catalogs_types::Catalogs;
use pnpm_lockfile::PkgName;
use pnpm_package_manifest::{
    DependencyGroup, PackageManifest, PackageManifestError, safe_read_package_json_from_dir,
};
use pnpm_patching::PatchGroupRecord;
use pnpm_resolving_resolver_base::{PreferredVersions, ResolveOptions, Resolver};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::Path,
    sync::Arc,
};

/// Options threaded into [`fn@resolve_importer`].
pub struct ResolveImporterOptions {
    pub base_opts: ResolveOptions,
    /// Cap on the rendered peer-suffix before the suffix is replaced
    /// with a short hash. Threaded into [`fn@resolve_peers`] via
    /// [`ResolvePeersOptions`]. This is the `peersSuffixMaxLength`
    /// setting (default 1000).
    pub peers_suffix_max_length: usize,
    pub peers: ImporterPeerOptions,
    pub links: PeerLinkOptions,
    pub resolution: ImporterResolutionInputs,
    pub hooks: ManifestTransformHooks,
}

#[derive(Debug, Clone)]
pub struct ImporterPeerOptions {
    /// When true, missing required peers get installed at the importer
    /// even if no preferred version is in scope (the picker uses the
    /// peer's declared range as the specifier).
    pub auto_install_peers: bool,
    /// When true, conflicting peer ranges from multiple consumers are
    /// merged with `||` instead of being dropped on intersection
    /// failure. This is the `autoInstallPeersFromHighestMatch` setting.
    pub auto_install_peers_from_highest_match: bool,
    /// When true, a missing peer matching one of the *workspace root*
    /// importer's direct deps is installed from that dep's specifier.
    /// [`fn@crate::resolve_workspace`] supplies the root's deps; the
    /// single-importer [`fn@resolve_importer`] path supplies its own.
    pub resolve_peers_from_workspace_root: bool,
    /// Threaded into [`ResolvePeersOptions::dedupe_peers`] on every
    /// `resolve_peers` invocation inside the auto-install-peers loop.
    /// See the field doc on [`ResolvePeersOptions`] for the behavior.
    pub dedupe_peers: bool,
    /// The `dedupePeerDependents` setting (default `true`). Together
    /// with [`Self::auto_install_peers`] it decides whether the hoist
    /// rounds run at all — a peer nothing asked to install is still
    /// hoisted to collapse peer-suffixed variants, so turning both off
    /// leaves every missing peer missing. The cross-importer collapse
    /// itself lives in [`fn@crate::resolve_peers_workspace`] and reads
    /// the setting separately.
    pub dedupe_peer_dependents: bool,
}

#[derive(Debug, Default, Clone)]
pub struct PeerLinkOptions {
    /// When `true`, `link:` direct deps whose target lives outside
    /// the lockfile root are seeded into the peer-resolution parent
    /// map with a remapped node id
    /// (`link:<rel-from-lockfile_dir-to-modules_dir>/<alias>`) so the
    /// peer suffix stays stable across machines. This is the
    /// `excludeLinksFromLockfile` flow. The remap fires only when
    /// [`Self::lockfile_dir`] and [`Self::modules_dir`] are both set.
    pub exclude_links_from_lockfile: bool,
    /// Absolute path of the directory `pnpm-lock.yaml` lives in.
    /// Forwarded to [`crate::resolve_peers()`] for the
    /// `excludeLinksFromLockfile` remap; the gate is no-op when `None`.
    pub lockfile_dir: Option<std::path::PathBuf>,
    /// Absolute path of the importer's `node_modules` directory.
    /// Forwarded to [`crate::resolve_peers()`] for the
    /// `excludeLinksFromLockfile` remap; the gate is no-op when `None`.
    pub modules_dir: Option<std::path::PathBuf>,
}

pub struct ImporterResolutionInputs {
    /// Seed for the preferred-versions tie-break table: the lockfile +
    /// manifest entries the peer-hoist pickers bias toward, so a
    /// version a sibling already brought is reused instead of adding a
    /// second instance. Versions resolved into the settled tree are
    /// derived once, workspace-wide, on the tree context and merged
    /// with these seed buckets per lookup (seed entries win) — see
    /// `TreeCtx::preferred_versions_for_names`. Pass the result of
    /// `get_preferred_versions_from_lockfile_and_manifests` from the
    /// `lockfile-preferred-versions` crate, or an empty map when no
    /// lockfile + manifest seeding is available.
    pub all_preferred_versions: Arc<PreferredVersions>,
    /// Applies `overrides` to auto-installed peers. See
    /// [`crate::DependencyOverrider`].
    pub override_bare_specifier: Option<Arc<DependencyOverrider>>,
    /// Configured `patchedDependencies`, grouped by package name. The
    /// tree walker appends `(patch_hash=<hash>)` to each matched
    /// package's `pkgIdWithPatchHash` and records the matched key on
    /// [`crate::ResolvedTree::applied_patches`]. `None` when no
    /// patches are configured for this install.
    pub patched_dependencies: Option<Arc<PatchGroupRecord>>,
    /// When `true`, the importer's direct dependencies are resolved to
    /// their lowest satisfying version (`resolutionMode: time-based` /
    /// `lowest-direct`). Transitive deps are always picked highest.
    pub pick_lowest_direct: bool,
    /// Publish-date cutoff applied to transitive dependencies. Under
    /// `resolutionMode: time-based` this is the workspace-wide cutoff
    /// derived from the resolved direct deps (the multi-importer
    /// orchestrator [`fn@crate::resolve_workspace`] computes it and
    /// overrides this field); otherwise it should equal
    /// `base_opts.published_by` (the `minimumReleaseAge` cutoff) so
    /// subdep resolution is unchanged. Direct deps always use
    /// `base_opts.published_by`, never this value.
    pub subdep_published_by: Option<DateTime<Utc>>,
    /// Catalogs parsed from `pnpm-workspace.yaml`. Applied to importer
    /// dependencies and to children of injected workspace packages.
    pub catalogs: Catalogs,
    /// Directory `pnpm-workspace.yaml` sits in, which a `file:` /
    /// `link:` catalog entry's relative path is measured from. `None`
    /// when the install has no workspace manifest, and so no catalogs.
    pub catalogs_dir: Option<std::path::PathBuf>,
    pub catalog_server: bool,
}

#[derive(Default, Clone)]
pub struct ManifestTransformHooks {
    /// `readPackageHook` applied to every resolved manifest before
    /// downstream consumers see it. Today drives `packageExtensions`;
    /// see [`crate::ManifestHook`].
    pub manifest_hook: Option<crate::ManifestHook>,
    /// Post-pnpmfile manifest hook (overrides). See
    /// `WorkspaceTreeCtx::overrides_hook` for the ordering contract.
    pub overrides_hook: Option<crate::ManifestHook>,
    /// `pnpmfileHook` applied to every resolved manifest. Wraps
    /// `readPackage` from `.pnpmfile.cjs` / `pnpmfile.cjs`.
    pub pnpmfile_hook: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
}

impl std::fmt::Debug for ResolveImporterOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolveImporterOptions")
            .field("auto_install_peers", &self.peers.auto_install_peers)
            .field(
                "auto_install_peers_from_highest_match",
                &self.peers.auto_install_peers_from_highest_match,
            )
            .field(
                "resolve_peers_from_workspace_root",
                &self.peers.resolve_peers_from_workspace_root,
            )
            .field("dedupe_peers", &self.peers.dedupe_peers)
            .field("dedupe_peer_dependents", &self.peers.dedupe_peer_dependents)
            .field("all_preferred_versions", &self.resolution.all_preferred_versions)
            .field(
                "override_bare_specifier",
                &self.resolution.override_bare_specifier.as_ref().map(|_| "<overrider>"),
            )
            .field("patched_dependencies", &self.resolution.patched_dependencies)
            .field("base_opts", &self.base_opts)
            .field("pick_lowest_direct", &self.resolution.pick_lowest_direct)
            .field("subdep_published_by", &self.resolution.subdep_published_by)
            .field("catalogs", &self.resolution.catalogs)
            .field("exclude_links_from_lockfile", &self.links.exclude_links_from_lockfile)
            .field("lockfile_dir", &self.links.lockfile_dir)
            .field("modules_dir", &self.links.modules_dir)
            .field("peers_suffix_max_length", &self.peers_suffix_max_length)
            .field("catalog_server", &self.resolution.catalog_server)
            .field("manifest_hook", &self.hooks.manifest_hook.as_ref().map(|_| "<hook>"))
            .field("overrides_hook", &self.hooks.overrides_hook.as_ref().map(|_| "<hook>"))
            .field("pnpmfile_hook", &self.hooks.pnpmfile_hook.as_ref().map(|_| "<hook>"))
            .finish()
    }
}

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
            opts.peers.auto_install_peers,
            &opts.resolution.catalogs,
            opts.resolution.catalogs_dir.as_deref(),
        )?;
        Ok(Self {
            wanted_specifier_by_alias: initial_wanted
                .iter()
                .map(|(alias, range, ..)| (alias.clone(), range.clone()))
                .collect(),
            parent_pkg_aliases: initial_wanted
                .iter()
                .map(|(alias, ..)| alias.clone())
                .collect(),
            initial_wanted,
        })
    }
}

/// What the hoist state keeps of the importer's options once the tree
/// context has taken the rest.
struct HoistSettings {
    all_preferred_versions: Arc<PreferredVersions>,
    override_bare_specifier: Option<Arc<DependencyOverrider>>,
    project_dir: std::path::PathBuf,
    peers_suffix_max_length: usize,
    peers: crate::ImporterPeerOptions,
    links: crate::PeerLinkOptions,
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
        let project_dir = self.base_opts.project.project_dir.clone();
        let tree_lockfile_dir =
            self.links.lockfile_dir.clone().unwrap_or_else(|| project_dir.clone());
        let ctx = TreeCtx::with_workspace(workspace, self.base_opts)
            .with_lockfile_dir(&tree_lockfile_dir)
            .with_importer_id(importer_id)
            .with_importer_order(importer_order)
            .with_patched_dependencies(self.resolution.patched_dependencies)
            .with_resolution_mode(
                self.resolution.pick_lowest_direct,
                self.resolution.subdep_published_by,
            )
            .with_catalogs(self.resolution.catalogs, self.resolution.catalogs_dir.clone());
        let settings = HoistSettings {
            all_preferred_versions: self.resolution.all_preferred_versions,
            override_bare_specifier: self.resolution.override_bare_specifier,
            project_dir,
            peers_suffix_max_length: self.peers_suffix_max_length,
            peers: crate::ImporterPeerOptions {
                auto_install_peers: self.peers.auto_install_peers,
                auto_install_peers_from_highest_match: self.peers
                    .auto_install_peers_from_highest_match,
                resolve_peers_from_workspace_root: self.peers.resolve_peers_from_workspace_root,
                dedupe_peers: self.peers.dedupe_peers,
                dedupe_peer_dependents: self.peers.dedupe_peer_dependents,
            },
            links: crate::PeerLinkOptions {
                exclude_links_from_lockfile: self.links.exclude_links_from_lockfile,
                lockfile_dir: self.links.lockfile_dir,
                modules_dir: self.links.modules_dir,
            },
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

    /// `alias → version` of the importer's direct dependencies.
    pub(crate) fn direct_dep_versions(&self) -> HashMap<String, String> {
        let mut versions = HashMap::default();
        for dep in &self.dependencies.direct {
            if versions.contains_key(&dep.alias) {
                continue;
            }
            if let Some(version) = self.ctx.workspace().inspect_package(&dep.id, resolved_version) {
                versions.insert(dep.alias.clone(), version);
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
