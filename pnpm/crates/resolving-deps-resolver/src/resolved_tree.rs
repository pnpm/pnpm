use crate::node_id::NodeId;
use pnpm_deps_path::DepPath;
use pnpm_resolving_resolver_base::{ResolutionPolicyViolation, ResolveResult};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{collections::BTreeMap, sync::Arc};

/// Per-occurrence tree carried by [`ResolvedTree::dependencies_tree`].
pub type DependenciesTree = HashMap<NodeId, DependenciesTreeNode>;

/// Output of [`fn@crate::resolve_dependency_tree`].
///
/// The shape carries two indices into the same set of resolved
/// packages:
///
/// - [`packages`](Self::packages) is the **flat dedup map**, keyed by
///   `pkgIdWithPatchHash` (today `name@version`). One entry per
///   resolved package, no per-occurrence repetition.
/// - [`dependencies_tree`](Self::dependencies_tree) is the **per-
///   occurrence tree**, keyed by [`NodeId`]. Non-leaf nodes get a fresh
///   child `NodeId` per parent occurrence so the peer-resolution stage
///   can compute different peer suffixes per call site. Leaves (no
///   `dependencies`, `optionalDependencies`, `peerDependencies`, or
///   `peerDependenciesMeta`) collapse onto one shared `NodeId`: a leaf
///   has no per-occurrence state worth distinguishing, so every parent
///   that references it points at the same tree node.
#[derive(Debug, Default, Clone)]
pub struct ResolvedTree {
    pub direct: Vec<DirectDep>,
    pub packages: HashMap<Arc<str>, ResolvedPackage>,
    pub dependencies_tree: DependenciesTree,
    pub all_peer_dep_names: HashSet<String>,
    pub policy_violations: Vec<ResolutionPolicyViolation>,
    /// Set of `patchedDependencies` keys (e.g. `lodash@4.17.21`,
    /// `react@^18`) whose patch was actually applied to at least one
    /// resolved package. Threaded out of the resolver so the
    /// orchestrator can pass it to [`pnpm_patching::verify_patches`]
    /// for the `ERR_PNPM_UNUSED_PATCH` diagnostic.
    pub applied_patches: HashSet<String>,
    /// Per-`pkgIdWithPatchHash` child list: `(install_alias,
    /// resolved_child_pkg_id, optional)`. Populated by the first walk
    /// of each package — every subsequent revisit reuses the same
    /// entry. The peer-resolver's `realize_children` walks this to
    /// allocate per-occurrence `NodeId`s for a
    /// [`TreeChildren::Lazy`] node.
    pub children_by_id: HashMap<Arc<str>, Arc<Vec<ChildEdge>>>,
}

/// One entry on [`ResolvedTree::children_by_id`] — the resolved
/// shape of a package's children list as recorded by the first walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildEdge {
    /// Install alias in `node_modules` (the manifest key under
    /// `dependencies` / `optionalDependencies`).
    pub alias: String,
    /// Resolved `pkgIdWithPatchHash` the alias points at. Shared: the
    /// peer walk copies this onto every occurrence node it realizes,
    /// millions of them for a few thousand distinct ids.
    pub pkg_id: Arc<str>,
    /// `true` when the edge came from `optionalDependencies`. Used
    /// to thread `current_is_optional` correctly through lazy
    /// realisation so the [`ResolvedPackage::optional`] AND-fold
    /// stays consistent with the eager-walk path.
    pub optional: bool,
}

/// Ancestor package ids for a lazy occurrence. The dependency walk keeps its
/// already-built contiguous vector as the base, while peer discovery appends
/// shallow vectors of shared string storage instead of copying every package
/// id for each context-sensitive revisit.
#[derive(Debug, Default, Clone)]
pub struct AncestorIds {
    base: Arc<Vec<String>>,
    appended: Arc<Vec<Arc<str>>>,
}

impl AncestorIds {
    /// The dependency walk's contiguous base ids, in order.
    pub fn base_ids(&self) -> impl Iterator<Item = &str> {
        self.base.iter().map(String::as_str)
    }

