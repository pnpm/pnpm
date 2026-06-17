//! Multi-importer entry point for an install pass. Mirrors pnpm's
//! [`resolveDependencies`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-resolver/src/index.ts#L128)
//! shape: take every workspace project the install touches, run the
//! per-importer hoist + peer-resolution loop with shared cross-importer
//! caches, and emit the combined `DependenciesGraph` plus the
//! per-importer `direct_dependencies_by_importer` map the install
//! layer consumes.
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
        ManifestHook, UpdateReuseScope, WorkspaceTreeCtx, importer_direct_wanted_specs,
    },
    resolve_importer::{ImporterHoistState, ResolveImporterError, ResolveImporterOptions},
    resolve_peers::{
        ImporterPeerInput, ResolvePeersOptions, WorkspaceResolvePeersResult,
        resolve_peers_workspace,
    },
    resolved_tree::ResolvedTree,
};
use chrono::{DateTime, Duration, Utc};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_resolving_resolver_base::{Resolver, WantedDependency, parse_packument_timestamp};
use std::{collections::HashMap, path::PathBuf, sync::Arc};

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
    /// it. Mirrors pnpm's `dedupePeerDependents` setting (default
    /// `true`).
    pub dedupe_peer_dependents: bool,
    /// When true, non-root importers can resolve peers from the
    /// workspace root's direct dependencies. Mirrors pnpm's
    /// `resolvePeersFromWorkspaceRoot` setting.
    pub resolve_peers_from_workspace_root: bool,
    /// Threaded into [`ResolvePeersOptions::exclude_links_from_lockfile`]
    /// for the workspace-wide peer pass. Per-importer
    /// [`ResolvePeersOptions::modules_dir`] comes from each
    /// [`crate::ImporterPeerInput::modules_dir`].
    pub exclude_links_from_lockfile: bool,
    pub lockfile_dir: PathBuf,
    pub peers_suffix_max_length: usize,
    /// `readPackageHook` applied to every resolved manifest before it
    /// enters the wanted-dep cache. Workspace-wide (one hook per
    /// install); the install layer typically threads
    /// `packageExtensions` here. See [`ManifestHook`].
    pub manifest_hook: Option<ManifestHook>,

    /// When `true`, every importer's direct dependencies are resolved
    /// to their lowest satisfying version (`resolutionMode: time-based`
    /// / `lowest-direct`). Threaded onto each
    /// [`ResolveImporterOptions::pick_lowest_direct`].
    pub pick_lowest_direct: bool,

    /// When `true` (`resolutionMode: time-based`), a pre-pass resolves
    /// every importer's direct deps to find the newest publication
    /// date, then constrains all transitive deps to versions published
    /// no later than that (plus a one-hour delta), clamped by any
    /// `minimumReleaseAge` cutoff. Mirrors pnpm's
    /// [`getPublishedByDate`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/installing/deps-resolver/src/resolveDependencies.ts#L506-L517)
    /// step.
    pub time_based: bool,

    /// The prior `pnpm-lock.yaml` the install started from, when one
    /// exists. Threaded into [`WorkspaceTreeCtx`] so the tree walk can
    /// reuse already-resolved dependencies instead of re-resolving them
    /// (see `pacquet/plans/LOCKFILE_RESOLUTION_REUSE.md`). `None` on a
    /// first install or when reuse is disabled.
    pub wanted_lockfile: Option<Arc<pacquet_lockfile::Lockfile>>,

    /// Which dependencies `pacquet update` excludes from lockfile-
    /// resolution reuse. [`UpdateReuseScope::All`] for `install` / `add`.
    pub update_reuse_scope: UpdateReuseScope,

    /// `pnpmfileHook` applied to every resolved manifest before it
    /// enters the wanted-dep cache. Workspace-wide (one hook per
    /// install); wraps `readPackage` from `.pnpmfile.cjs` / `pnpmfile.cjs`.
    pub pnpmfile_hook: Option<Arc<dyn pacquet_hooks::PnpmfileHooks>>,

    /// `context.log(...)` sink for the `pnpmfile_hook`'s `readPackage`
    /// calls, pre-bound to the install's reporter. `None` leaves hook
    /// logging a no-op.
    pub read_package_log: Option<pacquet_hooks::LogFn>,

    /// The install's `autoInstallPeers` setting, threaded onto the
    /// shared [`WorkspaceTreeCtx`] so the tree walk drops
    /// peer-shadowed `dependencies` entries the way pnpm does. Also
    /// overrides every per-importer
    /// [`crate::ResolveImporterOptions::auto_install_peers`] — the
    /// setting is workspace-wide, like pnpm's `autoInstallPeers`.
    pub auto_install_peers: bool,
    /// Resolved registry map (`"default"` + per-scope), for
    /// materializing a prior `Registry` lockfile resolution back into
    /// its tarball URL when building the `currentPkg` payload custom
    /// resolvers receive.
    pub registries: HashMap<String, String>,
}

/// Result of [`fn@resolve_workspace`]. The combined
/// [`WorkspaceResolvePeersResult`] holds the cross-importer graph + the
/// per-importer `direct_dependencies_by_alias` map; `merged_tree`
/// carries the shared `ResolvedTree` snapshot the workspace ctx
/// produced after every importer's walk folded into the shared maps.
pub struct ResolveWorkspaceResult {
    pub merged_tree: ResolvedTree,
    pub peers: WorkspaceResolvePeersResult,
}

