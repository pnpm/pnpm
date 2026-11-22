use super::{
    super::{
        Arena,
        EngineOptions,
        WorkNode,
        graph::clone_tree,
    },
    nested_with_carried_names,
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

/// `shared` is nested in `top` below `left` and below `right`. `left` also
/// holds `only-left` and `right` holds `only-right`, so neither name is on
/// every path to `shared`, nor to `below`, which is nested in it. `shared`
/// depends on `top` again, and `top` is not nested in itself.
#[test]
fn a_node_nested_on_two_paths_carries_the_names_both_paths_hold() {
    let (root, top, left, right, shared) = (
        tree_node("."),
        tree_node("top"),
        tree_node("left"),
        tree_node("right"),
        tree_node("shared"),
    );
    let (only_left, only_right, below) =
        (tree_node("only-left"), tree_node("only-right"), tree_node("below"));
    depend(&root, &[&top]);
    depend(&top, &[&left, &right]);
    depend(&left, &[&shared, &only_left]);
    depend(&right, &[&shared, &only_right]);
    depend(&shared, &[&below, &top]);

    let limits = HoistingLimits::new();
    let options = EngineOptions {
        hoisting_limits: &limits,
        fast_lookup_possible: Cell::new(true),
        nodes: Arena::default(),
    };
    let work_top = child(&clone_tree(&root, &options), "top");
    let work_shared = child(&child(&work_top, "left"), "shared");

    let nested = nested_with_carried_names(&work_top);
    let carried_to = |node: &Rc<WorkNode>| -> Vec<String> {
        let (_, names) = &nested[&Rc::as_ptr(node)];
        let mut names: Vec<String> = names
            .iter()
            .map(ToString::to_string)
            .collect();
        names.sort_unstable();
        names
    };

    assert!(!nested.contains_key(&Rc::as_ptr(&work_top)));
    assert_eq!(carried_to(&work_shared), ["left", "right", "shared", "top"]);
    assert_eq!(
        carried_to(&child(&work_shared, "below")),
        ["below", "left", "right", "shared", "top"],
    );
    for node in [root, top, left, right, shared, only_left, only_right, below] {
        node.dependencies.borrow_mut().clear();
    }
}
