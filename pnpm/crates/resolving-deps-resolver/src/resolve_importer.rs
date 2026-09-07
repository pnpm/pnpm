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
        ResolvePeersResult, apply_hoist_missing_scope, index_missing_names, resolve_peers,
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

    pub base_opts: ResolveOptions,

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

    /// Cap on the rendered peer-suffix before the suffix is replaced
    /// with a short hash. Threaded into [`fn@resolve_peers`] via
    /// [`ResolvePeersOptions`]. This is the `peersSuffixMaxLength`
    /// setting (default 1000).
    pub peers_suffix_max_length: usize,

    pub catalog_server: bool,

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
            .field("auto_install_peers", &self.auto_install_peers)
            .field(
                "auto_install_peers_from_highest_match",
                &self.auto_install_peers_from_highest_match,
            )
            .field("resolve_peers_from_workspace_root", &self.resolve_peers_from_workspace_root)
            .field("dedupe_peers", &self.dedupe_peers)
            .field("dedupe_peer_dependents", &self.dedupe_peer_dependents)
            .field("all_preferred_versions", &self.all_preferred_versions)
            .field(
                "override_bare_specifier",
                &self.override_bare_specifier.as_ref().map(|_| "<overrider>"),
            )
            .field("patched_dependencies", &self.patched_dependencies)
            .field("base_opts", &self.base_opts)
            .field("pick_lowest_direct", &self.pick_lowest_direct)
            .field("subdep_published_by", &self.subdep_published_by)
            .field("catalogs", &self.catalogs)
            .field("exclude_links_from_lockfile", &self.exclude_links_from_lockfile)
            .field("lockfile_dir", &self.lockfile_dir)
            .field("modules_dir", &self.modules_dir)
            .field("peers_suffix_max_length", &self.peers_suffix_max_length)
            .field("catalog_server", &self.catalog_server)
            .field("manifest_hook", &self.manifest_hook.as_ref().map(|_| "<hook>"))
            .field("overrides_hook", &self.overrides_hook.as_ref().map(|_| "<hook>"))
            .field("pnpmfile_hook", &self.pnpmfile_hook.as_ref().map(|_| "<hook>"))
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
    #[display("{_0}")]
    Resolve(#[error(source)] ResolveDependencyTreeError),

    /// Reading the manifest of a workspace-root `link:` / `file:`
    /// dependency, whose version stands in for the peer it may satisfy.
    #[display("{_0}")]
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
            .with_manifest_hook(opts.manifest_hook.clone())
            .with_overrides_hook(opts.overrides_hook.clone())
            .with_pnpmfile_hook(opts.pnpmfile_hook.clone())
            .with_auto_install_peers(opts.auto_install_peers),
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
        let ResolveImporterOptions {
            auto_install_peers,
            auto_install_peers_from_highest_match,
            resolve_peers_from_workspace_root,
            dedupe_peers,
            dedupe_peer_dependents,
            all_preferred_versions,
            override_bare_specifier,
            patched_dependencies,
            base_opts,
            pick_lowest_direct,
            subdep_published_by,
            catalogs,
            exclude_links_from_lockfile,
            lockfile_dir,
            modules_dir,
            peers_suffix_max_length,
            catalog_server: _,
            // The manifest hooks are workspace-wide; they live on the shared
            // [`WorkspaceTreeCtx`] and the caller (`resolve_importer` or
            // `resolve_workspace`) is responsible for setting them there before
            // handing the `Arc` to this function.
            manifest_hook: _,
            overrides_hook: _,
            pnpmfile_hook: _,
        } = opts;

        let initial_wanted = importer_direct_wanted_specs(
            manifest,
            dependency_groups,
            auto_install_peers,
            &catalogs,
        )?;
        let project_dir = base_opts.project_dir.clone();
        let tree_lockfile_dir = lockfile_dir.clone().unwrap_or_else(|| project_dir.clone());
        let mut ctx = TreeCtx::with_workspace(workspace, base_opts)
            .with_lockfile_dir(&tree_lockfile_dir)
            .with_importer_id(importer_id)
            .with_importer_order(importer_order)
            .with_patched_dependencies(patched_dependencies)
            .with_resolution_mode(pick_lowest_direct, subdep_published_by)
            .with_catalogs(catalogs);
        let locked_peer_versions = Arc::new(importer_locked_peer_versions(
            ctx.workspace().wanted_lockfile().map(AsRef::as_ref),
            importer_id,
        ));
        let locked_peer_names = Arc::new(locked_peer_versions.keys().cloned().collect());
        record_changed_direct_deps(&ctx, importer_id, &initial_wanted);
        let wanted_specifier_by_alias: BTreeMap<String, String> = initial_wanted
            .iter()
            .map(|(alias, range, ..)| (alias.clone(), range.clone()))
            .collect();
        // pnpm seeds `parentPkgAliases` from the importer's wanted
        // dependencies, before any of them resolve, so a direct dep's
        // own peer-shadowed dependency is dropped even when the
        // shadowing sibling is still resolving — and stays seeded even
        // when the sibling drops out (a skipped optional).
        let mut parent_pkg_aliases: HashSet<String> =
            initial_wanted.iter().map(|(alias, ..)| alias.clone()).collect();
        let direct = extend_tree(
            &ctx,
            resolver,
            initial_wanted,
            importer_id,
            &ParentPkgAliases::root(parent_pkg_aliases.clone()),
        )
        .await?;
        parent_pkg_aliases.extend(direct.iter().map(|dep| dep.alias.clone()));
        ctx.resolve_new_direct_deps_as_subdeps();
        Ok(ImporterHoistState {
            importer_id: importer_id.to_string(),
            ctx,
            direct,
            workspace_root_deps: Arc::default(),
            wanted_specifier_by_alias,
            hoisted_peer_provider_node_ids: HashSet::default(),
            discovery_converged: false,
            converged_children_rewrites: 0,
            walked_direct_len: 0,
            walked_children_rewrites: 0,
            merged_missing: HashMap::default(),
            parent_pkg_aliases,
            all_missing_optional_peers: BTreeMap::new(),
            preferred_versions_seed: all_preferred_versions,
            locked_peer_names,
            locked_peer_versions,
            override_bare_specifier,
            hoist_peers: auto_install_peers || dedupe_peer_dependents,
            auto_install_peers,
            auto_install_peers_from_highest_match,
            resolve_peers_from_workspace_root,
            peers_suffix_max_length,
            dedupe_peers,
            exclude_links_from_lockfile,
            lockfile_dir,
            project_dir,
            modules_dir,
        })
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

    /// Resolve the importer's missing *required* peers to a fixpoint,
    /// rebuilding the missing-*optional* buckets the round's
    /// [`Self::hoist_optional_round`] consumes. No-op when nothing may
    /// be hoisted (see [`ImporterHoistState::hoist_peers`]); the final
    /// peer pass still reports every unmet peer as a warning.
    pub(crate) async fn run_required_round<Chain>(
        &mut self,
        resolver: &Chain,
        peer_discovery: &mut PeerHoistDiscovery,
    ) -> Result<(), ResolveImporterError>
    where
        Chain: Resolver + ?Sized,
    {
        if !self.hoist_peers {
            return Ok(());
        }
        if self.discovery_converged
            && self.ctx.workspace().children_rewrites() == self.converged_children_rewrites
        {
            // See [`Self::discovery_converged`]: re-discovering an
            // unchanged importer reproduces the state its last round
            // converged to. Skipping keeps `all_missing_optional_peers`
            // intact for the next optional round.
            return Ok(());
        }
        self.begin_required_round();
        self.complete_required_round(resolver, None, peer_discovery).await
    }

    pub(crate) fn prepare_initial_required_round(
        &mut self,
        peer_discovery: &mut PeerHoistDiscovery,
    ) -> Option<RequiredRound> {
        if !self.hoist_peers {
            return None;
        }
        self.begin_required_round();
        Some(self.resolve_required_round(None, peer_discovery))
    }

    /// The caller snapshots the two context maps once per barrier and
    /// shares them across every importer's scope.
    pub(crate) fn apply_owner_missing_scope(
        &self,
        round: &mut RequiredRound,
        first_importer_by_pkg: &Arc<HashMap<String, String>>,
        first_walk_missing_by_pkg: &Arc<HashMap<String, HashSet<String>>>,
    ) {
        apply_hoist_missing_scope(
            &mut round.discovery,
            &HoistMissingScope {
                importer_id: self.importer_id.clone(),
                first_importer_by_pkg: Arc::clone(first_importer_by_pkg),
                first_walk_missing_by_pkg: Arc::clone(first_walk_missing_by_pkg),
                locked_peer_names: Arc::clone(&self.locked_peer_names),
            },
        );
    }

    pub(crate) async fn complete_initial_required_round<Chain>(
        &mut self,
        resolver: &Chain,
        round: RequiredRound,
        peer_discovery: &mut PeerHoistDiscovery,
    ) -> Result<(), ResolveImporterError>
    where
        Chain: Resolver + ?Sized,
    {
        self.complete_required_round(resolver, Some(round), peer_discovery).await
    }

    fn begin_required_round(&mut self) {
        // The hoist input must not see missing peers declared inside a
        // subtree owned by another importer's shared children context —
        // the owner walk's children report is reused there, so those
        // peers never reach a non-owner importer's hoist. The final peer
        // pass keeps the unscoped options so warnings stay complete.
        self.all_missing_optional_peers.clear();
    }

    fn resolve_required_round(
        &mut self,
        hoist_missing_scope: Option<Arc<HoistMissingScope>>,
        peer_discovery: &mut PeerHoistDiscovery,
    ) -> RequiredRound {
        let children_rewrites = self.ctx.workspace().children_rewrites();
        let walk_was_full =
            self.walked_direct_len == 0 || children_rewrites != self.walked_children_rewrites;
        let walk_from = if walk_was_full { 0 } else { self.walked_direct_len };
        let discovery = {
            let mut opts = self.peers_opts();
            opts.hoist_missing_scope = hoist_missing_scope;
            peer_discovery.discover(
                self.ctx.workspace(),
                &self.direct,
                &self.direct[walk_from..],
                opts,
            )
        };
        self.walked_direct_len = self.direct.len();
        self.walked_children_rewrites = children_rewrites;
        let provider_pkg_ids = self
            .ctx
            .workspace()
            .provider_pkg_ids(discovery.resolved_peer_providers_by_alias.values());
        self.ctx.workspace().record_first_walk_missing(
            &self.importer_id,
            &index_missing_names(&discovery.missing_summaries),
        );
        RequiredRound { provider_pkg_ids, discovery, walk_was_full }
    }

    async fn complete_required_round<Chain>(
        &mut self,
        resolver: &Chain,
        mut first_round: Option<RequiredRound>,
        peer_discovery: &mut PeerHoistDiscovery,
    ) -> Result<(), ResolveImporterError>
    where
        Chain: Resolver + ?Sized,
    {
        loop {
            let round = match first_round.take() {
                Some(round) => round,
                None => self.resolve_required_round(
                    Some(Arc::new(HoistMissingScope {
                        importer_id: self.importer_id.clone(),
                        first_importer_by_pkg: self.ctx.workspace().first_importer_by_pkg(),
                        first_walk_missing_by_pkg: self.ctx.workspace().first_walk_missing_by_pkg(),
                        locked_peer_names: Arc::clone(&self.locked_peer_names),
                    })),
                    peer_discovery,
                ),
            };
            let RequiredRound { provider_pkg_ids, discovery, walk_was_full } = round;

            if walk_was_full {
                self.merged_missing.clear();
            }
            for (peer_name, issues) in &discovery.peer_dependency_issues.missing {
                self.merged_missing
                    .entry(peer_name.clone())
                    .or_default()
                    .extend(issues.iter().cloned());
            }
            let (missing_required, fresh_optional) = partition_missing_peers(
                &self.merged_missing,
                &self.parent_pkg_aliases,
                self.auto_install_peers_from_highest_match,
            );
            self.append_resolved_peer_providers(
                &discovery.resolved_peer_providers_by_alias,
                &provider_pkg_ids,
                &missing_required,
            );
            for (name, ranges) in fresh_optional {
                let bucket = self.all_missing_optional_peers.entry(name).or_default();
                for range in ranges {
                    if !bucket.iter().any(|existing| existing == &range) {
                        bucket.push(range);
                    }
                }
            }

            if missing_required.is_empty() {
                self.discovery_converged = true;
                self.converged_children_rewrites = self.ctx.workspace().children_rewrites();
                break;
            }

            let workspace_root_deps: &[WorkspaceRootDep] = if self.resolve_peers_from_workspace_root
            {
                &self.workspace_root_deps
            } else {
                &[]
            };

            let missing_as_pairs: Vec<(String, MissingPeerInfo)> =
                missing_required.iter().map(|(n, info)| (n.clone(), info.clone())).collect();
            // Both hoists bias toward the run-resolved preferred
            // versions: the seed buckets for the missing names merged
            // with every version resolved into the settled tree so far.
            let hoist_preferred = self.ctx.preferred_versions_for_names(
                &self.preferred_versions_seed,
                missing_as_pairs.iter().map(|(name, _)| name.as_str()),
            );
            let hoisted = hoist_peers(
                &HoistPeersOptions {
                    auto_install_peers: self.auto_install_peers,
                    all_preferred_versions: &hoist_preferred,
                    workspace_root_deps,
                    override_bare_specifier: self.override_bare_specifier.as_deref(),
                    project_dir: &self.project_dir,
                },
                &missing_as_pairs,
            );
            if hoisted.is_empty() {
                break;
            }

            for name in hoisted.keys() {
                self.parent_pkg_aliases.insert(name.clone());
            }

            // Hoisted required peers are installed at the importer
            // level as non-optional direct deps — they exist precisely
            // to satisfy a missing required peer, so flipping their
            // own `optional` flag to `true` would defeat the
            // auto-install. Hoisted peers don't carry
            // `dependenciesMeta` from any manifest, so `injected`
            // defaults to `false`: the hoist path constructs a fresh
            // wanted dependency without threading the per-dep meta.
            let new_wanted: Vec<WantedSpec> =
                hoisted.into_iter().map(|(name, range)| (name, range, false, false)).collect();
            let new_direct = extend_tree(
                &self.ctx,
                resolver,
                new_wanted,
                &self.importer_id,
                &ParentPkgAliases::root(self.parent_pkg_aliases.clone()),
            )
            .await?;
            self.direct.extend(new_direct);
        }
        Ok(())
    }

    fn append_resolved_peer_providers(
        &mut self,
        providers: &BTreeMap<String, crate::NodeId>,
        provider_pkg_ids: &HashMap<crate::NodeId, String>,
        missing_required: &BTreeMap<String, MissingPeerInfo>,
    ) {
        if !self.auto_install_peers {
            return;
        }
        for (alias, node_id) in providers {
            if self.parent_pkg_aliases.contains(alias) || missing_required.contains_key(alias) {
                continue;
            }
            let Some(pkg_id) = provider_pkg_ids.get(node_id) else {
                continue;
            };
            self.direct.push(DirectDep {
                alias: alias.clone(),
                node_id: node_id.clone(),
                id: pkg_id.clone(),
            });
            self.hoisted_peer_provider_node_ids.insert(node_id.clone());
            self.parent_pkg_aliases.insert(alias.clone());
        }
    }

    /// Hoist this round's missing optional peers; `true` when any were
    /// installed (the workspace runs another round). No-op when nothing
    /// may be hoisted (see [`ImporterHoistState::hoist_peers`]).
    pub(crate) async fn hoist_optional_round<Chain>(
        &mut self,
        resolver: &Chain,
    ) -> Result<bool, ResolveImporterError>
    where
        Chain: Resolver + ?Sized,
    {
        if !self.hoist_peers || self.all_missing_optional_peers.is_empty() {
            return Ok(false);
        }
        let workspace_root_deps: &[WorkspaceRootDep] =
            if self.resolve_peers_from_workspace_root { &self.workspace_root_deps } else { &[] };
        let hoist_preferred = self.ctx.preferred_versions_for_names(
            &self.preferred_versions_seed,
            self.all_missing_optional_peers.keys().map(String::as_str),
        );
        let hoisted_optional = get_hoistable_optional_peers_with_locked_versions(
            &self.all_missing_optional_peers,
            &hoist_preferred,
            workspace_root_deps,
            &self.locked_peer_versions,
        );
        if hoisted_optional.is_empty() {
            return Ok(false);
        }
        for name in hoisted_optional.keys() {
            self.parent_pkg_aliases.insert(name.clone());
        }
        // Optional peers picked up via `getHoistableOptionalPeers` are
        // also installed at the importer level — the picker already
        // confirmed a preferred version is in scope. Treating them as
        // non-optional matches the required-peer arm above; `injected`
        // also defaults to `false` for the same reason.
        let new_wanted: Vec<WantedSpec> =
            hoisted_optional.into_iter().map(|(name, range)| (name, range, false, false)).collect();
        let new_direct = extend_tree(
            &self.ctx,
            resolver,
            new_wanted,
            &self.importer_id,
            &ParentPkgAliases::root(self.parent_pkg_aliases.clone()),
        )
        .await?;
        self.direct.extend(new_direct);
        // The direct set changed; the next required round must
        // re-discover so the hoisted names leave the missing buckets.
        self.discovery_converged = false;
        Ok(true)
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

/// The peer versions the wanted lockfile pinned, by peer name: those on
/// the importer's direct dependencies, or on every snapshot for an
/// importer the lockfile does not know yet. The optional-peer hoist
/// only picks versions from this set, and its names stay eligible for
/// importer-local hoisting (see [`HoistMissingScope::locked_peer_names`]).
fn importer_locked_peer_versions(
    wanted_lockfile: Option<&pnpm_lockfile::Lockfile>,
    importer_id: &str,
) -> HashMap<String, HashSet<String>> {
    let Some(lockfile) = wanted_lockfile else {
        return HashMap::default();
    };
    let mut versions = HashMap::<String, HashSet<String>>::default();
    let Some(importer) = lockfile.importers.get(importer_id) else {
        for (key, snapshot) in lockfile.snapshots.iter().flatten() {
            for (name, version) in locked_peer_versions_for_key(lockfile, key, Some(snapshot)) {
                versions.entry(name).or_default().insert(version);
            }
        }
        return versions;
    };
    for (alias, dependency) in importer.dependencies_by_groups([
        DependencyGroup::Prod,
        DependencyGroup::Optional,
        DependencyGroup::Dev,
    ]) {
        let Some(key) = dependency.version.resolved_key(alias) else {
            continue;
        };
        let snapshot = lockfile.snapshots.as_ref().and_then(|snapshots| snapshots.get(&key));
        for (name, version) in locked_peer_versions_for_key(lockfile, &key, snapshot) {
            versions.entry(name).or_default().insert(version);
        }
    }
    versions
}

/// The peer name/version pairs the wanted lockfile pinned for `key`.
///
/// An explicit suffix already spells them out, save for the names an
/// npm alias renamed ([`restore_aliased_peer_names`]). A hashed suffix
/// spells out nothing, so the pairs are recovered from the package's
/// declared peers and the snapshot edges that resolved them.
fn locked_peer_versions_for_key(
    lockfile: &pnpm_lockfile::Lockfile,
    key: &pnpm_lockfile::PkgNameVerPeer,
    snapshot: Option<&pnpm_lockfile::SnapshotEntry>,
) -> Vec<(String, String)> {
    let metadata =
        lockfile.packages.as_ref().and_then(|packages| packages.get(&key.without_peer()));
    let mut explicit = peer_suffix_versions(key.suffix.peer()).collect::<Vec<_>>();
    if !explicit.is_empty() {
        if let Some(snapshot) = snapshot {
            restore_aliased_peer_names(snapshot, metadata, &mut explicit);
        }
        return explicit;
    }
    if !is_hashed_peer_suffix(key.suffix.peer()) {
        return explicit;
    }
    let (Some(snapshot), Some(metadata)) = (snapshot, metadata) else {
        return Vec::new();
    };
    let peer_names = metadata
        .peer_dependencies
        .iter()
        .flatten()
        .filter_map(|(name, _)| name.parse::<PkgName>().ok())
        .collect::<HashSet<_>>();
    snapshot
        .dependencies
        .iter()
        .chain(snapshot.optional_dependencies.iter())
        .flatten()
        .filter(|(name, _)| peer_names.contains(*name))
        .filter_map(|(name, reference)| {
            reference
                .ver_peer()
                .map(|version| (name.to_string(), version.without_peer().to_string()))
        })
        .collect()
}

/// Rename the suffix segments an npm alias provides back to the name
/// the provider is installed under.
///
/// A peer suffix names each provider by the package it resolved to,
/// while peer resolution keys providers by the name they occupy in the
/// dependent's `node_modules` — which for `"peer": "npm:provider@1"` is
/// `peer`, not `provider`. Reading the segment back verbatim would pin a
/// peer nobody declares and leave the declared one unpinned, so a
/// repeated resolution is free to pick the other variant.
fn restore_aliased_peer_names(
    snapshot: &pnpm_lockfile::SnapshotEntry,
    metadata: Option<&pnpm_lockfile::PackageMetadata>,
    peers: &mut [(String, String)],
) {
    let mut providers = provider_aliases(snapshot, metadata);
    if providers.is_empty() {
        return;
    }
    for (name, version) in peers.iter_mut() {
        let Some(alias) =
            providers.get_mut(&format!("{name}@{version}")).and_then(ProviderAliases::claim)
        else {
            continue;
        };
        *name = alias;
    }
}

/// Index the snapshot's dependency edges by the `name@version` of the
/// package each one installs, so every suffix segment is attributed in
/// one lookup rather than another scan of the edges.
///
/// Empty — and every segment therefore left alone — unless some edge
/// installs a package under a name other than its own, since that is the
/// only shape that makes a segment disagree with its edge.
fn provider_aliases(
    snapshot: &pnpm_lockfile::SnapshotEntry,
    metadata: Option<&pnpm_lockfile::PackageMetadata>,
) -> HashMap<String, ProviderAliases> {
    if !dependency_edges(snapshot)
        .any(|(_, reference)| matches!(reference, pnpm_lockfile::SnapshotDepRef::Alias(_)))
    {
        return HashMap::default();
    }
    let declared = metadata.and_then(|metadata| metadata.peer_dependencies.as_ref());
    let mut providers = HashMap::<String, ProviderAliases>::default();
    for (edge_name, reference) in dependency_edges(snapshot) {
        let (provider, ver_peer) = match reference {
            pnpm_lockfile::SnapshotDepRef::Plain(ver_peer) => (edge_name.to_string(), ver_peer),
            pnpm_lockfile::SnapshotDepRef::Alias(target) => {
                (target.name.to_string(), &target.suffix)
            }
            pnpm_lockfile::SnapshotDepRef::Link(_) => continue,
        };
        let edge_name = edge_name.to_string();
        let declares_peer = declared.is_some_and(|peers| peers.contains_key(&edge_name));
        providers
            .entry(format!("{provider}@{}", ver_peer.without_peer()))
            .or_default()
            .record(edge_name, declares_peer);
    }
    providers
}

/// The names one provider is installed under, split by whether the
/// dependent declares that name as a peer.
///
/// The suffix spells one segment per peer-resolved edge and never merges
/// equal ones, so each segment claims an edge of its own.
#[derive(Default)]
struct ProviderAliases {
    declared_peers: Vec<String>,
    /// How many of `declared_peers` the segments seen so far claimed.
    claimed: usize,
    ordinary: Option<String>,
    ordinaries: usize,
}

impl ProviderAliases {
    fn record(&mut self, alias: String, declares_peer: bool) {
        if declares_peer {
            self.declared_peers.push(alias);
            return;
        }
        self.ordinaries += 1;
        if self.ordinaries == 1 {
            self.ordinary = Some(alias);
        }
    }

    /// The name to attribute the next segment naming this provider to.
    ///
    /// Declared peer edges go first, matching the
    /// `peerDependencies`-keyed lookup the TypeScript CLI restores peer
    /// context with; an ordinary edge takes the segment left over, which
    /// is how a peer propagated up from a child — satisfied by an
    /// ordinary dependency, under the name the child declared — gets its
    /// name back.
    ///
    /// Competing ordinary edges are unattributable, since a dependency
    /// may be aliased onto the very package and version a peer resolved
    /// to and nothing in the lockfile tells the two apart, so the segment
    /// keeps the name the suffix spelled.
    fn claim(&mut self) -> Option<String> {
        if self.claimed < self.declared_peers.len() {
            self.claimed += 1;
            return Some(self.declared_peers[self.claimed - 1].clone());
        }
        if self.ordinaries == 1 {
            return self.ordinary.take();
        }
        None
    }
}

fn dependency_edges(
    snapshot: &pnpm_lockfile::SnapshotEntry,
) -> impl Iterator<Item = (&PkgName, &pnpm_lockfile::SnapshotDepRef)> {
    snapshot.dependencies.iter().chain(snapshot.optional_dependencies.iter()).flatten()
}

/// Whether the suffix is the opaque hash
/// [`create_peer_dep_graph_hash`](fn@pnpm_deps_path::create_peer_dep_graph_hash)
/// emits once the spelled-out peers exceed
/// [`ResolvePeersOptions::peers_suffix_max_length`], rather than
/// segments [`peer_suffix_versions`] can read.
fn is_hashed_peer_suffix(peer_suffix: &str) -> bool {
    peer_suffix.rsplit_once('(').and_then(|(_, tail)| tail.strip_suffix(')')).is_some_and(|hash| {
        hash.len() == 32 && hash.chars().all(|character| character.is_ascii_hexdigit())
    })
}

fn peer_suffix_versions(peer_suffix: &str) -> impl Iterator<Item = (String, String)> + '_ {
    peer_suffix.match_indices('(').filter_map(|(start, _)| {
        let segment = peer_suffix[start + 1..].split(['(', ')']).next()?;
        let (name, version) = segment.rsplit_once('@')?;
        (!name.is_empty()).then(|| (name.to_string(), version.to_string()))
    })
}

/// Split the missing-peer report into the inputs the inner and outer
/// loops consume.
///
/// A peer name is **required** for this iteration when at least one of
/// its consumers declared it non-optional and it isn't already in
/// `parent_pkg_aliases` (i.e. not already a direct dep that just
/// hadn't been added to the alias set yet). Its merged range is
/// computed by [`merge_ranges`].
///
/// Peers whose consumers are *all* optional are returned as the second
/// component, keyed by peer name with the deduplicated range list the
/// outer loop's [`fn@crate::get_hoistable_optional_peers`] needs.
fn partition_missing_peers(
    missing: &HashMap<String, Vec<MissingPeer>>,
    parent_pkg_aliases: &HashSet<String>,
    auto_install_peers_from_highest_match: bool,
) -> (BTreeMap<String, MissingPeerInfo>, BTreeMap<String, Vec<String>>) {
    let mut missing_required: BTreeMap<String, MissingPeerInfo> = BTreeMap::new();
    let mut missing_optional: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (peer_name, entries) in missing {
        if parent_pkg_aliases.contains(peer_name) {
            continue;
        }
        // Hoisting a missing required peer fetches it, so it needs the original
        // specifier with its scheme preserved (`work:5.x.x`); hoist_peers reduces
        // it to a comparable range itself. The optional path below dedupes onto
        // an already-present version, so it uses the display range instead.
        let required_ranges: Vec<&str> = entries
            .iter()
            .filter(|entry| !entry.optional)
            .map(|entry| entry.raw_range.as_str())
            .collect();
        if required_ranges.is_empty() {
            let mut seen: BTreeSet<String> = BTreeSet::new();
            let mut ordered: Vec<String> = Vec::new();
            for entry in entries {
                if seen.insert(entry.wanted_range.clone()) {
                    ordered.push(entry.wanted_range.clone());
                }
            }
            if !ordered.is_empty() {
                missing_optional.insert(peer_name.clone(), ordered);
            }
            continue;
        }
        if let Some(range) = merge_ranges(&required_ranges, auto_install_peers_from_highest_match) {
            missing_required.insert(peer_name.clone(), MissingPeerInfo { range });
        }
    }
    (missing_required, missing_optional)
}

/// Combine multiple consumers' wanted ranges into a single specifier,
/// the upstream `mergePkgsDeps` policy: a single (or single unique)
/// range passes through unchanged, distinct compatible ranges reduce to
/// their semver intersection, and an empty intersection falls back to a
/// `||`-join under `auto_install_peers_from_highest_match` — or `None`,
/// dropping the peer on an unresolvable conflict.
///
/// Only the unique ranges reach [`intersect_ranges`]: folding a range in
/// a second time cannot narrow the result, and every pass costs another
/// Cartesian product over the alternatives.
fn merge_ranges(ranges: &[&str], auto_install_peers_from_highest_match: bool) -> Option<String> {
    if ranges.len() == 1 {
        return Some(ranges[0].to_string());
    }
    let mut seen: HashSet<&str> = HashSet::default();
    let unique: Vec<&str> = ranges.iter().copied().filter(|&range| seen.insert(range)).collect();
    if unique.len() == 1 {
        return Some(ranges[0].to_string());
    }
    if let Some(intersection) = intersect_ranges(&unique) {
        return Some(intersection);
    }
    if auto_install_peers_from_highest_match {
        return Some(ranges.join(" || "));
    }
    None
}

/// Semver intersection of every range, rendered in `node-semver`'s
/// canonical form (`2` ∩ `^2.2.0` → `>=2.2.0 <3.0.0-0`, the same shape
/// the `semver-range-intersect` npm package emits upstream). `None`
/// when a range fails to parse or the ranges share no versions,
/// mirroring `safeIntersect`'s caught-throw `null`.
fn intersect_ranges(ranges: &[&str]) -> Option<String> {
    let mut iter = ranges.iter();
    let first = Range::parse(iter.next()?).ok()?;
    iter.try_fold(first, |acc, range| {
        Range::parse(range)
            .ok()
            .and_then(|range| acc.intersect(&range))
            .map(|intersection| collapse_covered_alternatives(&intersection))
    })
    .map(|range| range.to_string())
}

/// Drop the alternatives of a union that another alternative already
/// covers, leaving the same set of versions behind.
///
/// [`Range::intersect`] pairs every alternative of one union with every
/// alternative of the other, so an alternative two consumers agree on
/// survives once per pair and the next intersection multiplies that
/// count again. `semver-range-intersect` collapses the union upstream,
/// which is why the growth is pacquet's alone.
fn collapse_covered_alternatives(range: &Range) -> Range {
    let rendered = range.to_string();
    let alternatives: Vec<&str> = rendered.split("||").collect();
    if alternatives.len() < 2 {
        return range.clone();
    }
    let Some(parsed) = alternatives
        .iter()
        .map(|alternative| Range::parse(alternative).ok())
        .collect::<Option<Vec<Range>>>()
    else {
        return range.clone();
    };
    let mut kept: Vec<&str> = Vec::with_capacity(alternatives.len());
    for (index, alternative) in parsed.iter().enumerate() {
        // Of two alternatives that cover each other, only the first is kept.
        let covered = parsed.iter().enumerate().any(|(other_index, other)| {
            other_index != index
                && other.allows_all(alternative)
                && (other_index < index || !alternative.allows_all(other))
        });
        if !covered {
            kept.push(alternatives[index]);
        }
    }
    Range::parse(kept.join("||")).unwrap_or_else(|_| range.clone())
}

/// `link:` / `file:` and the path form of `workspace:` name a directory
/// relative to the project that declares them, so the root's specifier
/// can't be hoisted verbatim — it would reach a different path from the
/// importer the peer is hoisted into, or nothing. A `workspace:` range is
/// not path-relative: it selects the same workspace package from every
/// importer, so it needs none of this.
fn is_project_relative_specifier(spec: &str) -> bool {
    spec.starts_with("link:") || spec.starts_with("file:") || spec.starts_with("workspace:.")
}

/// The name and version of the package a project-relative specifier points
/// at, or `None` when there is none to read. Its version is what stands in
/// for the path in [`build_workspace_root_deps`], so the root keeps the
/// authority over the peer that a registry dependency has and the peer
/// resolves to the same package from every importer. The manifest is read
/// from disk rather than taken from the resolved tree, which a linked
/// dependency reused from the lockfile is absent from — reading it makes a
/// repeat install hoist what a fresh install of the same manifest hoists.
fn local_target_identity(
    spec: &str,
    project_dir: &Path,
) -> Result<Option<LocalTargetIdentity>, PackageManifestError> {
    let path_without_protocol = spec.split_once(':').map_or(spec, |(_, path)| path);
    let Some(manifest) = read_manifest_of_local_target(&project_dir.join(path_without_protocol))?
    else {
        return Ok(None);
    };
    let Some(version) = manifest.get("version").and_then(serde_json::Value::as_str) else {
        return Ok(None);
    };
    if version.parse::<Version>().is_err() {
        return Ok(None);
    }
    Ok(Some(LocalTargetIdentity {
        name: manifest.get("name").and_then(serde_json::Value::as_str).map(str::to_string),
        version: version.to_string(),
    }))
}

/// What a project-relative root dep is pinned to. See
/// [`local_target_identity`].
struct LocalTargetIdentity {
    /// `None` when the manifest carries no name, leaving the dep under the
    /// name it was already listed by.
    name: Option<String>,
    version: String,
}

/// [`safe_read_package_json_from_dir`] maps only a missing file to
/// `Ok(None)`; a `file:` target is a tarball as often as a directory, and
/// a path component of a tarball is not a directory to read a manifest
/// from.
fn read_manifest_of_local_target(
    dir: &Path,
) -> Result<Option<serde_json::Value>, PackageManifestError> {
    match safe_read_package_json_from_dir(dir) {
        Err(PackageManifestError::Io(err)) if err.kind() == io::ErrorKind::NotADirectory => {
            Ok(None)
        }
        result => result,
    }
}

/// `name_ver`, else the manifest — the canonical name for the protocols
/// that leave `name_ver` unset. The git and remote-tarball resolvers fill
/// `manifest` from the package's own `package.json` during resolution
/// whenever they hold a fetch context, which every install path wires, so
/// those arrive named too. `None` when no manifest name is available: a
/// repository with no `package.json`, or the resolve-only chain that
/// wires no fetch context — and hoists no peers.
fn resolved_pkg_name(result: &pnpm_resolving_resolver_base::ResolveResult) -> Option<String> {
    if let Some(name_ver) = result.name_ver.as_ref() {
        return Some(name_ver.name.to_string());
    }
    result.manifest.as_deref()?.get("name").and_then(serde_json::Value::as_str).map(str::to_string)
}

/// The picker matches by alias *and* by real package name, so a dep
/// [`resolved_pkg_name`] can't name still enters under its alias —
/// usually the package name anyway, and better than no candidate.
fn build_workspace_root_deps(
    direct: &[DirectDep],
    snapshot: &ResolvedTree,
    declared: &BTreeMap<String, String>,
    project_dir: &Path,
) -> Result<Vec<WorkspaceRootDep>, PackageManifestError> {
    let mut out = Vec::with_capacity(direct.len());
    let mut named = HashSet::default();
    for dep in direct {
        let Some(pkg) = snapshot.packages.get(dep.id.as_str()) else { continue };
        let Some(pkg_name) = resolved_pkg_name(&pkg.result) else { continue };
        named.insert(dep.alias.as_str());
        out.push(WorkspaceRootDep {
            alias: dep.alias.clone(),
            pkg_name,
            normalized_bare_specifier: pkg
                .result
                .normalized_bare_specifier
                .clone()
                .or_else(|| declared.get(&dep.alias).cloned()),
        });
    }
    for (alias, bare_specifier) in declared {
        if named.contains(alias.as_str()) {
            continue;
        }
        let (pkg_name, _) = unwrap_package_name(alias, bare_specifier);
        out.push(WorkspaceRootDep {
            alias: alias.clone(),
            pkg_name: pkg_name.to_string(),
            normalized_bare_specifier: Some(bare_specifier.clone()),
        });
    }
    for dep in &mut out {
        // Cloned so the identity can be written back onto `dep`; only the
        // handful of root deps declared with a local protocol reach here.
        let Some(spec) = dep.normalized_bare_specifier.clone() else { continue };
        if !is_project_relative_specifier(&spec) {
            continue;
        }
        // A path is never hoistable, so a target that names no version to
        // stand in for it leaves the root offering no candidate at all.
        dep.normalized_bare_specifier = None;
        if let Some(identity) = local_target_identity(&spec, project_dir)? {
            if let Some(name) = identity.name {
                dep.pkg_name = name;
            }
            dep.normalized_bare_specifier = Some(identity.version);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
