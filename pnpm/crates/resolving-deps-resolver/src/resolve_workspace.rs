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

pub use dependencies::{ResolvedWorkspaceDependencies, resolve_workspace_dependencies};
pub use options::{
    WorkspaceImporter, WorkspaceLockfileReuse, WorkspacePeerResolutionOptions,
    WorkspaceResolveHooks, WorkspaceResolveOptions, WorkspaceVersionResolution,
};

mod broken_optional;
mod dependencies;
mod finish;
mod hoist_waves;
mod importers;
mod options;
mod time_based;

use crate::{
    resolve_importer::{ResolveImporterError, ResolveImporterOptions},
    resolve_peers::WorkspaceResolvePeersResult,
    resolved_tree::ResolvedTree,
};
use pnpm_package_manifest::DependencyGroup;
use pnpm_resolving_resolver_base::Resolver;
use std::collections::BTreeMap;

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
    resolve_workspace_dependencies(
        resolver,
        importers,
        dependency_groups,
        opts,
        per_importer_options,
    )
    .await?
    .resolve_peers(resolver)
    .await
}

#[cfg(test)]
mod tests;
