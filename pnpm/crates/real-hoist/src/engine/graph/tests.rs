use super::{
    super::{
        Arena,
        EngineOptions,
        WorkNode,
    },
    clone_tree,
};
use crate::{
    HoisterDependencyKind,
    HoisterTree,
    HoistingLimits,
    RcByPtr,
};
use indexmap::IndexSet;
use std::{
    cell::{
        Cell,
        RefCell,
    },
    rc::Rc,
};

fn tree_node(name: &str) -> Rc<HoisterTree> {
    Rc::new(HoisterTree {
        identity: None,
        name: name.to_string(),
        ident_name: name.to_string(),
        reference: format!("{name}@1.0.0"),
        peer_names: IndexSet::new(),
        dependency_kind: HoisterDependencyKind::Regular,
        hoist_priority: 0,
        dependencies: RefCell::default(),
    })
}

fn depend(parent: &HoisterTree, children: &[&Rc<HoisterTree>]) {
    let mut dependencies = parent.dependencies.borrow_mut();
    for child in children {
        dependencies.insert(RcByPtr(Rc::clone(child)));
    }
}

fn child(node: &WorkNode, name: &str) -> Rc<WorkNode> {
    node.dependency(name)
        .unwrap_or_else(|| panic!("{} has no child {name}", node.input.name))
}

/// `b` is first seen again from inside its own cycle, while it is still
/// taking children: `c` is added to it only after that walk. The sighting
/// through `x` has to walk `b` again and reach `c`, as upstream does.
#[test]
fn a_node_seen_again_while_open_is_walked_again_later() {
    let (root, first, second, third, other) =
        (tree_node("."), tree_node("a"), tree_node("b"), tree_node("c"), tree_node("x"));
    depend(&root, &[&first, &other]);
    depend(&first, &[&second]);
    depend(&second, &[&first, &third]);
    depend(&other, &[&second]);

    let limits = HoistingLimits::new();
    let options = EngineOptions {
        hoisting_limits: &limits,
        fast_lookup_possible: Cell::new(true),
        nodes: Arena::default(),
    };
    let work_root = clone_tree(&root, &options);

    let work_first = child(&work_root, "a");
    let work_second = child(&work_first, "b");
    assert!(!work_first.decoupled.get(), "a is reached through the root and through b");
    assert!(!work_second.decoupled.get(), "b is reached through a and through x");
    assert!(!child(&work_second, "c").decoupled.get(), "c is below b");
    assert!(child(&work_root, "x").decoupled.get(), "x has one parent");
    for node in [root, first, second, third, other] {
        node.dependencies.borrow_mut().clear();
    }
}
