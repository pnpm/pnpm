//! Multi-importer entry point for an install pass: take every workspace
//! project the install touches, run the per-importer hoist +
//! peer-resolution loop with shared cross-importer caches, and emit the
//! combined `DependenciesGraph` plus the per-importer
//! `direct_dependencies_by_importer` map the install layer consumes.
//!
//! The cross-importer cache that matters for performance lives on the
//! peer walker (`peersCache` + `purePkgs`); making it workspace-wide
//! means an importer revisiting a `(pkgIdWithPatchHash,
//! parent-peer-context)` pair that an earlier importer already resolved
//! short-circuits straight to the cached `depPath`. Sharing the
//! `TreeCtx` resolved-pkgs map across importers is a separate axis
//! pacquet hasn't landed yet — `base_opts.project_dir` varies per
//! importer, which the existing `TreeCtx` shape ties to one importer
//! at a time. The peer-walker share captures the hot path; the
//! resolved-pkgs share is a follow-up perf win.

mod time_based;
use time_based::{TimeBasedCutoff, time_cutoff};

use crate::{
    resolve_dependency_tree::{
        ManifestHook, UpdateDepth, UpdateReuseScope, WorkspaceTreeCtx, importer_direct_wanted_specs,
    },
    resolve_importer::{ImporterHoistState, ResolveImporterError, ResolveImporterOptions},
    resolve_peers::{
        ImporterPeerInput, PeerHoistDiscovery, ResolvePeersOptions, WorkspaceResolvePeersResult,
        resolve_peers_workspace,
    },
    resolved_tree::ResolvedTree,
};
use chrono::{DateTime, Duration, Utc};
use pnpm_lockfile::RegistryContext;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_resolving_resolver_base::{Resolver, parse_packument_timestamp};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

/// One importer's input to [`fn@resolve_workspace`].
pub struct WorkspaceImporter<'a> {
    pub id: String,
    pub manifest: &'a PackageManifest,
}

/// Workspace-shared opts that don't vary per importer.
pub struct WorkspaceResolveOptions {
    pub dedupe_peers: bool,
    /// `true` enables [`fn@crate::resolve_peers_workspace`]'s cross-
    /// importer dedupe pass — `dependenciesMeta[<alias>].injected: true`
    /// workspace edges collapse back to `link:` when the injected
    /// snapshot's children are a subset of the target project's own
    /// direct deps.
    pub dedupe_injected_deps: bool,
    /// `true` enables [`fn@crate::resolve_peers_workspace`]'s peer-
    /// dependent dedupe pass — peer-suffixed variants of one package
    /// that are a subset of a larger compatible variant collapse into
    /// it. Maps to the `dedupePeerDependents` setting (default `true`).
    pub dedupe_peer_dependents: bool,
    /// When true, non-root importers can resolve peers from the
    /// workspace root's direct dependencies. Maps to the
    /// `resolvePeersFromWorkspaceRoot` setting.
    pub resolve_peers_from_workspace_root: bool,
    /// Threaded into [`ResolvePeersOptions::exclude_links_from_lockfile`]
    /// for the workspace-wide peer pass. Per-importer
    /// [`ResolvePeersOptions::modules_dir`] comes from each
    /// [`crate::ImporterPeerInput::modules_dir`].
    pub exclude_links_from_lockfile: bool,
    pub lockfile_dir: PathBuf,
    pub peers_suffix_max_length: usize,
    /// Whether named workspace resolutions may be shared across importers.
    /// When `true`, an eligible named `workspace:` request resolves once
    /// against a cache key that omits the consuming importer's `project_dir`,
    /// and the importer-relative `link:` is rendered from that canonical
    /// result afterwards. Must stay `false` whenever the resolver chain can
    /// make a resolution depend on the consuming importer beyond that
    /// rendering — a pnpmfile custom resolver above all.
    pub share_workspace_resolutions: bool,
    /// `readPackageHook` applied to every resolved manifest before it
    /// enters the wanted-dep cache. Workspace-wide (one hook per
    /// install); the install layer typically threads
    /// `packageExtensions` here. See [`ManifestHook`].
    pub manifest_hook: Option<ManifestHook>,

