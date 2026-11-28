use super::{
    Arc,
    HashSet,
    ParentPkgAliases,
    RecordedChildren,
    RecordedChildrenContext,
};

impl RecordedChildren {
    /// Children recorded by a walk whose context a test does not vary.
    pub(in crate::resolve_dependency_tree) fn with_edges(
        edges: Vec<crate::resolved_tree::ChildEdge>,
    ) -> Self {
        RecordedChildren {
            edges: Arc::new(edges),
            context: RecordedChildrenContext {
                peer_shadowed: Arc::default(),
                prior_key: None,
                reused: false,
                update_active: false,
                children_scope: ParentPkgAliases::root(HashSet::default()),
            },
        }
    }
}