    /// The ids peer discovery appended after the base, in order.
    pub fn appended_ids(&self) -> impl Iterator<Item = &str> {
        self.appended.iter().map(|id| &**id)
    }

    #[must_use]
    pub fn pushed(&self, id: String) -> Self {
        let mut appended = Vec::with_capacity(self.appended.len() + 1);
        appended.extend(self.appended.iter().cloned());
        appended.push(Arc::from(id));
        Self { base: Arc::clone(&self.base), appended: Arc::new(appended) }
    }
}

impl From<Arc<Vec<String>>> for AncestorIds {
    fn from(base: Arc<Vec<String>>) -> Self {
        Self { base, appended: Arc::new(Vec::new()) }
    }
}

/// One edge in the resolved tree: the local install name (`alias`) and
/// the resolved node's [`NodeId`], plus the resolved `pkgId`.
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

/// One resolved package, deduped by `pkgIdWithPatchHash`.
///
/// **Children live on [`DependenciesTreeNode`], not here.** Two parents
/// that share a non-leaf resolved package each get their own per-
/// occurrence tree node with its own children edges; leaves collapse
/// onto one shared tree node (see [`DependenciesTree`]). Either way,
/// [`ResolvedPackage`] is the dedup-shared *envelope*, not a tree node.
#[derive(Debug, Clone)]
pub struct ResolvedPackage {
    pub id: Arc<str>,
    /// Held as `Arc` so cloning a [`ResolvedPackage`] (which the
    /// per-occurrence tree walk does on every snapshot, and which
    /// the peer-resolution pass does when it carves
    /// `DependenciesGraphNode`s out of the resolved tree) is an
    /// `Arc::clone` instead of a deep copy of every `String` field
    /// on `ResolveResult` (id, alias, `resolved_via`, `name_ver`, ...).
    pub result: std::sync::Arc<ResolveResult>,
    /// `peerDependencies` from the package's manifest, with names that
    /// also appear in the package's own `dependencies` /
    /// `optionalDependencies` filtered out. `BTreeMap` keeps iteration
    /// order stable so peer-suffix construction is deterministic.
    pub peer_dependencies: BTreeMap<String, PeerDep>,
    /// `true` when every path from any importer to this package goes
    /// through at least one `optionalDependencies` edge, computed by
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
    /// realisation paths.
    pub is_leaf: bool,
}

/// One peer-dependency entry on a [`ResolvedPackage`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerDep {
    /// Semver range from the package's manifest. May carry a
    /// `workspace:` prefix that the peer matcher strips before
    /// checking.
    pub version: String,
    /// `true` when the manifest's `peerDependenciesMeta[name].optional`
    /// is set — a missing peer with `optional` true is recorded as an
    /// issue but does not block resolution.
    pub optional: bool,
}

/// The wanted-lockfile carry-over a [`DependenciesTreeNode`] may hold.
///
/// Split out of the node so the common case — a fresh resolution, with
/// none of these set — costs one null pointer instead of four `Option`s.
#[derive(Debug, Default, Clone)]
pub struct LockedResolution {
    /// `DepPath` this occurrence's package resolved to in the wanted
    /// lockfile, when its resolution was reused from it. Feeds the
    /// reverse index that locked-peer-provider reuse looks providers
    /// up by. `None` for fresh resolutions.
    ///
    /// TODO: the resolver does not capture this from the wanted
    /// lockfile yet; only `resolve_peers` consumes it.
    pub previous_dep_path: Option<DepPath>,
    /// `peer name → provider DepPath` bindings the wanted lockfile
    /// recorded for this package's snapshot. A second peer-resolution
    /// pass ([`crate::ResolvePeersOptions::resolved_peer_provider_paths`])
    /// re-pins a still-compatible locked provider so re-installs keep
    /// the provider choice stable.
    ///
    /// TODO: the resolver does not capture this from the wanted
    /// lockfile yet; only `resolve_peers` consumes it.
    pub locked_peer_context: Option<BTreeMap<String, DepPath>>,
    /// Child aliases whose resolution changed against the wanted
    /// lockfile. A locked peer provider reachable through one of these
    /// aliases loses to the current provider.
    ///
    /// TODO: the resolver does not compute this yet; only
    /// `resolve_peers` consumes it.
    pub dependency_names_whose_current_provider_must_win: Option<HashSet<String>>,
}

