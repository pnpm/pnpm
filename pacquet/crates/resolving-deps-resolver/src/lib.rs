//! Port of pnpm's
//! [`@pnpm/installing.deps-resolver`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/index.ts).
//!
//! Two passes live here:
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
//! This is intentionally a thin slice of upstream:
//!
//! - **Single importer.** Pacquet's install doesn't expose workspaces
//!   to the resolver yet; the entry point takes one manifest at a
//!   time.
//! - **No `autoInstallPeers` hoisting.** Upstream's
//!   [`hoistPeers`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/hoistPeers.ts)
//!   pre-pass that folds peer deps into the importer's direct deps is
//!   a separate concern, scheduled after this slice lands.
//! - **No catalog / hook / lockfile-pinned-version bias.** The
//!   resolver is fed each child's manifest range verbatim.

mod dependencies_graph;
mod node_id;
mod resolve_dependency_tree;
mod resolve_peers;
mod resolved_tree;

pub use dependencies_graph::{
    DepPath, DependenciesGraph, DependenciesGraphNode, MissingPeer, PeerDependencyIssue,
    PeerDependencyIssues,
};
pub use node_id::NodeId;
pub use resolve_dependency_tree::{
    ResolveDependencyTreeError, ResolveDependencyTreeOptions, resolve_dependency_tree,
};
pub use resolve_peers::{ResolvePeersOptions, resolve_peers};
pub use resolved_tree::{
    DependenciesTree, DependenciesTreeNode, DirectDep, PeerDep, ResolvedPackage, ResolvedTree,
};

#[cfg(test)]
mod tests;
