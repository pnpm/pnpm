use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// Per-occurrence identifier for a node in the [`DependenciesTree`].
/// Mirrors pnpm's [`NodeId`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/nextNodeId.ts).
///
/// Pnpm's `NodeId` is a branded `string | number` union: numbers come
/// from a monotonic counter; strings are reused for leaf packages (no
/// children, no peers) and for `link:<rel-path>` linked local
/// workspace packages. Pacquet ports the counter and leaf arms;
/// workspace-link resolution hasn't been ported yet.
///
/// Leaves share a single tree node across every parent that references
/// them — see [`resolveDependencies.ts:1580`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L1580)
/// for the upstream gate.
///
/// [`DependenciesTree`]: super::resolved_tree::DependenciesTree
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NodeId {
    /// Fresh per-occurrence counter value. Allocated by [`NodeId::next`].
    Counter(u64),
    /// Leaf package id reused across every occurrence of the same
    /// package. Allocated by [`NodeId::leaf`].
    Leaf(Arc<str>),
}

impl NodeId {
    /// Allocate a fresh per-occurrence `NodeId`. Mirrors pnpm's
    /// [`nextNodeId`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/nextNodeId.ts).
    pub fn next() -> NodeId {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        NodeId::Counter(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// Build a leaf `NodeId` from a package id. Every occurrence of a
    /// leaf shares the same `NodeId`, so the tree carries one node
    /// instead of one per parent edge.
    #[must_use]
    pub fn leaf(id: &str) -> NodeId {
        NodeId::Leaf(Arc::from(id))
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeId::Counter(n) => write!(f, "{n}"),
            NodeId::Leaf(id) => f.write_str(id),
        }
    }
}