/// One per-occurrence node in the dependencies tree.
#[derive(Debug, Clone)]
pub struct DependenciesTreeNode {
    /// Key into [`ResolvedTree::packages`]. Shared rather than owned —
    /// see [`ChildEdge::pkg_id`], which is where most of these come from.
    pub resolved_package_id: Arc<str>,
    /// `alias → child NodeId` edges, possibly deferred.
    pub children: TreeChildren,
    /// Distance from the root importer (root = 0). A `depth = -1` marks
    /// linked / pruned nodes; pacquet doesn't emit `-1` today because
    /// workspace-link resolution hasn't been implemented.
    pub depth: i32,
    /// Whether the package may be skipped when an optional dep fails
    /// for its host platform. Always `true` for the npm-shaped slice
    /// pacquet currently exposes.
    pub installable: bool,
    /// Wanted-lockfile carry-over for this occurrence, boxed because it
    /// is `None` for every fresh resolution — which is every node the
    /// resolver produces today. Inline, its three rarely-set fields cost
    /// ~80 bytes on each of the millions of nodes the peer walk realizes.
    pub locked: Option<Box<LockedResolution>>,
}

impl DependenciesTreeNode {
    /// Node with no wanted-lockfile carry-over (a fresh resolution).
    #[must_use]
    pub fn new(
        resolved_package_id: Arc<str>,
        children: TreeChildren,
        depth: i32,
        installable: bool,
    ) -> Self {
        DependenciesTreeNode { resolved_package_id, children, depth, installable, locked: None }
    }

    /// Wanted-lockfile `DepPath` for this occurrence, if it carried one.
    #[must_use]
    pub fn previous_dep_path(&self) -> Option<&DepPath> {
        self.locked.as_ref()?.previous_dep_path.as_ref()
    }

    /// Locked `peer name → provider DepPath` bindings, if any.
    #[must_use]
    pub fn locked_peer_context(&self) -> Option<&BTreeMap<String, DepPath>> {
        self.locked.as_ref()?.locked_peer_context.as_ref()
    }

    /// Child aliases whose current provider must win over a locked one.
    #[must_use]
    pub fn must_win_dependency_names(&self) -> Option<&HashSet<String>> {
        self.locked.as_ref()?.dependency_names_whose_current_provider_must_win.as_ref()
    }

    /// `true` when no locked peer context is recorded — the fast-cache
    /// precondition.
    #[must_use]
    pub fn has_no_locked_peer_context(&self) -> bool {
        self.locked.as_ref().is_none_or(|locked| locked.locked_peer_context.is_none())
    }

    /// The carry-over slot, allocated on first write.
    pub fn locked_mut(&mut self) -> &mut LockedResolution {
        self.locked.get_or_insert_with(Box::default)
    }
}

/// Children edges of a [`DependenciesTreeNode`].
///
/// A node enters the tree as [`Self::Lazy`] when the
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
    /// Shared: a node's realized children are immutable once built, and
    /// the peer walk hands the same map to every revisit of the node.
    Realized(Arc<BTreeMap<String, NodeId>>),
    /// Children are known by spec only. `parent_ids` is the chain of
    /// `pkgIdWithPatchHash` ancestors this occurrence reached the
    /// node through, excluding the node itself. The reader appends the
    /// node from `resolved_package_id`, which lets all siblings share one
    /// chain allocation. The chain is threaded so the peer resolver can
    /// apply the parent-ids-contain-sequence cycle-break
    /// per-occurrence. Without it, a revisit's subtree would
    /// silently include cycle edges that the first walk correctly
    /// rejected, or omit valid edges the first walk's ancestor
    /// chain happened to exclude.
    Lazy { parent_ids: AncestorIds },
}

impl TreeChildren {
    /// Empty realized children. Used for leaves so callers don't have
    /// to construct an empty `BTreeMap` themselves.
    #[must_use]
    pub fn empty() -> Self {
        TreeChildren::Realized(Arc::new(BTreeMap::new()))
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

#[cfg(test)]
mod tests;