/// Resolve every importer's dependencies, then run one workspace-wide
/// peer-resolution + dedupe pass.
///
/// `per_importer_options` is invoked per importer to build that
/// importer's own [`ResolveImporterOptions`] — the install layer owns
/// the per-importer wiring (project dir, modules dir, lockfile dir,
/// exclude-links-from-lockfile, etc.). The closure shape mirrors how
/// pnpm constructs `ImporterToResolve` per project inside
/// [`resolveDependencyTree`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-resolver/src/resolveDependencyTree.ts#L236).
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
        manifest_hook,
        pnpmfile_hook,
        read_package_log,
        pick_lowest_direct,
        time_based,
        wanted_lockfile,
        update_reuse_scope,
        auto_install_peers,
        registries,
    } = opts;
    let workspace = Arc::new(
        WorkspaceTreeCtx::default()
            .with_manifest_hook(manifest_hook)
            .with_wanted_lockfile(wanted_lockfile)
            .with_update_reuse_scope(update_reuse_scope)
            .with_pnpmfile_hook(pnpmfile_hook)
            .with_read_package_log(read_package_log)
            .with_auto_install_peers(auto_install_peers)
            .with_registries(registries),
    );

    // Build every importer's options up front so the `time-based`
    // pre-pass and the resolve loop see the same per-importer wiring.
    // `auto_install_peers` is workspace-wide (one setting per install,
    // like pnpm's `autoInstallPeers`), so the workspace-level value
    // overrides whatever the per-importer callback set — the importer
    // hoist loop and the tree walk's shadow pruning must agree.
    let importer_opts: Vec<ResolveImporterOptions> = importers
        .iter()
        .map(&mut per_importer_options)
        .map(|mut opts| {
            opts.auto_install_peers = auto_install_peers;
            opts
        })
        .collect();

    // The `minimumReleaseAge` cutoff is set uniformly on every
    // importer's `base_opts.published_by` by the install layer; it is
    // pnpm's `maximumPublishedBy`, the upper bound on the time-based
    // cutoff.
    let maximum_published_by = importer_opts.first().and_then(|opts| opts.base_opts.published_by);
    let subdep_published_by = if time_based {
        compute_time_based_cutoff(
            resolver,
            importers,
            &importer_opts,
            dependency_groups,
            pick_lowest_direct,
            maximum_published_by,
        )
        .await
    } else {
        maximum_published_by
    };

    // Phase 1: every importer's initial wave resolves before any peer
    // hoist runs, then hoist rounds repeat across all importers until
    // none hoists — upstream's `resolveRootDependencies` barrier, so
    // an optional-peer pick sees every importer's resolved versions.
    let mut states = Vec::with_capacity(importers.len());
    let mut input_dirs = Vec::with_capacity(importers.len());
    for (importer_order, (importer, mut importer_opts)) in
        importers.iter().zip(importer_opts).enumerate()
    {
        importer_opts.pick_lowest_direct = pick_lowest_direct;
        importer_opts.subdep_published_by = subdep_published_by;
        input_dirs
            .push((importer_opts.base_opts.project_dir.clone(), importer_opts.modules_dir.clone()));
        states.push(
            ImporterHoistState::init(
                resolver,
                &importer.id,
                importer_order,
                importer.manifest,
                dependency_groups.iter().copied(),
                importer_opts,
                Arc::clone(&workspace),
            )
            .await?,
        );
    }
    loop {
        for state in &mut states {
            state.run_required_round(resolver).await?;
        }
        let mut any_hoisted = false;
        for state in &mut states {
            any_hoisted |= state.hoist_optional_round(resolver).await?;
        }
        if !any_hoisted {
            break;
        }
    }
    let mut per_importer_inputs: Vec<ImporterPeerInput> = Vec::with_capacity(importers.len());
    for ((importer, state), (project_dir, modules_dir)) in
        importers.iter().zip(states).zip(input_dirs)
    {
        per_importer_inputs.push(ImporterPeerInput {
            id: importer.id.clone(),
            direct: state.into_direct(),
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
        // Per-importer; resolve_peers_workspace swaps the
        // ImporterPeerInput's modules_dir into walker.opts before each
        // importer's walk.
        modules_dir: None,
        hoist_missing_scope: None,
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

    Ok(ResolveWorkspaceResult { merged_tree, peers })
}

/// Resolve every importer's direct dependencies and derive the
/// `time-based` publish-date cutoff for transitive deps.
///
/// Mirrors pnpm's
/// [`getPublishedByDate` + clamp](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/installing/deps-resolver/src/resolveDependencies.ts#L506-L517).
/// Only the direct deps' `published_at` is read here, so the throwaway
/// resolves warm the resolver's packument cache for the real walk that
/// follows. Resolver errors are ignored here — the real walk surfaces
/// them.
async fn compute_time_based_cutoff<Chain>(
    resolver: &Chain,
    importers: &[WorkspaceImporter<'_>],
    importer_opts: &[ResolveImporterOptions],
    dependency_groups: &[DependencyGroup],
    pick_lowest_direct: bool,
    maximum_published_by: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>>
where
    Chain: Resolver + ?Sized,
{
    let mut newest: Option<DateTime<Utc>> = None;
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
            if let Ok(Some(result)) = resolver.resolve(&wanted, &direct_opts).await
                && let Some(published_at) = result.published_at.as_deref()
                && let Some(parsed) = parse_packument_timestamp(published_at)
            {
                newest = Some(newest.map_or(parsed, |current| current.max(parsed)));
            }
        }
    }

    let candidate = newest.and_then(|date| date.checked_add_signed(Duration::hours(1)));
    match (candidate, maximum_published_by) {
        (Some(candidate), Some(maximum)) => Some(candidate.min(maximum)),
        (Some(candidate), None) => Some(candidate),
        (None, maximum) => maximum,
    }
}

#[cfg(test)]
mod tests;
