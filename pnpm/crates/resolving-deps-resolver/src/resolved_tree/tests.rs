use std::sync::Arc;

#[test]
fn tree_node_keeps_its_per_occurrence_footprint_small() {
    assert!(
        size_of::<super::DependenciesTreeNode>() <= 48,
        "DependenciesTreeNode grew to {} bytes; box rarely-set state instead of inlining it",
        size_of::<super::DependenciesTreeNode>(),
    );
}

#[test]
fn locked_resolution_is_unallocated_until_written() {
    let mut node = super::DependenciesTreeNode::new(
        Arc::from("a@1.0.0".to_string()),
        super::TreeChildren::Realized(Arc::new(std::collections::BTreeMap::new())),
        0,
        true,
    );

    assert!(node.locked.is_none(), "a fresh resolution carries no wanted-lockfile state");
    assert!(node.previous_dep_path().is_none());
    assert!(node.locked_peer_context().is_none());
    assert!(node.must_win_dependency_names().is_none());
    assert!(node.has_no_locked_peer_context());

    node.locked_mut().previous_dep_path = Some(pnpm_deps_path::DepPath::from("a@1.0.0"));

    assert_eq!(
        node.previous_dep_path(),
        Some(&pnpm_deps_path::DepPath::from("a@1.0.0")),
        "the accessor reads back what `locked_mut` wrote",
    );
    assert!(node.has_no_locked_peer_context(), "a previous depPath is not a locked peer");
}
