use std::collections::{BTreeMap, HashMap, HashSet};

use pacquet_resolving_resolver_base::{ResolutionPolicyViolation, ResolveResult};

use crate::node_id::NodeId;

/// Per-occurrence tree carried by [`ResolvedTree::dependencies_tree`].
/// Mirrors upstream's
/// [`DependenciesTree`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L103-L109)
/// type alias.
pub type DependenciesTree = HashMap<NodeId, DependenciesTreeNode>;

/// Output of [`fn@crate::resolve_dependency_tree`].
///
/// Mirrors upstream's
/// [`ResolveDependencyTreeResult`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencyTree.ts#L151-L170)
/// for the npm-shaped slice pacquet currently exposes.
///
/// The shape carries two indices into the same set of resolved
/// packages:
///
/// - [`packages`](Self::packages) is the **flat dedup map**, keyed by
///   `pkgIdWithPatchHash` (today `name@version`). One entry per
///   resolved package, no per-occurrence repetition. Upstream calls
///   the equivalent index `resolvedPkgsById`.
/// - [`dependencies_tree`](Self::dependencies_tree) is the **per-
///   occurrence tree**, keyed by [`NodeId`]. Every parent → child edge
///   has a fresh child `NodeId`, even when two parents share the same
///   `pkgIdWithPatchHash`, because the peer-resolution stage needs
///   per-occurrence state (a shared package under two parents can
///   compute different peer suffixes).
#[derive(Debug, Default, Clone)]
pub struct ResolvedTree {
    pub direct: Vec<DirectDep>,
    pub packages: HashMap<String, ResolvedPackage>,
    pub dependencies_tree: DependenciesTree,
    pub all_peer_dep_names: HashSet<String>,
    pub policy_violations: Vec<ResolutionPolicyViolation>,
    /// Set of `patchedDependencies` keys (e.g. `lodash@4.17.21`,
    /// `react@^18`) whose patch was actually applied to at least one
    /// resolved package. Mirrors upstream's
    /// [`appliedPatches`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L1505)
    /// set, threaded out of the resolver so the orchestrator can pass
    /// it to [`pacquet_patching::verify_patches`] for the
    /// `ERR_PNPM_UNUSED_PATCH` diagnostic.
    pub applied_patches: HashSet<String>,
}

/// One edge in the resolved tree: the local install name (`alias`) and
/// the resolved node's [`NodeId`]. Mirrors upstream's edge shape on
/// `directNodeIdsByAlias` plus the `pkgId` field carried on
/// `ResolvedDirectDependency`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectDep {
    /// Local install name in `node_modules`. For an npm-alias entry
    /// (`"foo": "npm:bar@^1"`) this is `"foo"`; the resolved
    /// package's real name is recoverable from
    /// [`ResolvedPackage::result`].
    pub alias: String,
    /// Per-occurrence node identifier. Use this to look up the
    /// corresponding [`DependenciesTreeNode`] in
    /// [`ResolvedTree::dependencies_tree`].
    pub node_id: NodeId,
    /// `pkgIdWithPatchHash` of the resolved package — same value as
    /// `dependencies_tree[node_id].resolved_package_id`. Carried at
    /// the edge for callers that only need the dedup key and want to
    /// avoid the tree lookup.
    pub id: String,
}

/// One resolved package, deduped by `pkgIdWithPatchHash`. Mirrors
/// upstream's
/// [`ResolvedPackage`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L248-L279)
/// for the npm-shaped slice pacquet currently exposes.
///
/// **Children live on [`DependenciesTreeNode`], not here.** Two parents
/// that share a resolved package each get their own per-occurrence
/// tree node with its own children edges — a `ResolvedPackage` is the
/// dedup-shared *envelope*, not a tree node.
#[derive(Debug, Clone)]
pub struct ResolvedPackage {
    pub id: String,
    pub result: ResolveResult,
    /// `peerDependencies` from the package's manifest, with names that
    /// also appear in the package's own `dependencies` /
    /// `optionalDependencies` filtered out (mirrors upstream's
    /// [`peerDependenciesWithoutOwn`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L1791-L1815)).
    /// `BTreeMap` keeps iteration order stable so peer-suffix
    /// construction is deterministic.
    pub peer_dependencies: BTreeMap<String, PeerDep>,
    /// `true` when every path from any importer to this package goes
    /// through at least one `optionalDependencies` edge. Mirrors
    /// upstream's [`ResolvedPackage.optional`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L254)
    /// AND-fold:
    ///
    /// - On the first visit, `optional` is set to
    ///   `wanted.optional || parent.optional` — propagating an
    ///   ancestor's optionality down the chain.
    /// - On every subsequent visit, `optional` is AND-folded with the
    ///   new edge's `current_is_optional`, so a single non-optional
    ///   path flips it back to `false` and keeps it there.
    ///
    /// Downstream consumers (the lockfile adapter, the `BuildModules`
    /// failure-tolerance gate) read this to decide whether a build
    /// failure is fatal or should be reported as a skipped optional.
    pub optional: bool,
}

/// One peer-dependency entry on a [`ResolvedPackage`]. Mirrors upstream's
/// [`PeerDependency`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L246-L247)
/// shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerDep {
    /// Semver range from the upstream manifest. May carry a
    /// `workspace:` prefix that the peer matcher strips before
    /// checking.
    pub version: String,
    /// `true` when the manifest's `peerDependenciesMeta[name].optional`
    /// is set — a missing peer with `optional` true is recorded as an
    /// issue but does not block resolution.
    pub optional: bool,
}

/// One per-occurrence node in the dependencies tree. Mirrors upstream's
/// [`DependenciesTreeNode`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L92-L103).
///
/// Children resolution is eager in pacquet — upstream supports a lazy
/// `children: () => ChildrenMap` arm so cycles can be broken inside
/// `buildTree`. Pacquet's port detects cycles by tracking the chain of
/// `pkgIdWithPatchHash` ancestors directly inside
/// [`fn@crate::resolve_dependency_tree`], so children are always materialised.
#[derive(Debug, Clone)]
pub struct DependenciesTreeNode {
    /// Key into [`ResolvedTree::packages`].
    pub resolved_package_id: String,
    /// `alias → child NodeId` edges. `BTreeMap` keeps iteration order
    /// stable so downstream peer-suffix construction is deterministic.
    pub children: BTreeMap<String, NodeId>,
    /// Distance from the root importer (root = 0). Upstream uses
    /// `depth = -1` to mark linked / pruned nodes; pacquet doesn't
    /// emit `-1` today because workspace-link resolution hasn't been
    /// ported.
    pub depth: i32,
    /// Whether the package may be skipped when an optional dep fails
    /// for its host platform. Always `true` for the npm-shaped slice
    /// pacquet currently exposes.
    pub installable: bool,
}
