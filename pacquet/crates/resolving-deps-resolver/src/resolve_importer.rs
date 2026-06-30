//! Port of pnpm's
//! [`resolveRootDependencies`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L327-L437)
//! — the multi-pass loop that hoists missing peers into the importer's
//! direct deps until no required peer is missing and no optional peer
//! is satisfiable from the in-flight preferred-versions map.
//!
//! Two nested fixed-point loops:
//!
//! 1. **Inner / required pass.** Run [`fn@crate::resolve_peers`] over the
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
        HoistPeersOptions, MissingPeerInfo, WorkspaceRootDep, get_hoistable_optional_peers,
        hoist_peers,
    },
    resolve_dependency_tree::{
        ResolveDependencyTreeError, TreeCtx, WantedSpec, WorkspaceTreeCtx, extend_tree,
        importer_direct_wanted_specs,
    },
    resolve_peers::{ResolvePeersOptions, ResolvePeersResult, resolve_peers},
    resolved_tree::ResolvedTree,
};
use chrono::{DateTime, Utc};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_catalogs_types::Catalogs;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_patching::PatchGroupRecord;
use pacquet_resolving_resolver_base::{
    PreferredVersions, ResolveOptions, Resolver, VersionSelectorEntry, VersionSelectorType,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
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
    /// failure. Mirrors pnpm's `autoInstallPeersFromHighestMatch`.
    pub auto_install_peers_from_highest_match: bool,

    /// When true, the importer's direct deps are used as
    /// `workspace_root_deps` for the hoist picker so a peer matching a
    /// direct dep's alias / name short-circuits straight to that
    /// dep's specifier. Single-importer pacquet treats the importer
    /// as the root.
    pub resolve_peers_from_workspace_root: bool,

    /// Threaded into [`ResolvePeersOptions::dedupe_peers`] on every
    /// `resolve_peers` invocation inside the auto-install-peers loop.
    /// See the field doc on [`ResolvePeersOptions`] for the behavior.
    pub dedupe_peers: bool,

    /// Seed for the preferred-versions tie-break table. The
    /// orchestrator extends this in place as packages are walked —
    /// each newly-resolved `name@version` lands as a plain
    /// [`VersionSelectorType::Version`] entry so the [`hoist_peers`]
    /// (required-peer) picker can reuse a version a sibling already
    /// brought. The [`get_hoistable_optional_peers`] picker instead
    /// reads a snapshot taken *before* any run-resolved version is
    /// folded in — mirroring upstream's static
    /// [`ctx.allPreferredVersions`](https://github.com/pnpm/pnpm/blob/894ea6af2c/installing/deps-resolver/src/resolveDependencies.ts#L340-L342)
    /// — so an optional peer is never hoisted against a deep-tree
    /// provider pnpm can't see. Pass the result of
    /// `get_preferred_versions_from_lockfile_and_manifests` from the
    /// `lockfile-preferred-versions` crate, or an empty map when no
    /// lockfile + manifest seeding is available.
    pub all_preferred_versions: PreferredVersions,

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
    /// Mirrors pnpm's
    /// [`pickLowestVersion`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/installing/deps-resolver/src/resolveDependencies.ts#L470)
    /// for importer deps.
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

    /// Catalogs parsed from `pnpm-workspace.yaml`. Applied only to the
    /// importer's direct dependencies; transitive `catalog:` entries
    /// are not resolved through the catalog, matching upstream's
    /// [importer-only catalog scope](https://github.com/pnpm/pnpm/blob/a8a8cbce6d/installing/deps-resolver/src/resolveDependencies.ts#L592-L600).
    pub catalogs: Catalogs,

    /// When `true`, `link:` direct deps whose target lives outside
    /// the lockfile root are seeded into the peer-resolution parent
    /// map with a remapped node id
    /// (`link:<rel-from-lockfile_dir-to-modules_dir>/<alias>`) so the
    /// peer suffix stays stable across machines. Mirrors pnpm's
    /// [`excludeLinksFromLockfile`](https://github.com/pnpm/pnpm/blob/094aa6e57b/installing/deps-resolver/src/index.ts#L232-L244)
    /// flow. The remap fires only when [`Self::lockfile_dir`] and
    /// [`Self::modules_dir`] are both set.
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
    /// [`ResolvePeersOptions`]. Mirrors upstream's
    /// `peersSuffixMaxLength` (default 1000).
    pub peers_suffix_max_length: usize,

    pub catalog_server: bool,

    /// `readPackageHook` applied to every resolved manifest before
    /// downstream consumers see it. Today drives `packageExtensions`;
    /// see [`crate::ManifestHook`].
    pub manifest_hook: Option<crate::ManifestHook>,

    /// `pnpmfileHook` applied to every resolved manifest. Wraps
    /// `readPackage` from `.pnpmfile.cjs` / `pnpmfile.cjs`.
    pub pnpmfile_hook: Option<Arc<dyn pacquet_hooks::PnpmfileHooks>>,
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
            .field("all_preferred_versions", &self.all_preferred_versions)
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
}