    /// Post-pnpmfile manifest hook (overrides). See
    /// `WorkspaceTreeCtx::overrides_hook` for the ordering contract.
    pub overrides_hook: Option<ManifestHook>,

    /// When `true`, every importer's direct dependencies are resolved
    /// to their lowest satisfying version (`resolutionMode: time-based`
    /// / `lowest-direct`). Threaded onto each
    /// [`ResolveImporterOptions::pick_lowest_direct`].
    pub pick_lowest_direct: bool,

    /// When `true` (`resolutionMode: time-based`), a pre-pass resolves
    /// every importer's direct deps to find the newest publication
    /// date, then constrains all transitive deps to versions published
    /// no later than that (plus a one-hour delta), clamped by any
    /// `minimumReleaseAge` cutoff.
    pub time_based: bool,

    /// The prior `pnpm-lock.yaml` the install started from, when one
    /// exists. Threaded into [`WorkspaceTreeCtx`] so the tree walk can
    /// reuse already-resolved dependencies instead of re-resolving them
    /// (see `pnpm/plans/LOCKFILE_RESOLUTION_REUSE.md`). `None` on a
    /// first install or when reuse is disabled.
    pub wanted_lockfile: Option<Arc<pnpm_lockfile::Lockfile>>,

    /// Whether the walk may reuse whole already-resolved subtrees from
    /// [`Self::wanted_lockfile`]. `false` keeps the lockfile as a
    /// per-edge version-pin source only: every node re-resolves against
    /// its (hook-rewritten) manifest range, and an edge whose recorded
    /// version still satisfies that range stays on it — mirroring the
    /// TypeScript resolver's forced full resolution, which forces the
    /// walk without unpinning still-satisfied edges. The config drift
    /// that denied subtree reuse stays effective: hooks rewrite the
    /// drifted manifests before the satisfies check, so the edges a
    /// changed override or extension reaches re-resolve.
    pub reuse_lockfile_subtrees: bool,

    /// Which dependencies `pacquet update` excludes from lockfile-
    /// resolution reuse. [`UpdateReuseScope::All`] for `install` / `add`.
    pub update_reuse_scope: UpdateReuseScope,

    /// Per-importer update scopes for filtered workspace updates. An importer
    /// absent from this map uses [`Self::update_reuse_scope`].
    pub update_reuse_scopes_by_importer: BTreeMap<String, UpdateReuseScope>,
    /// `pacquet update --depth`: how deep the update reaches. Nodes
    /// past the ceiling keep their locked resolutions even when their
    /// name is an update target.
    pub update_depth: UpdateDepth,

    /// `pnpmfileHook` applied to every resolved manifest before it
    /// enters the wanted-dep cache. Workspace-wide (one hook per
    /// install); wraps `readPackage` from `.pnpmfile.cjs` / `pnpmfile.cjs`.
    pub pnpmfile_hook: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,

    /// `context.log(...)` sink for the `pnpmfile_hook`'s `readPackage`
    /// calls, pre-bound to the install's reporter. `None` leaves hook
    /// logging a no-op.
    pub read_package_log: Option<pnpm_hooks::LogFn>,

    /// Sink for skipped-optional-dependency notifications, pre-bound to
    /// the install's reporter (the install layer forwards each one as a
    /// `pnpm:skipped-optional-dependency` `resolution_failure` debug
    /// log). `None` keeps the skip behavior but drops the notification.
    pub skipped_optional_log: Option<crate::SkippedOptionalLogFn>,

    /// Sink told about every package whose subtree has settled peer-free,
    /// so the install layer can materialize it into the virtual store
    /// before peer resolution. `None` skips the sweep. See
    /// [`crate::FinalizedPackageFn`].
    pub finalized_package: Option<crate::FinalizedPackageFn>,

    /// Package-name → semver-range map from the
    /// `pnpm.allowedDeprecatedVersions` setting. When a newly-resolved
    /// package is deprecated and its `name@version` satisfies an entry
    /// here, the deprecation warning is suppressed.
    pub allowed_deprecated_versions: BTreeMap<String, String>,

    /// Sink for deprecation notifications, pre-bound to the install's
    /// reporter (the install layer forwards each one as a
    /// `pnpm:deprecation` debug log). `None` keeps the deprecation
    /// check but drops the notification.
    pub deprecation_log: Option<crate::DeprecationLogFn>,

