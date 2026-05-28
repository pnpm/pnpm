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
    resolve_importer::{
        ResolveImporterError, ResolveImporterOptions, ResolveImporterResult, resolve_importer,
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
}

/// Result of [`fn@resolve_workspace`]. The combined
/// [`WorkspaceResolvePeersResult`] holds the cross-importer graph + the
/// per-importer `direct_dependencies_by_alias` map; the per-importer
/// resolved-tree slices are retained so callers that need the
/// `policy_violations` / `applied_patches` sets can read them off the
/// per-importer entries.
pub struct ResolveWorkspaceResult {
    pub merged_tree: ResolvedTree,
    pub per_importer_trees: Vec<ResolvedTree>,
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
    } = opts;
    let mut per_importer_trees: Vec<ResolvedTree> = Vec::with_capacity(importers.len());
    let mut per_importer_inputs: Vec<ImporterPeerInput> = Vec::with_capacity(importers.len());
    for importer in importers {
        let importer_opts = per_importer_options(importer);
        let project_dir = importer_opts.base_opts.project_dir.clone();
        let modules_dir = importer_opts.modules_dir.clone();
        let ResolveImporterResult { resolved_tree, .. } = resolve_importer(
            resolver,
            importer.manifest,
            dependency_groups.iter().copied(),
            importer_opts,
        )
        .await?;
        let direct = resolved_tree.direct.clone();
        per_importer_inputs.push(ImporterPeerInput {
            id: importer.id.clone(),
            direct,
            root_dir: project_dir,
            modules_dir,
        });
        per_importer_trees.push(resolved_tree);
    }

    let mut merged_tree = merge_trees(&per_importer_trees);
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

    Ok(ResolveWorkspaceResult { merged_tree, per_importer_trees, peers })
}

/// Combine every importer's [`ResolvedTree`] into one shared tree the
/// multi-importer peer walker walks against. Counter-allocated NodeIds
/// are globally unique (see [`crate::NodeId::next`]); leaf NodeIds for
/// the same package id unify naturally. The `optional` AND-fold across
/// duplicate `ResolvedPackage` entries mirrors upstream's same fold
/// inside `resolveDependencies`.
fn merge_trees(trees: &[ResolvedTree]) -> ResolvedTree {
    let mut merged = ResolvedTree::default();
    for tree in trees {
        for (id, pkg) in &tree.packages {
            merged
                .packages
                .entry(id.clone())
                .and_modify(|existing| existing.optional = existing.optional && pkg.optional)
                .or_insert_with(|| pkg.clone());
        }
        for (node_id, node) in &tree.dependencies_tree {
            merged.dependencies_tree.entry(node_id.clone()).or_insert_with(|| node.clone());
        }
        for name in &tree.all_peer_dep_names {
            merged.all_peer_dep_names.insert(name.clone());
        }
        for key in &tree.applied_patches {
            merged.applied_patches.insert(key.clone());
        }
        for (id, edges) in &tree.children_by_id {
            merged.children_by_id.entry(id.clone()).or_insert_with(|| Arc::clone(edges));
        }
        merged.policy_violations.extend(tree.policy_violations.iter().cloned());
    }
    merged
}