impl From<ResolveDependencyTreeError> for ResolveImporterError {
    fn from(err: ResolveDependencyTreeError) -> Self {
        ResolveImporterError::Resolve(err)
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
            .with_pnpmfile_hook(opts.pnpmfile_hook.clone())
            .with_auto_install_peers(opts.auto_install_peers),
    );
    resolve_importer_with_workspace(
        resolver,
        pacquet_lockfile::Lockfile::ROOT_IMPORTER_KEY,
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
    loop {
        state.run_required_round(resolver).await?;
        if !state.hoist_optional_round(resolver).await? {
            break;
        }
    }
    Ok(state.into_result())
}

/// One importer's resolution state across the workspace's hoist
/// rounds. The multi-importer orchestrator
/// [`fn@crate::resolve_workspace`] initializes every importer before
/// running any hoist round, mirroring upstream's
/// [`resolveRootDependencies`](https://github.com/pnpm/pnpm/blob/ce9c096e8e/installing/deps-resolver/src/resolveDependencies.ts#L335-L445)
/// barrier: every importer's initial wave completes first, then
/// per-round required-peer loops and one optional-peer hoist per
/// round repeat across all importers until no importer hoists — so an
/// optional-peer pick sees every importer's resolved versions, not
/// just the importers processed so far.
pub(crate) struct ImporterHoistState {
    importer_id: String,
    ctx: TreeCtx,
    direct: Vec<DirectDep>,
    parent_pkg_aliases: HashSet<String>,
    all_missing_optional_peers: BTreeMap<String, Vec<String>>,
    all_preferred_versions: PreferredVersions,
    auto_install_peers: bool,
    auto_install_peers_from_highest_match: bool,
    resolve_peers_from_workspace_root: bool,
    peers_suffix_max_length: usize,
    dedupe_peers: bool,
    exclude_links_from_lockfile: bool,
    lockfile_dir: Option<std::path::PathBuf>,
    modules_dir: Option<std::path::PathBuf>,
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
            all_preferred_versions,
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
            // `manifest_hook` and `pnpmfile_hook` are workspace-wide; they live
            // on the shared [`WorkspaceTreeCtx`] and the caller (`resolve_importer`
            // or `resolve_workspace`) is responsible for setting them there before
            // handing the `Arc` to this function.
            manifest_hook: _,
            pnpmfile_hook: _,
        } = opts;

        let ctx = TreeCtx::with_workspace(workspace, base_opts)
            .with_importer_id(importer_id)
            .with_importer_order(importer_order)
            .with_patched_dependencies(patched_dependencies)
            .with_resolution_mode(pick_lowest_direct, subdep_published_by);