    /// The install's `autoInstallPeers` setting, threaded onto the
    /// shared [`WorkspaceTreeCtx`] so the tree walk drops
    /// peer-shadowed `dependencies` entries. Also overrides every
    /// per-importer
    /// [`crate::ResolveImporterOptions::auto_install_peers`] — the
    /// setting is workspace-wide.
    pub auto_install_peers: bool,
    /// How a package's registry is decided and what it serves: the scope
    /// map, the named-registry aliases (built-ins merged with the user's
    /// setting), and the per-registry settings. Used to materialize a
    /// prior `Registry` lockfile resolution back into its tarball URL when
    /// building the `currentPkg` payload custom resolvers receive.
    pub registry_context: RegistryContext,
}

/// Result of [`fn@resolve_workspace`]. The combined
/// [`WorkspaceResolvePeersResult`] holds the cross-importer graph + the
/// per-importer `direct_dependencies_by_alias` map; `merged_tree`
/// carries the shared `ResolvedTree` snapshot the workspace ctx
/// produced after every importer's walk folded into the shared maps.
pub struct ResolveWorkspaceResult {
    pub merged_tree: ResolvedTree,
    pub peers: WorkspaceResolvePeersResult,
    /// Publish date of every direct dependency, for the lockfile's
    /// `time:` section. Empty unless the install ran `time-based`.
    pub time: BTreeMap<String, String>,
}

