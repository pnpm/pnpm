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

use std::path::PathBuf;
use std::sync::Arc;

use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_resolving_resolver_base::Resolver;

use crate::{
    resolve_dependency_tree::{ManifestHook, WorkspaceTreeCtx},
    resolve_importer::{
        ResolveImporterError, ResolveImporterOptions, ResolveImporterResult,
        resolve_importer_with_workspace,
    },
    resolve_peers::{
        ImporterPeerInput, ResolvePeersOptions, WorkspaceResolvePeersResult,
        resolve_peers_workspace,
    },
    resolved_tree::ResolvedTree,
};

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
pub async fn resolve_workspace<'a, R, F>(
    resolver: &R,
    importers: &[WorkspaceImporter<'a>],
    dependency_groups: &[DependencyGroup],
    opts: WorkspaceResolveOptions,
    mut per_importer_options: F,
) -> Result<ResolveWorkspaceResult, ResolveImporterError>
where
    R: Resolver + ?Sized,
    F: FnMut(&WorkspaceImporter<'a>) -> ResolveImporterOptions,
{
    let WorkspaceResolveOptions {
        dedupe_peers,
        dedupe_injected_deps,
        exclude_links_from_lockfile,
        lockfile_dir,
        peers_suffix_max_length,
        manifest_hook,
    } = opts;
    let workspace = Arc::new(WorkspaceTreeCtx::default().with_manifest_hook(manifest_hook));
    let mut per_importer_inputs: Vec<ImporterPeerInput> = Vec::with_capacity(importers.len());
    for importer in importers {
        let importer_opts = per_importer_options(importer);
        let project_dir = importer_opts.base_opts.project_dir.clone();
        let modules_dir = importer_opts.modules_dir.clone();
        let ResolveImporterResult { resolved_tree, .. } = resolve_importer_with_workspace(
            resolver,
            importer.manifest,
            dependency_groups.iter().copied(),
            importer_opts,
            Arc::clone(&workspace),
        )
        .await?;
        let direct = resolved_tree.direct;
        per_importer_inputs.push(ImporterPeerInput {
            id: importer.id.clone(),
            direct,
            root_dir: project_dir,
            modules_dir,
        });
    }

    // Reclaim the workspace ctx now that every per-importer
    // `resolve_importer_with_workspace` call has dropped its
    // `Arc<WorkspaceTreeCtx>`. The `try_unwrap` succeeds when this is
    // the sole remaining `Arc` reference (the common case); the
    // fallback snapshots out via the shared `Arc` for parity.
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
    };
    let peers = resolve_peers_workspace(
        &mut merged_tree,
        &per_importer_inputs,
        &lockfile_dir,
        dedupe_injected_deps,
        peer_opts,
    );

    Ok(ResolveWorkspaceResult { merged_tree, peers })
}