        let initial_wanted = importer_direct_wanted_specs(
            manifest,
            dependency_groups,
            auto_install_peers,
            &catalogs,
        )?;
        let direct = extend_tree(&ctx, resolver, initial_wanted, importer_id).await?;
        let parent_pkg_aliases: HashSet<String> =
            direct.iter().map(|dep| dep.alias.clone()).collect();
        Ok(ImporterHoistState {
            importer_id: importer_id.to_string(),
            ctx,
            direct,
            parent_pkg_aliases,
            all_missing_optional_peers: BTreeMap::new(),
            all_preferred_versions,
            auto_install_peers,
            auto_install_peers_from_highest_match,
            resolve_peers_from_workspace_root,
            peers_suffix_max_length,
            dedupe_peers,
            exclude_links_from_lockfile,
            lockfile_dir,
            modules_dir,
        })
    }

    fn peers_opts(&self) -> ResolvePeersOptions {
        ResolvePeersOptions {
            peers_suffix_max_length: self.peers_suffix_max_length,
            dedupe_peers: self.dedupe_peers,
            exclude_links_from_lockfile: self.exclude_links_from_lockfile,
            lockfile_dir: self.lockfile_dir.clone(),
            modules_dir: self.modules_dir.clone(),
            hoist_missing_scope: None,
        }
    }

    /// Resolve the importer's missing *required* peers to a fixpoint,
    /// rebuilding the missing-*optional* buckets the round's
    /// [`Self::hoist_optional_round`] consumes. Upstream's inner
    /// `while` per importer per round.
    pub(crate) async fn run_required_round<Chain>(
        &mut self,
        resolver: &Chain,
    ) -> Result<(), ResolveImporterError>
    where
        Chain: Resolver + ?Sized,
    {
        // Both hoists read the run-extended preferred-versions map:
        // upstream folds every resolved package's version into
        // `ctx.allPreferredVersions`
        // ([`resolveDependencies.ts#L1483-L1488`](https://github.com/pnpm/pnpm/blob/01b3d45ddb/installing/deps-resolver/src/resolveDependencies.ts#L1483-L1488))
        // and consults it for the optional hoist after each wave.
        // Refreshed per round so it carries every importer's versions,
        // not just this importer's.
        update_preferred_versions_with_ctx(&self.ctx, &mut self.all_preferred_versions);
        // The hoist input must not see missing peers declared inside a
        // subtree owned by another importer's shared children context —
        // upstream reuses the owner walk's children report there, so
        // those peers never reach a non-owner importer's hoist. The
        // final peer pass keeps the unscoped options so warnings stay
        // complete.
        self.all_missing_optional_peers.clear();
        loop {
            let mut snapshot = self.ctx.snapshot(self.direct.clone());
            let original_node_ids: HashSet<crate::NodeId> =
                snapshot.dependencies_tree.keys().cloned().collect();
            let peers_result = {
                let mut opts = self.peers_opts();
                // Re-snapshotted per peer pass because auto-hoisting
                // can extend the tree and move children ownership to a
                // shallower occurrence.
                opts.hoist_missing_scope =
                    Some(Arc::new(crate::resolve_peers::HoistMissingScope {
                        importer_id: self.importer_id.clone(),
                        first_importer_by_pkg: self.ctx.workspace().first_importer_by_pkg(),
                        first_walk_missing_by_pkg: self.ctx.workspace().first_walk_missing_by_pkg(),
                    }));
                resolve_peers(&mut snapshot, opts)
            };
            self.ctx
                .workspace()
                .record_first_walk_missing(&self.importer_id, &peers_result.missing_names_by_pkg);

            let (missing_required, fresh_optional) = partition_missing_peers(
                &peers_result.peer_dependency_issues.missing,
                &self.parent_pkg_aliases,
                self.auto_install_peers_from_highest_match,
            );
            self.append_resolved_peer_providers(
                &peers_result.resolved_peer_providers_by_alias,
                &snapshot,
                &original_node_ids,
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
                break;
            }

            let workspace_root_deps = if self.resolve_peers_from_workspace_root {
                build_workspace_root_deps(&self.direct, &snapshot)
            } else {
                Vec::new()
            };

            let missing_as_pairs: Vec<(String, MissingPeerInfo)> =
                missing_required.iter().map(|(n, info)| (n.clone(), info.clone())).collect();
            let hoisted = hoist_peers(
                &HoistPeersOptions {
                    auto_install_peers: self.auto_install_peers,
                    all_preferred_versions: &self.all_preferred_versions,
                    workspace_root_deps: &workspace_root_deps,
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
            // auto-install. Mirrors upstream's `wantedDependency`
            // shape inside `hoistPeers`. Hoisted peers don't carry
            // `dependenciesMeta` from any manifest, so `injected`
            // defaults to `false` — matches upstream where the hoist
            // path constructs a fresh `WantedDependency` without
            // threading the per-dep meta.
            let new_wanted: Vec<WantedSpec> =
                hoisted.into_iter().map(|(name, range)| (name, range, false, false)).collect();
            let new_direct =
                extend_tree(&self.ctx, resolver, new_wanted, &self.importer_id).await?;
            self.direct.extend(new_direct);
            update_preferred_versions_with_ctx(&self.ctx, &mut self.all_preferred_versions);
        }
        Ok(())
    }

    fn append_resolved_peer_providers(
        &mut self,
        providers: &BTreeMap<String, crate::NodeId>,
        snapshot: &ResolvedTree,
        original_node_ids: &HashSet<crate::NodeId>,
        missing_required: &BTreeMap<String, MissingPeerInfo>,
    ) {
        if !self.auto_install_peers {
            return;
        }
        for (alias, node_id) in providers {
            if self.parent_pkg_aliases.contains(alias) || missing_required.contains_key(alias) {
                continue;
            }
            if !original_node_ids.contains(node_id) {
                continue;
            }
            let Some(tree_node) = snapshot.dependencies_tree.get(node_id) else { continue };
            if !snapshot.packages.contains_key(&tree_node.resolved_package_id) {
                continue;
            }
            self.direct.push(DirectDep {
                alias: alias.clone(),
                node_id: node_id.clone(),
                id: tree_node.resolved_package_id.clone(),
            });
            self.parent_pkg_aliases.insert(alias.clone());
        }
    }

    /// Hoist this round's missing optional peers; `true` when any were
    /// installed (the workspace runs another round). Upstream's
    /// `getHoistableOptionalPeers` arm per importer per round.
    pub(crate) async fn hoist_optional_round<Chain>(
        &mut self,
        resolver: &Chain,
    ) -> Result<bool, ResolveImporterError>
    where
        Chain: Resolver + ?Sized,
    {
        if self.all_missing_optional_peers.is_empty() {
            return Ok(false);
        }
        let hoisted_optional = get_hoistable_optional_peers(
            &self.all_missing_optional_peers,
            &self.all_preferred_versions,
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
        let new_direct = extend_tree(&self.ctx, resolver, new_wanted, &self.importer_id).await?;
        self.direct.extend(new_direct);
        update_preferred_versions_with_ctx(&self.ctx, &mut self.all_preferred_versions);
        Ok(true)
    }

    /// The importer's direct-dep envelopes for the workspace-wide peer
    /// pass (which recomputes peers across importers; the per-importer
    /// pass would be discarded).
    pub(crate) fn into_direct(self) -> Vec<DirectDep> {
        self.direct
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

/// Split the missing-peer report into the inputs the inner and outer
/// loops consume.
///
/// A peer name is **required** for this iteration when at least one of
/// its consumers declared it non-optional and it isn't already in
/// `parent_pkg_aliases` (i.e. not already a direct dep that just
/// hadn't been added to the alias set yet). Its merged range follows
/// upstream's [`mergePkgsDeps`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L796-L818):
/// single-range cases pass through, multi-range cases intersect via a
/// stub (see [`merge_ranges`]) and fall through to `||`-join when
/// `auto_install_peers_from_highest_match` is set.
///
/// Peers whose consumers are *all* optional are returned as the second
/// component, keyed by peer name with the deduplicated range list the
/// outer loop's [`get_hoistable_optional_peers`] needs.
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
        let required_ranges: Vec<&str> = entries
            .iter()
            .filter(|entry| !entry.optional)
            .map(|entry| entry.wanted_range.as_str())
            .collect();
        if required_ranges.is_empty() {
            let mut seen: BTreeSet<String> = BTreeSet::new();
            let mut ordered: Vec<String> = Vec::new();
            for entry in entries {
                if entry.meta_only {
                    continue;
                }
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

/// Combine multiple consumers' wanted ranges into a single specifier.
/// Single-range cases pass through unchanged. Multi-range cases that
/// reduce to one unique string also pass through. Anything else returns
/// `Some(joined)` when `auto_install_peers_from_highest_match` is set,
/// or `None` otherwise — same drop-on-conflict shape as upstream's
/// `mergePkgsDeps` when `safeIntersect` returns null.
///
/// Pacquet's `safe_intersect` stand-in is exact only for the
/// "single unique range" case; broader semver-range intersection
/// would need a port of the `semver-range-intersect` npm package.
/// In practice the multi-consumer non-identical case is rare enough
/// that conservative "no merge" matches upstream's `intersection ===
/// null` arm for the slice.
fn merge_ranges(ranges: &[&str], auto_install_peers_from_highest_match: bool) -> Option<String> {
    if ranges.len() == 1 {
        return Some(ranges[0].to_string());
    }
    let unique: BTreeSet<&&str> = ranges.iter().collect();
    if unique.len() == 1 {
        return Some(ranges[0].to_string());
    }
    if auto_install_peers_from_highest_match {
        return Some(ranges.join(" || "));
    }
    None
}

/// Build the [`WorkspaceRootDep`] slice the hoist picker sees. Reads
/// `direct` for the importer's slot names and `snapshot` for the
/// resolved package each slot points at — `normalized_bare_specifier`
/// passes through from the resolver verbatim.
fn build_workspace_root_deps(
    direct: &[DirectDep],
    snapshot: &ResolvedTree,
) -> Vec<WorkspaceRootDep> {
    let mut out = Vec::with_capacity(direct.len());
    for dep in direct {
        let Some(pkg) = snapshot.packages.get(&dep.id) else { continue };
        // `name_ver` is `None` for resolvers that learn the name from
        // the manifest only after the fetch (git / tarball / local).
        // Workspace-root short-circuit needs the resolution-time real
        // name, so skip those — they fall through to the
        // preferred-versions arm of `hoist_peers` instead.
        let Some(name_ver) = pkg.result.name_ver.as_ref() else { continue };
        out.push(WorkspaceRootDep {
            alias: dep.alias.clone(),
            pkg_name: name_ver.name.to_string(),
            normalized_bare_specifier: pkg.result.normalized_bare_specifier.clone(),
        });
    }
    out
}

/// Add every newly-resolved `name@version` from `ctx` to
/// `preferred` as a plain [`VersionSelectorType::Version`] entry,
/// mirroring upstream's per-resolve `allPreferredVersions[name][version]
/// = 'version'` assignment at
/// [`resolveDependencies.ts:1440`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L1440).
/// Idempotent: only inserts when no entry exists for `(name, version)`.
fn update_preferred_versions_with_ctx(ctx: &TreeCtx, preferred: &mut PreferredVersions) {
    for (name, version) in ctx.resolved_versions() {
        let bucket = preferred.entry(name).or_default();
        bucket.entry(version).or_insert(VersionSelectorEntry::Plain(VersionSelectorType::Version));
    }
}

#[cfg(test)]
mod tests;
