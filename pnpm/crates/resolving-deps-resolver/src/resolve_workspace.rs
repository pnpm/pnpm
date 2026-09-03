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
use pnpm_resolving_resolver_base::{Resolver, WantedDependency, parse_packument_timestamp};
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
    mut per_importer_options: BuildImporterOptions,
) -> Result<ResolveWorkspaceResult, ResolveImporterError>
where
    Chain: Resolver + ?Sized,
    BuildImporterOptions: FnMut(&WorkspaceImporter<'a>) -> ResolveImporterOptions,
{
    let WorkspaceResolveOptions {
        dedupe_peers,
        dedupe_injected_deps,
        dedupe_peer_dependents,
        resolve_peers_from_workspace_root,
        exclude_links_from_lockfile,
        lockfile_dir,
        peers_suffix_max_length,
        share_workspace_resolutions,
        manifest_hook,
        overrides_hook,
        pnpmfile_hook,
        read_package_log,
        skipped_optional_log,
        finalized_package,
        allowed_deprecated_versions,
        deprecation_log,
        pick_lowest_direct,
        time_based,
        wanted_lockfile,
        reuse_lockfile_subtrees,
        update_reuse_scope,
        update_reuse_scopes_by_importer,
        update_depth,
        auto_install_peers,
        registry_context,
    } = opts;
    // Taken before the lockfile moves into the workspace ctx below, and
    // only for the pre-pass that reads it — a lockfile is untrusted
    // input, so an install that will not consult the recorded dates
    // must not copy them.
    let recorded_time = time_based
        .then(|| wanted_lockfile.as_ref().and_then(|lockfile| lockfile.time.clone()))
        .flatten();
    let workspace = Arc::new(
        WorkspaceTreeCtx::default()
            .with_shared_workspace_resolutions(share_workspace_resolutions)
            .with_manifest_hook(manifest_hook)
            .with_overrides_hook(overrides_hook)
            .with_wanted_lockfile(wanted_lockfile)
            .with_reuse_lockfile_subtrees(reuse_lockfile_subtrees)
            .with_update_reuse_scope(update_reuse_scope)
            .with_update_reuse_scopes_by_importer(update_reuse_scopes_by_importer)
            .with_update_depth(update_depth)
            .with_pnpmfile_hook(pnpmfile_hook)
            .with_read_package_log(read_package_log)
            .with_skipped_optional_log(skipped_optional_log)
            .with_finalized_package(finalized_package)
            .with_allowed_deprecated_versions(allowed_deprecated_versions)
            .with_deprecation_log(deprecation_log)
            .with_auto_install_peers(auto_install_peers)
            .with_registry_context(registry_context),
    );

    // Build every importer's options up front so the `time-based`
    // pre-pass and the resolve loop see the same per-importer wiring.
    // `auto_install_peers` and `dedupe_peer_dependents` are
    // workspace-wide (one setting per install), so the workspace-level
    // values override whatever the per-importer callback set — the
    // importer hoist loop and the tree walk's shadow pruning must agree.
    let importer_opts: Vec<ResolveImporterOptions> = importers
        .iter()
        .map(&mut per_importer_options)
        .map(|mut opts| {
            opts.auto_install_peers = auto_install_peers;
            opts.dedupe_peer_dependents = dedupe_peer_dependents;
            opts
        })
        .collect();

    // Resolve importers in id order. Children-owner claims are ranked
    // by importer position and the hoist rounds run sequentially in
    // list order, so a stable order makes ownership, the first-walk
    // missing scope, and every auto-install decision a function of the
    // importer set rather than of the caller's listing order
    // (pnpm/pnpm#13846).
    let (importers, importer_opts): (Vec<&WorkspaceImporter<'a>>, Vec<ResolveImporterOptions>) = {
        let mut paired: Vec<(&WorkspaceImporter<'a>, ResolveImporterOptions)> =
            importers.iter().zip(importer_opts).collect();
        paired.sort_by(|(left, _), (right, _)| left.id.cmp(&right.id));
        paired.into_iter().unzip()
    };

    // The `minimumReleaseAge` cutoff is set uniformly on every
    // importer's `base_opts.published_by` by the install layer; it is
    // the upper bound on the time-based cutoff.
    let maximum_published_by = importer_opts.first().and_then(|opts| opts.base_opts.published_by);
    let TimeBasedCutoff { published_by: subdep_published_by, time } = if time_based {
        compute_time_based_cutoff(
            resolver,
            &importers,
            &importer_opts,
            dependency_groups,
            pick_lowest_direct,
            maximum_published_by,
            recorded_time.as_ref(),
        )
        .await
    } else {
        TimeBasedCutoff { published_by: maximum_published_by, time: BTreeMap::new() }
    };

    // Phase 1: every importer's initial wave resolves before any peer
    // hoist runs, then hoist rounds repeat across all importers until
    // none hoists — a workspace-wide barrier, so an optional-peer pick
    // sees every importer's resolved versions.
    //
    // The initial waves run concurrently, like the TypeScript resolver's
    // importer fan-out: the shared context's children-owner claims are
    // rank-ordered (not arrival-ordered) and the peer-hoist pickers'
    // preferred-version candidates are derived from the settled
    // reachable tree (see `WorkspaceTreeCtx::run_preferred_versions`),
    // so the resolved graph is the same regardless of interleaving, and
    // a large workspace's walks overlap their resolver and hook waits
    // instead of paying them importer by importer.
    let mut input_dirs = Vec::with_capacity(importers.len());
    let mut states = Vec::with_capacity(importers.len());
    for (importer_order, (importer, mut importer_opts)) in
        importers.iter().zip(importer_opts).enumerate()
    {
        importer_opts.pick_lowest_direct = pick_lowest_direct;
        importer_opts.subdep_published_by = subdep_published_by;
        input_dirs
            .push((importer_opts.base_opts.project_dir.clone(), importer_opts.modules_dir.clone()));
        // Boxed to keep the enclosing install future small: inlining a
        // wave's frame into it trips the workspace's large-future lint.
        let wave = Box::pin(ImporterHoistState::init(
            resolver,
            &importer.id,
            importer_order,
            importer.manifest,
            dependency_groups.iter().copied(),
            importer_opts,
            Arc::clone(&workspace),
        ));
        states.push(wave.await?);
    }
    // Computed after the init barrier and shared unchanged: recomputing it
    // per round would let the root's own hoisted peers become candidates for
    // the importers hoisted after it.
    let root_deps = Arc::new(
        states
            .iter()
            .find(|state| state.importer_id() == pnpm_lockfile::Lockfile::ROOT_IMPORTER_KEY)
            .map(ImporterHoistState::hoistable_root_deps)
            .transpose()?
            .unwrap_or_default(),
    );
    for state in &mut states {
        state.set_workspace_root_deps(Arc::clone(&root_deps));
    }
    // One discovery engine serves every hoist round of the workspace:
    // its persistent tree view + walker caches are what keep the
    // barrier below linear in workspace size (each importer's pass
    // short-circuits on the subtree verdicts recorded by the passes
    // before it).
    let mut peer_discovery = PeerHoistDiscovery::new();
    let mut initial_required_rounds: Vec<_> = states
        .iter_mut()
        .map(|state| state.prepare_initial_required_round(&mut peer_discovery))
        .collect();
    // The context is quiescent between the prepare barrier above and
    // the completes below, so one snapshot of the owner-scope maps
    // serves every importer.
    let first_importer_by_pkg = workspace.first_importer_by_pkg();
    let first_walk_missing_by_pkg = workspace.first_walk_missing_by_pkg();
    for (state, round) in states.iter().zip(&mut initial_required_rounds) {
        if let Some(round) = round {
            state.apply_owner_missing_scope(
                round,
                &first_importer_by_pkg,
                &first_walk_missing_by_pkg,
            );
        }
    }
    for (state, round) in states.iter_mut().zip(initial_required_rounds) {
        if let Some(round) = round {
            state.complete_initial_required_round(resolver, round, &mut peer_discovery).await?;
        }
    }
    loop {
        let mut any_hoisted = false;
        for state in &mut states {
            any_hoisted |= state.hoist_optional_round(resolver).await?;
        }
        if !any_hoisted {
            break;
        }
        for state in &mut states {
            state.run_required_round(resolver, &mut peer_discovery).await?;
        }
    }
    // Release the engine's tree view before the merged-tree snapshot
    // below clones the context again, so the two never coexist at peak.
    drop(peer_discovery);
    let mut per_importer_inputs: Vec<ImporterPeerInput> = Vec::with_capacity(importers.len());
    let mut hoisted_peer_provider_node_ids = std::collections::HashSet::default();
    for ((importer, state), (project_dir, modules_dir)) in
        importers.iter().zip(states).zip(input_dirs)
    {
        let (direct, importer_provider_node_ids) = state.into_direct();
        hoisted_peer_provider_node_ids.extend(importer_provider_node_ids);
        per_importer_inputs.push(ImporterPeerInput {
            id: importer.id.clone(),
            direct,
            root_dir: project_dir,
            modules_dir,
        });
    }

    // Reclaim the workspace ctx now that every importer's state has
    // dropped its `Arc<WorkspaceTreeCtx>`. The `try_unwrap` succeeds
    // when this is the sole remaining `Arc` reference (the common
    // case); the fallback snapshots out via the shared `Arc` for
    // parity.
    let mut merged_tree = match Arc::try_unwrap(workspace) {
        Ok(ws) => ws.into_resolved_tree(Vec::new()),
        Err(arc) => arc.snapshot(Vec::new()),
    };

    let peer_opts = ResolvePeersOptions {
        peers_suffix_max_length,
        dedupe_peers,
        exclude_links_from_lockfile,
        lockfile_dir: Some(lockfile_dir.clone()),
        project_dir: None,
        // Per-importer; resolve_peers_workspace swaps the
        // ImporterPeerInput's modules_dir into walker.opts before each
        // importer's walk.
        modules_dir: None,
        hoist_missing_scope: None,
        hoisted_peer_provider_node_ids,
        ..ResolvePeersOptions::default()
    };
    let peers = resolve_peers_workspace(
        &mut merged_tree,
        &per_importer_inputs,
        &lockfile_dir,
        dedupe_injected_deps,
        dedupe_peer_dependents,
        resolve_peers_from_workspace_root,
        peer_opts,
    );
    Ok(ResolveWorkspaceResult { merged_tree, peers, time })
}

/// What a `time-based` pre-pass learned about the direct dependencies.
struct TimeBasedCutoff {
    /// The ceiling every transitive dependency's publish date must
    /// respect.
    published_by: Option<DateTime<Utc>>,
    /// Publish date per direct dependency, for the lockfile's `time:`
    /// section.
    time: BTreeMap<String, String>,
}

/// Resolve every importer's direct dependencies and derive the
/// `time-based` publish-date cutoff for transitive deps.
///
/// Each direct dependency's publish date comes from its packument, or —
/// against a registry whose abbreviated metadata omits publish times —
/// from `recorded_time`, the date the lockfile's `time:` section
/// recorded for it. The cutoff is the newest of those dates plus an
/// hour, clamped by `maximum_published_by`.
///
/// Only the direct deps' publish date is read here, so the throwaway
/// resolves warm the resolver's packument cache for the real walk that
/// follows. Resolver errors are ignored here — the real walk surfaces
/// them.
async fn compute_time_based_cutoff<Chain>(
    resolver: &Chain,
    importers: &[&WorkspaceImporter<'_>],
    importer_opts: &[ResolveImporterOptions],
    dependency_groups: &[DependencyGroup],
    pick_lowest_direct: bool,
    maximum_published_by: Option<DateTime<Utc>>,
    recorded_time: Option<&BTreeMap<String, String>>,
) -> TimeBasedCutoff
where
    Chain: Resolver + ?Sized,
{
    let mut time = BTreeMap::new();
    for (importer, opts) in importers.iter().zip(importer_opts) {
        let Ok(specs) = importer_direct_wanted_specs(
            importer.manifest,
            dependency_groups.iter().copied(),
            opts.auto_install_peers,
            &opts.catalogs,
        ) else {
            continue;
        };
        let mut direct_opts = opts.base_opts.clone();
        direct_opts.pick_lowest_version = pick_lowest_direct;
        for (alias, bare_specifier, optional, injected) in specs {
            let wanted = WantedDependency {
                alias: Some(alias),
                bare_specifier: Some(bare_specifier),
                optional: Some(optional),
                injected: injected.then_some(true),
                ..WantedDependency::default()
            };
            let Ok(Some(result)) = resolver.resolve(&wanted, &direct_opts).await else { continue };
            let published_at = result.published_at.or_else(|| {
                recorded_time.and_then(|recorded| recorded.get(result.id.as_str())).cloned()
            });
            if let Some(published_at) = published_at {
                time.insert(result.id.into_inner(), published_at);
            }
        }
    }

    let newest =
        time.values().filter_map(|published_at| parse_packument_timestamp(published_at)).max();
    let candidate = newest.and_then(|date| date.checked_add_signed(Duration::hours(1)));
    let published_by = match (candidate, maximum_published_by) {
        (Some(candidate), Some(maximum)) => Some(candidate.min(maximum)),
        (Some(candidate), None) => Some(candidate),
        (None, maximum) => maximum,
    };
    TimeBasedCutoff { published_by, time }
}

#[cfg(test)]
mod tests;
