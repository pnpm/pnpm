use crate::node_id::NodeId;
use pacquet_resolving_resolver_base::{ResolutionPolicyViolation, ResolveResult};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};

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
///   occurrence tree**, keyed by [`NodeId`]. Non-leaf nodes get a fresh
///   child `NodeId` per parent occurrence so the peer-resolution stage
///   can compute different peer suffixes per call site. Leaves (no
///   `dependencies`, `optionalDependencies`, `peerDependencies`, or
///   `peerDependenciesMeta`) collapse onto one shared `NodeId`,
///   mirroring upstream's
///   [`pkgIsLeaf` reuse](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L1580):
///   a leaf has no per-occurrence state worth distinguishing, so every
///   parent that references it points at the same tree node.
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
    /// Per-`pkgIdWithPatchHash` child list: `(install_alias,
    /// resolved_child_pkg_id, optional)`. Populated by the first walk
    /// of each package — every subsequent revisit reuses the same
    /// entry. Mirrors upstream's
    /// [`childrenByParentId`](https://github.com/pnpm/pnpm/blob/c86c423bdc/installing/deps-resolver/src/resolveDependencies.ts#L185-L186)
    /// plus the
    /// [`buildTree` child enumeration](https://github.com/pnpm/pnpm/blob/c86c423bdc/installing/deps-resolver/src/resolveDependencyTree.ts#L371-L401)
    /// — the peer-resolver's `realize_children` walks this to
    /// allocate per-occurrence `NodeId`s for a
    /// [`TreeChildren::Lazy`] node.
    pub children_by_id: HashMap<String, Arc<Vec<ChildEdge>>>,
}

/// One entry on [`ResolvedTree::children_by_id`] — the resolved
/// shape of a package's children list as recorded by the first walk.
#[derive(Debug, Clone)]
pub struct ChildEdge {
    /// Install alias in `node_modules` (the manifest key under
    /// `dependencies` / `optionalDependencies`).
    pub alias: String,
    /// Resolved `pkgIdWithPatchHash` the alias points at.
    pub pkg_id: String,
    /// `true` when the edge came from `optionalDependencies`. Used
    /// to thread `current_is_optional` correctly through lazy
    /// realisation so the [`ResolvedPackage::optional`] AND-fold
    /// stays consistent with the eager-walk path.
    pub optional: bool,
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
/// that share a non-leaf resolved package each get their own per-
/// occurrence tree node with its own children edges; leaves collapse
/// onto one shared tree node (see [`DependenciesTree`]). Either way,
/// `ResolvedPackage` is the dedup-shared *envelope*, not a tree node.
#[derive(Debug, Clone)]
pub struct ResolvedPackage {
    pub id: String,
    /// Held as `Arc` so cloning a `ResolvedPackage` (which the
    /// per-occurrence tree walk does on every snapshot, and which
    /// the peer-resolution pass does when it carves
    /// `DependenciesGraphNode`s out of the resolved tree) is an
    /// `Arc::clone` instead of a deep copy of every `String` field
    /// on `ResolveResult` (id, alias, `resolved_via`, `name_ver`, ...).
    pub result: std::sync::Arc<ResolveResult>,
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
    /// `true` when the package's manifest has no `dependencies`,
    /// `optionalDependencies`, `peerDependencies`, or
    /// `peerDependenciesMeta`. Computed once on the first walk by
    /// `pkg_is_leaf` and reused by the peer resolver's
    /// `realize_children` so a lazy-realized child reuses the same
    /// leaf/non-leaf classification the eager walker picked — keeping
    /// `NodeId::leaf` vs `NodeId::next` consistent across both
    /// realisation paths. Mirrors upstream's
    /// [`ResolvedPackage.isLeaf`](https://github.com/pnpm/pnpm/blob/b9de85dcb6/installing/deps-resolver/src/resolveDependencies.ts#L250)
    /// — populated in
    /// [`getResolvedPackage`](https://github.com/pnpm/pnpm/blob/b9de85dcb6/installing/deps-resolver/src/resolveDependencies.ts#L1771)
    /// and consumed by
    /// [`buildTree`](https://github.com/pnpm/pnpm/blob/b9de85dcb6/installing/deps-resolver/src/resolveDependencyTree.ts#L381).
    pub is_leaf: bool,
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
#[derive(Debug, Clone)]
pub struct DependenciesTreeNode {
    /// Key into [`ResolvedTree::packages`].
    pub resolved_package_id: String,
    /// `alias → child NodeId` edges, possibly deferred.
    pub children: TreeChildren,
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

/// Children edges of a [`DependenciesTreeNode`].
///
/// Mirrors upstream's [`children: (() => ChildrenMap) | ChildrenMap`](https://github.com/pnpm/pnpm/blob/c86c423bdc/installing/deps-resolver/src/resolveDependencies.ts#L92-L94)
/// sum type. A node enters the tree as [`Self::Lazy`] when the
/// dependency-tree walker doesn't need to materialise its children
/// immediately (the common case for revisits, where the first walk
/// already populated `ResolvedTree::children_by_id`); the
/// peer-resolution stage flips it to [`Self::Realized`] on first
/// descent. Pure subtrees that the peer resolver short-circuits via
/// `purePkgs` never get realised at all.
#[derive(Debug, Clone)]
pub enum TreeChildren {
    /// `alias → child NodeId` map, fully populated. `BTreeMap` keeps
    /// iteration order stable so downstream peer-suffix construction
    /// is deterministic.
    Realized(BTreeMap<String, NodeId>),
    /// Children are known by spec only. `parent_ids` is the chain of
    /// `pkgIdWithPatchHash` ancestors this occurrence reached the
    /// node through, threaded so the peer resolver's `buildTree`
    /// equivalent can apply upstream's
    /// [`parentIdsContainSequence` cycle-break](https://github.com/pnpm/pnpm/blob/c86c423bdc/installing/deps-resolver/src/resolveDependencyTree.ts#L378)
    /// per-occurrence. Without it, a revisit's subtree would
    /// silently include cycle edges that the first walk correctly
    /// rejected, or omit valid edges the first walk's ancestor
    /// chain happened to exclude.
    Lazy { parent_ids: Arc<Vec<String>> },
}

impl TreeChildren {
    /// Empty realized children. Used for leaves so callers don't have
    /// to construct an empty `BTreeMap` themselves.
    #[must_use]
    pub fn empty() -> Self {
        TreeChildren::Realized(BTreeMap::new())
    }

    /// Borrow the realized children map.
    ///
    /// Panics on the [`Self::Lazy`] arm — callers that may encounter
    /// a lazy node must realize it first (peer-resolution does this
    /// via `Walker::realize_children`). Consumers that genuinely
    /// can't realize (e.g. the dependency-tree walker writing a
    /// fresh map) should match on the enum directly.
    #[must_use]
    pub fn realized(&self) -> &BTreeMap<String, NodeId> {
        match self {
            TreeChildren::Realized(map) => map,
            TreeChildren::Lazy { .. } => panic!(
                "TreeChildren::realized() called on a Lazy node; realize via the peer-resolver first",
            ),
        }
    }
}