/// Resolve every importer's dependencies, then run one workspace-wide
/// peer-resolution + dedupe pass.
///
/// `per_importer_options` is invoked per importer to build that
/// importer's own [`ResolveImporterOptions`] — the install layer owns
/// the per-importer wiring (project dir, modules dir, lockfile dir,
/// exclude-links-from-lockfile, etc.).
pub async fn resolve_workspace<'a, Chain, BuildImporterOptions>(
    resolver: &Chain,
    importers: &[WorkspaceImporter<'a>],
    dependency_groups: &[DependencyGroup],
    opts: WorkspaceResolveOptions,
    per_importer_options: BuildImporterOptions,
) -> Result<ResolveWorkspaceResult, ResolveImporterError>
where
    Chain: Resolver + ?Sized,
    BuildImporterOptions: FnMut(&WorkspaceImporter<'a>) -> ResolveImporterOptions,
{
    let (workspace, settings) = opts.split();
    let sorted = sorted_importers(importers, per_importer_options, &settings);
    let cutoff = time_cutoff(resolver, &sorted, dependency_groups, &settings).await;
    let mut initialized =
        init_importers(resolver, sorted, dependency_groups, &cutoff, &settings, &workspace).await?;
    run_hoist_rounds(resolver, &mut initialized.states, &workspace).await?;
    Ok(finish(&settings, workspace, initialized, cutoff.time))
}

/// What the pass keeps for itself once the shared tree context has
/// taken the hooks, logs and lockfile.
struct PassSettings {
    dedupe_peers: bool,
    dedupe_injected_deps: bool,
    dedupe_peer_dependents: bool,
    resolve_peers_from_workspace_root: bool,
    exclude_links_from_lockfile: bool,
    lockfile_dir: PathBuf,
    peers_suffix_max_length: usize,
    pick_lowest_direct: bool,
    time_based: bool,
    auto_install_peers: bool,
    /// The lockfile's recorded publish dates, taken only for the
    /// `time-based` pre-pass that reads them — a lockfile is untrusted
    /// input, so an install that will not consult the dates must not
    /// copy them.
    recorded_time: Option<BTreeMap<String, String>>,
}

impl WorkspaceResolveOptions {
    fn split(self) -> (Arc<WorkspaceTreeCtx>, PassSettings) {
        let recorded_time = self
            .time_based
            .then(|| self.wanted_lockfile.as_ref().and_then(|lockfile| lockfile.time.clone()))
            .flatten();
        let settings = PassSettings {
            dedupe_peers: self.dedupe_peers,
            dedupe_injected_deps: self.dedupe_injected_deps,
            dedupe_peer_dependents: self.dedupe_peer_dependents,
            resolve_peers_from_workspace_root: self.resolve_peers_from_workspace_root,
            exclude_links_from_lockfile: self.exclude_links_from_lockfile,
            lockfile_dir: self.lockfile_dir,
            peers_suffix_max_length: self.peers_suffix_max_length,
            pick_lowest_direct: self.pick_lowest_direct,
            time_based: self.time_based,
            auto_install_peers: self.auto_install_peers,
            recorded_time,
        };
        let workspace = WorkspaceTreeCtx::default()
            .with_shared_workspace_resolutions(self.share_workspace_resolutions)
            .with_manifest_hook(self.manifest_hook)
            .with_overrides_hook(self.overrides_hook)
            .with_wanted_lockfile(self.wanted_lockfile)
            .with_reuse_lockfile_subtrees(self.reuse_lockfile_subtrees)
            .with_update_reuse_scope(self.update_reuse_scope)
            .with_update_reuse_scopes_by_importer(self.update_reuse_scopes_by_importer)
            .with_update_depth(self.update_depth)
            .with_pnpmfile_hook(self.pnpmfile_hook)
            .with_read_package_log(self.read_package_log)
            .with_skipped_optional_log(self.skipped_optional_log)
            .with_finalized_package(self.finalized_package)
            .with_allowed_deprecated_versions(self.allowed_deprecated_versions)
            .with_deprecation_log(self.deprecation_log)
            .with_auto_install_peers(self.auto_install_peers)
            .with_registry_context(self.registry_context);
        (Arc::new(workspace), settings)
    }
}

/// The importers in id order, each with its options.
struct SortedImporters<'i, 'a> {
    importers: Vec<&'i WorkspaceImporter<'a>>,
    opts: Vec<ResolveImporterOptions>,
}

/// Build every importer's options up front so the `time-based` pre-pass
/// and the resolve loop see the same per-importer wiring.
/// `auto_install_peers` and `dedupe_peer_dependents` are workspace-wide
/// (one setting per install), so the workspace-level values override
/// whatever the per-importer callback set — the importer hoist loop and
/// the tree walk's shadow pruning must agree.
///
/// Sorted by importer id: children-owner claims are ranked by importer
/// position and the hoist rounds run sequentially in list order, so a
/// stable order makes ownership, the first-walk missing scope, and every
/// auto-install decision a function of the importer set rather than of
/// the caller's listing order (pnpm/pnpm#13846).
fn sorted_importers<'i, 'a, BuildImporterOptions>(
    importers: &'i [WorkspaceImporter<'a>],
    mut per_importer_options: BuildImporterOptions,
    settings: &PassSettings,
) -> SortedImporters<'i, 'a>
where
    BuildImporterOptions: FnMut(&WorkspaceImporter<'a>) -> ResolveImporterOptions,
{
    let mut paired: Vec<(&WorkspaceImporter<'a>, ResolveImporterOptions)> = importers
        .iter()
        .map(|importer| {
            let mut opts = per_importer_options(importer);
            opts.auto_install_peers = settings.auto_install_peers;
            opts.dedupe_peer_dependents = settings.dedupe_peer_dependents;
            (importer, opts)
        })
        .collect();
    paired.sort_by(|(left, _), (right, _)| left.id.cmp(&right.id));
    let (importers, opts) = paired.into_iter().unzip();
    SortedImporters { importers, opts }
}

struct InitializedImporters<'i, 'a> {
    importers: Vec<&'i WorkspaceImporter<'a>>,
    states: Vec<ImporterHoistState>,
    /// Each importer's project and modules dir, for its peer input.
    input_dirs: Vec<(PathBuf, Option<PathBuf>)>,
}

/// Phase 1: every importer's initial wave resolves before any peer
/// hoist runs, then hoist rounds repeat across all importers until
/// none hoists — a workspace-wide barrier, so an optional-peer pick
/// sees every importer's resolved versions.
///
/// The initial waves run concurrently, like the TypeScript resolver's
/// importer fan-out: the shared context's children-owner claims are
/// rank-ordered (not arrival-ordered) and the peer-hoist pickers'
/// preferred-version candidates are derived from the settled
/// reachable tree (see `WorkspaceTreeCtx::run_preferred_versions`),
/// so the resolved graph is the same regardless of interleaving, and
/// a large workspace's walks overlap their resolver and hook waits
/// instead of paying them importer by importer.
async fn init_importers<'i, 'a, Chain>(
    resolver: &Chain,
    sorted: SortedImporters<'i, 'a>,
    dependency_groups: &[DependencyGroup],
    cutoff: &TimeBasedCutoff,
    settings: &PassSettings,
    workspace: &Arc<WorkspaceTreeCtx>,
) -> Result<InitializedImporters<'i, 'a>, ResolveImporterError>
where
    Chain: Resolver + ?Sized,
{
    let mut input_dirs = Vec::with_capacity(sorted.importers.len());
    let mut states = Vec::with_capacity(sorted.importers.len());
    for (importer_order, (importer, mut importer_opts)) in
        sorted.importers.iter().zip(sorted.opts).enumerate()
    {
        importer_opts.pick_lowest_direct = settings.pick_lowest_direct;
        importer_opts.subdep_published_by = cutoff.published_by;
        input_dirs
            .push((importer_opts.base_opts.project_dir.clone(), importer_opts.modules_dir.clone()));
        // Boxed to keep the enclosing install future small: inlining a
        // wave's frame into it trips the workspace's large-future lint.
        states.push(
            Box::pin(ImporterHoistState::init(
                resolver,
                &importer.id,
                importer_order,
                importer.manifest,
                dependency_groups.iter().copied(),
                importer_opts,
                Arc::clone(workspace),
            ))
            .await?,
        );
    }
    share_root_deps(&mut states)?;
    Ok(InitializedImporters { importers: sorted.importers, states, input_dirs })
}

/// Computed after the init barrier and shared unchanged: recomputing it
/// per round would let the root's own hoisted peers become candidates
/// for the importers hoisted after it.
fn share_root_deps(states: &mut [ImporterHoistState]) -> Result<(), ResolveImporterError> {
    let root_deps = Arc::new(
        states
            .iter()
            .find(|state| state.importer_id() == pnpm_lockfile::Lockfile::ROOT_IMPORTER_KEY)
            .map(ImporterHoistState::hoistable_root_deps)
            .transpose()?
            .unwrap_or_default(),
    );
    for state in states.iter_mut() {
        state.set_workspace_root_deps(Arc::clone(&root_deps));
    }
    Ok(())
}

/// One discovery engine serves every hoist round of the workspace: its
/// persistent tree view + walker caches are what keep the barrier
/// linear in workspace size (each importer's pass short-circuits on the
/// subtree verdicts recorded by the passes before it). The engine's
/// tree view is released on return, before the merged-tree snapshot
/// clones the context again, so the two never coexist at peak.
async fn run_hoist_rounds<Chain>(
    resolver: &Chain,
    states: &mut [ImporterHoistState],
    workspace: &WorkspaceTreeCtx,
) -> Result<(), ResolveImporterError>
where
    Chain: Resolver + ?Sized,
{
    let mut peer_discovery = PeerHoistDiscovery::new();
    run_initial_required_rounds(resolver, states, workspace, &mut peer_discovery).await?;
    run_hoist_barrier(resolver, states, &mut peer_discovery).await
}

/// Fold every importer's direct deps into the peer inputs, reclaim the
/// tree and run the workspace-wide peer pass.
fn finish(
    settings: &PassSettings,
    workspace: Arc<WorkspaceTreeCtx>,
    initialized: InitializedImporters<'_, '_>,
    time: BTreeMap<String, String>,
) -> ResolveWorkspaceResult {
    let peer_inputs = importer_peer_inputs(initialized);
    // Reclaim the workspace ctx now that every importer's state has
    // dropped its `Arc<WorkspaceTreeCtx>`. The `try_unwrap` succeeds
    // when this is the sole remaining `Arc` reference (the common
    // case); the fallback snapshots out via the shared `Arc` for
    // parity.
    let mut merged_tree = match Arc::try_unwrap(workspace) {
        Ok(ws) => ws.into_resolved_tree(Vec::new()),
        Err(arc) => arc.snapshot(Vec::new()),
    };
    let peers = resolve_workspace_peers(settings, &mut merged_tree, peer_inputs);
    ResolveWorkspaceResult { merged_tree, peers, time }
}

struct PeerInputs {
    per_importer: Vec<ImporterPeerInput>,
    hoisted_provider_node_ids: std::collections::HashSet<crate::NodeId, rustc_hash::FxBuildHasher>,
}

fn importer_peer_inputs(initialized: InitializedImporters<'_, '_>) -> PeerInputs {
    let mut per_importer = Vec::with_capacity(initialized.importers.len());
    let mut hoisted_provider_node_ids = std::collections::HashSet::default();
    for ((importer, state), (project_dir, modules_dir)) in
        initialized.importers.iter().zip(initialized.states).zip(initialized.input_dirs)
    {
        let (direct, importer_provider_node_ids) = state.into_direct();
        hoisted_provider_node_ids.extend(importer_provider_node_ids);
        per_importer.push(ImporterPeerInput {
            id: importer.id.clone(),
            direct,
            root_dir: project_dir,
            modules_dir,
        });
    }
    PeerInputs { per_importer, hoisted_provider_node_ids }
}

fn resolve_workspace_peers(
    settings: &PassSettings,
    tree: &mut ResolvedTree,
    inputs: PeerInputs,
) -> WorkspaceResolvePeersResult {
    resolve_peers_workspace(
        tree,
        &inputs.per_importer,
        &settings.lockfile_dir,
        settings.dedupe_injected_deps,
        settings.dedupe_peer_dependents,
        settings.resolve_peers_from_workspace_root,
        ResolvePeersOptions {
            peers_suffix_max_length: settings.peers_suffix_max_length,
            dedupe_peers: settings.dedupe_peers,
            exclude_links_from_lockfile: settings.exclude_links_from_lockfile,
            lockfile_dir: Some(settings.lockfile_dir.clone()),
            project_dir: None,
            // Per-importer; resolve_peers_workspace swaps the
            // ImporterPeerInput's modules_dir into walker.opts before each
            // importer's walk.
            modules_dir: None,
            hoist_missing_scope: None,
            hoisted_peer_provider_node_ids: inputs.hoisted_provider_node_ids,
            ..ResolvePeersOptions::default()
        },
    )
}

/// The first required round of every importer, prepared against one quiescent
/// snapshot of the owner-scope maps and then completed. The context is
/// quiescent between the prepare barrier and the completes, so the single
/// snapshot serves every importer.
async fn run_initial_required_rounds<Chain>(
    resolver: &Chain,
    states: &mut [ImporterHoistState],
    workspace: &WorkspaceTreeCtx,
    peer_discovery: &mut PeerHoistDiscovery,
) -> Result<(), ResolveImporterError>
where
    Chain: Resolver + ?Sized,
{
    let mut rounds: Vec<_> = states
        .iter_mut()
        .map(|state| state.prepare_initial_required_round(peer_discovery))
        .collect();
    let first_importer_by_pkg = workspace.first_importer_by_pkg();
    let first_walk_missing_by_pkg = workspace.first_walk_missing_by_pkg();
    for (state, round) in states.iter().zip(rounds.iter_mut().flatten()) {
        state.apply_owner_missing_scope(round, &first_importer_by_pkg, &first_walk_missing_by_pkg);
    }
    for (state, round) in
        states.iter_mut().zip(rounds).filter_map(|(state, round)| round.map(|round| (state, round)))
    {
        state.complete_initial_required_round(resolver, round, peer_discovery).await?;
    }
    Ok(())
}

/// Repeat optional-peer hoist rounds across every importer until none hoists,
/// re-running the required rounds after each wave. A workspace-wide barrier,
/// so an optional-peer pick sees every importer's resolved versions.
async fn run_hoist_barrier<Chain>(
    resolver: &Chain,
    states: &mut [ImporterHoistState],
    peer_discovery: &mut PeerHoistDiscovery,
) -> Result<(), ResolveImporterError>
where
    Chain: Resolver + ?Sized,
{
    loop {
        let mut any_hoisted = false;
        for state in &mut *states {
            any_hoisted |= state.hoist_optional_round(resolver).await?;
        }
        if !any_hoisted {
            return Ok(());
        }
        for state in &mut *states {
            state.run_required_round(resolver, peer_discovery).await?;
        }
    }
}

#[cfg(test)]
mod tests;
