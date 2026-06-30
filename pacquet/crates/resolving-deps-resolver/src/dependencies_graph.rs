use std::collections::{BTreeMap, HashMap, HashSet};

use pacquet_deps_path::DepPath;
use pacquet_resolving_resolver_base::ResolveResult;

use crate::resolved_tree::PeerDep;

/// Post-peer-resolution graph keyed by depPath. Mirrors upstream's
/// [`GenericDependenciesGraphWithResolvedChildren`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolvePeers.ts#L66-L68).
///
/// Two snapshots of the same package can coexist here with different
/// keys (e.g. `react-dom@18.0.0(react@18.0.0)` and
/// `react-dom@18.0.0(react@17.0.0)`) — exactly the case the peer-
/// resolution stage exists to handle.
pub type DependenciesGraph = HashMap<DepPath, DependenciesGraphNode>;

/// One node in the [`DependenciesGraph`]. Mirrors upstream's
/// [`GenericDependenciesGraphNodeWithResolvedChildren`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolvePeers.ts#L50-L53).
#[derive(Debug, Clone)]
pub struct DependenciesGraphNode {
    pub dep_path: DepPath,
    /// The shared envelope, looked up via `ResolvedTree::packages`. Held
    /// by reference value (cloned) so consumers don't need a separate
    /// map lookup.
    pub resolved_package_id: String,
    /// Held as `Arc` so the graph's per-occurrence clones (one per
    /// `(dep_path, peer-suffix)` slot) reuse the same heap-allocated
    /// `ResolveResult` instead of deep-copying each field. The graph
    /// is built once and read by the install dispatch; nothing
    /// mutates the inner `ResolveResult` after `resolve_peers`.
    pub resolve_result: std::sync::Arc<ResolveResult>,
    /// `alias → DepPath` edges to children + resolved peers. Children
    /// inherited from the per-occurrence tree node, peers added during
    /// peer resolution.
    pub children: BTreeMap<String, DepPath>,
    /// Child aliases that originated from `optionalDependencies`.
    /// This is kept separately from [`Self::children`] because lockfile
    /// reuse may realize child edges from an existing `pnpm-lock.yaml`
    /// snapshot whose synthetic manifest no longer carries an
    /// `optionalDependencies` block.
    pub optional_children: HashSet<String>,
    pub peer_dependencies: BTreeMap<String, PeerDep>,
    /// Names of peers resolved from outside this node's own peer-deps —
    /// i.e. peers it inherited from its parents' context.
    pub transitive_peer_dependencies: HashSet<String>,
    /// Names of peers actually resolved (parents present in the chain).
    pub resolved_peer_names: HashSet<String>,
    pub depth: i32,
    pub installable: bool,
    /// `true` when this snapshot has zero unresolved + missing peers,
    /// i.e. its depPath equals its `pkgIdWithPatchHash`. Mirrors
    /// upstream's `isPure` flag.
    pub is_pure: bool,
    /// Mirrors [`crate::ResolvedPackage::optional`]: `true` when every
    /// path from any importer to this package goes through at least
    /// one `optionalDependencies` edge. Threaded through from the
    /// tree-walker so the lockfile adapter can set
    /// `SnapshotEntry.optional` per upstream's
    /// [`updateLockfile`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/updateLockfile.ts#L99-L101).
    /// Every peer-variant of the same `pkgIdWithPatchHash` shares the
    /// same value because they share one [`crate::ResolvedPackage`].
    pub optional: bool,
}

/// One issue collected during peer resolution. Mirrors upstream's per-
/// project
/// [`PeerDependencyIssues`](https://github.com/pnpm/pnpm/blob/097983fbca/packages/types/src/peerDependencyIssues.ts)
/// shape, simplified to the surface pacquet exposes today.
#[derive(Debug, Default, Clone)]
pub struct PeerDependencyIssues {
    /// `peerName → entries` where each entry describes a parent that
    /// requires the peer but a satisfying version isn't reachable.
    pub missing: HashMap<String, Vec<MissingPeer>>,
    /// `peerName → entries` where each entry describes a parent that
    /// requires the peer and a candidate exists but doesn't satisfy
    /// the range.
    pub bad: HashMap<String, Vec<PeerDependencyIssue>>,
}

/// One missing-peer entry. Mirrors upstream's `missing[peerName]` item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingPeer {
    pub wanted_range: String,
    pub optional: bool,
    /// `true` when the requiring package declares the peer only via
    /// `peerDependenciesMeta`. Pacquet-internal (upstream's issue item
    /// has no such field): the importer hoist loop uses it to keep
    /// meta-only peers out of the optional-peer hoist, mirroring
    /// upstream's `getMissingPeers`, which feeds the hoist from
    /// `peerDependencies` entries only.
    pub meta_only: bool,
    /// Chain of `(name, version)` from the root importer down to the
    /// parent that declared the peer requirement. Mirrors upstream's
    /// `parents: ParentPackages`.
    pub parents: Vec<ParentPackageRef>,
}

/// One bad-peer entry. Mirrors upstream's `bad[peerName]` item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerDependencyIssue {
    pub wanted_range: String,
    pub found_version: String,
    pub optional: bool,
    pub parents: Vec<ParentPackageRef>,
    /// Chain that brought the bad candidate into scope — `parents`
    /// describes who requires the peer; this describes where the bad
    /// version was found.
    pub resolved_from: Vec<ParentPackageRef>,
}

/// One `(name, version)` link in the chain returned with each peer
/// issue. Mirrors upstream's `ParentPackages` element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParentPackageRef {
    pub name: String,
    pub version: String,
}
