use std::collections::{BTreeMap, HashMap, HashSet};

use pacquet_resolving_resolver_base::ResolveResult;

use crate::resolved_tree::PeerDep;

/// Branded depPath string. Mirrors pnpm's
/// [`DepPath`](https://github.com/pnpm/pnpm/blob/097983fbca/packages/types/src/misc.ts).
/// Today this is a plain wrapper; future tightening could enforce the
/// `name@version(peer)*` shape at the parser boundary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DepPath(pub String);

impl DepPath {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for DepPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for DepPath {
    fn from(value: String) -> DepPath {
        DepPath(value)
    }
}

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
    pub resolve_result: ResolveResult,
    /// `alias → DepPath` edges to children + resolved peers. Children
    /// inherited from the per-occurrence tree node, peers added during
    /// peer resolution.
    pub children: BTreeMap<String, DepPath>,
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
