use std::sync::atomic::{AtomicU64, Ordering};

/// Per-occurrence identifier for a node in the [`DependenciesTree`].
/// Mirrors pnpm's [`NodeId`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/nextNodeId.ts).
///
/// Pnpm's `NodeId` is a branded `string | number` union: numbers come
/// from a monotonic counter; strings are `link:<rel-path>` for linked
/// local workspace packages. Pacquet stays in the numeric arm —
/// workspace-link resolution hasn't been ported yet, and the peer
/// resolver only needs equality / hashing of opaque ids.
///
/// [`DependenciesTree`]: super::resolved_tree::DependenciesTree
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(u64);

impl NodeId {
    /// Allocate a fresh `NodeId`. Mirrors pnpm's
    /// [`nextNodeId`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/nextNodeId.ts).
    pub fn next() -> NodeId {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        NodeId(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
