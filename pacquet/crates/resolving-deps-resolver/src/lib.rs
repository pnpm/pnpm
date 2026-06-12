#![cfg_attr(dylint_lib = "perfectionist", feature(register_tool))]
#![cfg_attr(dylint_lib = "perfectionist", register_tool(perfectionist))]
//! Port of pnpm's
//! [`@pnpm/installing.deps-resolver`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/index.ts).
//!
//! The public entry point for an install pass is
//! [`fn@resolve_workspace`] — it loops over every workspace project,
//! drives [`fn@resolve_importer`] per importer, merges the per-importer
//! trees, and finishes with one [`fn@resolve_peers_workspace`] pass that
//! shares peer caches across importers and applies
//! `dedupeInjectedDeps` once with every importer's direct deps in
//! scope. [`fn@resolve_importer`] still owns three lower-level passes
//! and the `autoInstallPeers` hoist loop that ties them together.
//!
//! 1. **Tree pass** ([`fn@resolve_dependency_tree`]). Walks a project
//!    manifest's direct dependencies through a
//!    [`Resolver`](pacquet_resolving_resolver_base::Resolver) chain
//!    and recurses on every resolved package's own manifest
//!    dependencies. Produces:
//!
//!    - [`ResolvedTree::packages`] — flat dedup map keyed by
//!      `pkgIdWithPatchHash`, mirroring upstream's `resolvedPkgsById`.
//!    - [`ResolvedTree::dependencies_tree`] — per-occurrence tree
//!      keyed by [`NodeId`], mirroring upstream's `dependenciesTree`.
//!    - [`ResolvedTree::all_peer_dep_names`] — names every visited
//!      package declares as a peer, used by the peer pass as the
//!      `parentPkgs` filter.
//!
//!    Peer dependencies are **not** walked as regular edges during the
//!    tree pass. They are recorded on [`ResolvedPackage::peer_dependencies`]
//!    and consumed by the peer pass.
//!
//! 2. **Peer pass** ([`fn@resolve_peers`]). Walks the tree, matches each
//!    package's peer requirements against the parent chain, and
//!    produces [`DependenciesGraph`] — keyed by `DepPath` (one entry per
//!    `(pkgIdWithPatchHash, peer-suffix)` combination) and the entry
//!    point for the install layer.
//!
//! 3. **Hoist loop** ([`fn@resolve_importer`]). Runs passes 1–2,
//!    aggregates missing required and optional peers via
//!    [`fn@hoist_peers`] / [`fn@get_hoistable_optional_peers`], extends
//!    the tree with hoisted picks via
//!    [`extend_tree`], and re-runs the peer pass
//!    until both pass-1 and pass-2 reach a fixed point. Mirrors
//!    upstream's
//!    [`resolveRootDependencies`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L327-L437).
//!
//! This is intentionally a thin slice of upstream:
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
//!   `pacquet-lockfile-preferred-versions` crate.

mod dedupe_injected_deps;
mod dedupe_peer_dependents;
mod dependencies_graph;
mod hoist_peers;
mod lockfile_reuse;
mod node_id;
mod resolve_dependency_tree;
mod resolve_importer;
mod resolve_peers;
mod resolve_workspace;
mod resolved_tree;
mod validate_dependency_alias;

pub use dependencies_graph::{
    DependenciesGraph, DependenciesGraphNode, MissingPeer, PeerDependencyIssue,
    PeerDependencyIssues,
};
pub use hoist_peers::{
    HoistPeersOptions, MissingPeerInfo, WorkspaceRootDep, get_hoistable_optional_peers, hoist_peers,
};
pub use node_id::NodeId;
pub use pacquet_deps_path::DepPath;
pub use resolve_dependency_tree::{
    ManifestHook, ResolveDependencyTreeError, ResolveDependencyTreeOptions, TreeCtx,
    UpdateReuseScope, WorkspaceTreeCtx, extend_tree, resolve_dependency_tree,
};
pub use resolve_importer::{
    ResolveImporterError, ResolveImporterOptions, ResolveImporterResult, resolve_importer,
    resolve_importer_with_workspace,
};
pub use resolve_peers::{
    HoistMissingScope, ImporterPeerInput, ResolvePeersOptions, ResolvePeersResult,
    WorkspaceResolvePeersResult, resolve_peers, resolve_peers_workspace,
};
pub use resolve_workspace::{
    ResolveWorkspaceResult, WorkspaceImporter, WorkspaceResolveOptions, resolve_workspace,
};
pub use resolved_tree::{
    ChildEdge, DependenciesTree, DependenciesTreeNode, DirectDep, PeerDep, ResolvedPackage,
    ResolvedTree, TreeChildren,
};
pub use validate_dependency_alias::is_valid_dependency_alias;

#[cfg(test)]
mod tests;
