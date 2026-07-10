use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// Per-occurrence identifier for a node in the [`DependenciesTree`].
///
/// A [`NodeId`] is either a number from a monotonic counter or a string
/// reused for leaf packages (no children, no peers). String ids for
/// `link:<rel-path>` linked local workspace packages aren't produced
/// yet — workspace-link resolution hasn't been implemented.
///
/// Leaves share a single tree node across every parent that references
/// them.
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
    /// Allocate a fresh per-occurrence [`NodeId`].
    pub fn next() -> NodeId {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        NodeId::Counter(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// Build a leaf [`NodeId`] from a package id. Every occurrence of a
    /// leaf shares the same [`NodeId`], so the tree carries one node
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
