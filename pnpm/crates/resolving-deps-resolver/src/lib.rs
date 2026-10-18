#![cfg_attr(dylint_lib = "perfectionist", feature(register_tool))]
#![cfg_attr(dylint_lib = "perfectionist", register_tool(perfectionist))]
//! Dependency resolution for an install pass.
//!
//! The public entry point for an install pass is
//! [`fn@resolve_workspace`] — it loops over every workspace project,
//! drives [`fn@resolve_importer`] per importer, merges the per-importer
//! trees, and finishes with one [`fn@resolve_peers_workspace`] pass that
//! shares peer caches across importers and applies
//! `dedupeInjectedDeps` once with every importer's direct deps in
//! scope.
//!
//! 1. **Tree pass** ([`fn@resolve_dependency_tree`]). Walks a project
//!    manifest's direct dependencies through a
//!    [`Resolver`](pnpm_resolving_resolver_base::Resolver) chain
//!    and recurses on every resolved package's own manifest
//!    dependencies. Produces:
//!
//!    - [`ResolvedTree::packages`] — flat dedup map keyed by
//!      `pkgIdWithPatchHash`.
//!    - [`ResolvedTree::dependencies_tree`] — per-occurrence tree
//!      keyed by [`NodeId`].
//!    - [`ResolvedTree::all_peer_dep_names`] — names every visited
//!      package declares as a peer, used by the peer pass as the
//!      `parentPkgs` filter.
//!
//!    Peer dependencies are **not** walked as regular edges during the
//!    tree pass. They are recorded on [`ResolvedPackage::peer_set`]
//!    and consumed by the peer pass.
//!
//! 2. **Peer pass** ([`fn@resolve_peers`]). Walks the tree, matches each
//!    package's peer requirements against the parent chain, and
//!    produces [`DependenciesGraph`] — keyed by `DepPath` (one entry per
//!    `(pkgIdWithPatchHash, peer-suffix)` combination) and the entry
//!    point for the install layer.
//!
//! 3. **Hoist loop** ([`fn@resolve_importer`]). Runs pass 1 plus a
//!    graph-free discovery variant of pass 2 (one persistent
//!    discovery engine per workspace resolve, so repeat walks
//!    short-circuit on already-settled subtrees), aggregates missing
//!    required and optional peers via [`fn@hoist_peers`] /
//!    [`fn@get_hoistable_optional_peers`], extends the tree with
//!    hoisted picks via [`extend_tree`], and re-runs discovery until
//!    both reach a fixed point. The full pass 2 then runs once per
//!    install.
//!
//! Notable design points:
//!
//! - **Shared workspace ctx.** [`fn@resolve_workspace`] constructs one
//!   [`WorkspaceTreeCtx`] and hands an `Arc::clone` to every per-importer
//!   [`TreeCtx`], so the resolver's per-`pkgIdWithPatchHash` dedup
//!   (`packages`, `resolved_by_wanted`, `children_specs_by_id`,
//!   `children_by_id`) and the peer-walker seed sets
//!   (`all_peer_dep_names`, `applied_patches`) carry across importers.
//!   `base_opts.project_dir` stays per-importer on each [`TreeCtx`] so
//!   transitive local-protocol resolutions keep the right anchor. The
//!   downstream [`fn@resolve_peers_workspace`] then walks every
//!   importer's direct deps through one Walker so `peersCache` +
//!   `purePkgs` share the same way.
//! - **No catalog / hook / lockfile-pinned-version bias.** The
//!   resolver is fed each child's manifest range verbatim. Lockfile-
//!   seeded preferred versions arrive via the orchestrator's
//!   `all_preferred_versions` option — callers pre-seed with the
//!   `pnpm-lockfile-preferred-versions` crate.

pub use dependencies_graph::{
    DependenciesGraph, DependenciesGraphNode, MissingPeer, ParentChain, ParentPackageRef,
    PeerDependencyIssue, PeerDependencyIssues, ResolvedDependencyEdges,
};
pub use lockfile_reuse::locked_child_admits;
pub use node_id::NodeId;
pub use parent_pkg_aliases::{ParentPkgAliases, declared_peer_dependencies, own_dependency_names};
pub use peer_provider::{
    DependencyOverrider, HoistPeersOptions, MissingPeerInfo, WorkspaceRootDep,
    get_hoistable_optional_peers, hoist_peers, peer_shadowed_dependency_names,
};
pub use pnpm_deps_path::DepPath;
pub use pnpm_package_name::is_valid_dependency_alias;
pub use resolve_dependency_tree::{
    ChildCatalogs, ChildSpec, DependencyResolutionError, Deprecation, DeprecationLogFn,
    FinalizedChild, FinalizedPackage, FinalizedPackageFn, ManifestHook, ResolveDependencyTreeError,
    ResolveDependencyTreeOptions, SkippedOptionalDependency, SkippedOptionalDependencyParent,
    SkippedOptionalLogFn, TreeCtx, UpdateDepth, UpdateReuseScope, UpdateTargets, VersionLine,
    WorkspaceTreeCtx, extend_tree, local_directory_child_specs, manifest_child_specs,
    real_package_name_of, resolve_dependency_tree,
};
pub use resolve_importer::{
    ImporterPeerOptions, ImporterResolutionInputs, ManifestTransformHooks, PeerLinkOptions,
    ResolveImporterError, ResolveImporterOptions, ResolveImporterResult, resolve_importer,
    resolve_importer_with_workspace,
};
pub use resolve_peers::{
    HoistMissingScope, ImporterPeerInput, PeerResolutionScope, ResolvePeersOptions,
    ResolvePeersResult, WorkspaceResolvePeersResult, resolve_peers, resolve_peers_workspace,
};
pub use resolve_workspace::{
    ResolveWorkspaceResult, ResolvedWorkspaceDependencies, WorkspaceImporter,
    WorkspaceLockfileReuse, WorkspacePeerResolutionOptions, WorkspaceResolveHooks,
    WorkspaceResolveOptions, WorkspaceVersionResolution, resolve_workspace,
    resolve_workspace_dependencies,
};
pub use resolved_tree::{
    ChildEdge, DependenciesTree, DependenciesTreeNode, DirectDep, LockfilePeerBlocks, PeerDep,
    PeerSet, ResolvedPackage, ResolvedPackageInput, ResolvedTree, TreeChildren,
    lockfile_peer_blocks,
};

mod dedupe_injected_deps;
mod dedupe_peer_dependents;
mod dep_path_compatibility;
mod dependencies_graph;
mod link_target;
mod lockfile_reuse;
mod node_id;
mod parent_pkg_aliases;
mod peer_provider;
mod resolve_dependency_tree;
mod resolve_importer;
mod resolve_peers;
mod resolve_workspace;
mod resolved_tree;

#[cfg(test)]
mod tests;
