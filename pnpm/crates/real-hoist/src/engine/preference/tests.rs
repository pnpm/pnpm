use super::{
    super::{
        EngineOptions,
        WorkNode,
        graph::clone_tree,
    },
    hoist_ident_map,
};
use crate::{
    HoisterDependencyKind,
    HoisterTree,
    HoistingLimits,
    RcByPtr,
};
use std::{
    cell::{
        Cell,
        RefCell,
    },
    rc::Rc,
};

fn tree_node(
    name: &str,
    reference: &str,
    peers: &[&str],
    deps: Vec<Rc<HoisterTree>>,
) -> Rc<HoisterTree> {
    Rc::new(HoisterTree {
        identity: None,
        name: name.to_string(),
        ident_name: name.to_string(),
        reference: reference.to_string(),
        peer_names: peers
            .iter()
            .map(|&peer| peer.to_string())
            .collect(),
        dependency_kind: HoisterDependencyKind::Regular,
        hoist_priority: 0,
        dependencies: RefCell::new(deps.into_iter().map(RcByPtr).collect()),
    })
}

fn work_root(tree: &HoisterTree) -> Rc<WorkNode> {
    let limits = HoistingLimits::new();
    clone_tree(
        tree,
        &EngineOptions {
            hoisting_limits: &limits,
            fast_lookup_possible: Cell::new(true),
            nodes: super::super::Arena::default(),
        },
    )
}

/// A name the root takes as a peer stays out of the map, even when a
/// regular descendant reaches it (upstream's `!rootNode.peerNames.has(name)`
/// in `getHoistIdentMap`).
#[test]
fn root_peer_names_are_not_candidates() {
    let react = tree_node("react", "react@18.0.0", &[], vec![]);
    let app = tree_node("app", "app@1.0.0", &[], vec![react]);
    let root = work_root(&tree_node(".", "", &["react"], vec![app]));

    let idents = hoist_ident_map(&root);
    assert!(idents.contains_key("app"), "the regular dependency is a candidate");
    assert!(!idents.contains_key("react"), "the root's peer is not: {idents:?}");
}

/// A node's peer is recorded as a candidate, but the walk does not descend
/// into it, so what only the peer depends on never becomes one (upstream's
/// `entry.peerDependents.add` branch of `buildPreferenceMap`).
#[test]
fn a_peer_is_a_candidate_but_its_subtree_is_not_walked() {
    let scheduler = tree_node("scheduler", "scheduler@1.0.0", &[], vec![]);
    let react = tree_node("react", "react@18.0.0", &[], vec![scheduler]);
    let app = tree_node("app", "app@1.0.0", &["react"], vec![react]);
    let root = work_root(&tree_node(".", "", &[], vec![app]));

    let idents = hoist_ident_map(&root);
    assert!(idents.contains_key("app"));
    assert!(idents.contains_key("react"), "the peer is a candidate: {idents:?}");
    assert!(!idents.contains_key("scheduler"), "the peer's subtree is not walked: {idents:?}");
}
