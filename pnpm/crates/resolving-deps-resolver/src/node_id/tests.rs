use super::NodeId;

/// The peer-resolution passes sort `NodeId`s to keep their output
/// independent of hash iteration order, so the order itself is part of
/// the resolved lockfile: counters first, in numeric (not textual)
/// order, then leaves by package id.
#[test]
fn sorts_counters_numerically_before_leaves() {
    let mut node_ids = vec![
        NodeId::leaf("zod@3.25.76"),
        NodeId::Counter(10),
        NodeId::leaf("react@19.2.8"),
        NodeId::Counter(2),
    ];
    node_ids.sort();
    dbg!(&node_ids);
    assert_eq!(
        node_ids,
        vec![
            NodeId::Counter(2),
            NodeId::Counter(10),
            NodeId::leaf("react@19.2.8"),
            NodeId::leaf("zod@3.25.76"),
        ],
    );
}
