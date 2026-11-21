//! Hoisting into one root and then into each of its children: upstream's
//! `hoistTo` and the used-dependency maps it builds.

use super::{
    EngineOptions,
    WorkNode,
    graph::regular_dependencies,
    hoist_graph::{
        PassOutcome,
        hoist_graph,
    },
    hoist_info::{
        RootContext,
        ShadowedNodes,
    },
    preference::{
        front_idents,
        hoist_ident_map,
        shift_unplaced,
    },
};
use rustc_hash::{
    FxHashMap,
    FxHashSet,
};
use std::{
    cell::RefCell,
    rc::Rc,
};

type Names = FxHashSet<Rc<str>>;

/// One round of upstream's `hoist` loop: `hoistTo` from the top root.
pub(super) fn hoist_to(top: &Rc<WorkNode>, options: &EngineOptions<'_>) -> PassOutcome {
    let mut path_locators: FxHashSet<Rc<str>> = FxHashSet::from_iter([Rc::clone(&top.input.ident)]);
    hoist_to_root(&[Rc::clone(top)], &mut path_locators, &ShadowedNodes::default(), options)
}

/// Upstream's `hoistTo`. `root_path` ends with the hoisting root;
/// `path_locators` holds the locators of every root on it.
fn hoist_to_root(
    root_path: &[Rc<WorkNode>],
    path_locators: &mut FxHashSet<Rc<str>>,
    parent_shadowed: &ShadowedNodes,
    options: &EngineOptions<'_>,
) -> PassOutcome {
    let Some(root) = root_path.last() else { return PassOutcome::default() };
    let shadowed = RefCell::new(ShadowedNodes::default());
    let mut outcome = hoist_into(root, root_path, parent_shadowed, &shadowed, options);
    let shadowed = shadowed.into_inner();
    let names: Vec<Rc<str>> = root.dependencies
        .borrow()
        .keys()
        .cloned()
        .collect();
    for name in names {
        // Upstream iterates the live map, so a child replaced since is
        // visited as it is now.
        let Some(dependency) = root.dependency(&name) else { continue };
        let locator = &dependency.input.ident;
        if root.input.peer_names.contains(&name) || path_locators.contains(locator) {
            continue;
        }
        path_locators.insert(Rc::clone(locator));
        let mut child_path = root_path.to_vec();
        child_path.push(Rc::clone(&dependency));
        outcome.absorb(hoist_to_root(&child_path, path_locators, &shadowed, options));
        path_locators.remove(locator);
    }
    outcome
}

/// The `hoistGraph` passes of one `hoistTo` call, repeated while the
/// preferred version of some name failed to reach the root.
fn hoist_into(
    root: &Rc<WorkNode>,
    root_path: &[Rc<WorkNode>],
    parent_shadowed: &ShadowedNodes,
    shadowed: &RefCell<ShadowedNodes>,
    options: &EngineOptions<'_>,
) -> PassOutcome {
    let mut ident_map = hoist_ident_map(root);
    let mut hoist_idents = front_idents(&ident_map);
    let is_top = root_path.len() == 1;
    let fast_lookup_possible = options.fast_lookup_possible.get();
    let used = match (is_top, fast_lookup_possible) {
        (true, _) => FxHashMap::default(),
        (false, true) => zero_round_used_dependencies(root),
        (false, false) => used_dependencies(root_path),
    };
    let mut outcome = PassOutcome::default();
    loop {
        let ctx = RootContext {
            root,
            used: &used,
            hoist_idents: &hoist_idents,
            shadowed,
            fast_lookup_possible,
            arena: &options.nodes,
        };
        outcome.absorb(hoist_graph(&ctx, root_path, parent_shadowed));
        if !shift_unplaced(&mut ident_map, root, &mut hoist_idents) {
            return outcome;
        }
    }
}

/// Upstream's `getZeroRoundUsedDependencies`: every name the root's
/// subtree has had hoisted out of it, so resolves from above.
fn zero_round_used_dependencies(root: &Rc<WorkNode>) -> FxHashMap<Rc<str>, Rc<WorkNode>> {
    let mut used = FxHashMap::default();
    for node in preorder(root) {
        for (name, dep) in node.hoisted_dependencies.borrow().iter() {
            used.insert(Rc::clone(name), Rc::clone(dep));
        }
    }
    used
}

/// Every node reachable from `root` through regular edges, each once, in
/// the preorder of upstream's recursive walks.
fn preorder(root: &Rc<WorkNode>) -> Vec<Rc<WorkNode>> {
    let mut seen: FxHashSet<*const WorkNode> = FxHashSet::default();
    let mut order = Vec::new();
    let mut pending = vec![Rc::clone(root)];
    while let Some(node) = pending.pop() {
        if !seen.insert(Rc::as_ptr(&node)) {
            continue;
        }
        pending.extend(regular_dependencies(&node).into_iter().rev());
        order.push(node);
    }
    order
}

/// Upstream's `getUsedDependencies`: for each name hoisted out of the
/// root's subtree and not shadowed by a sibling of the node it left, the
/// copies the root path provides.
fn used_dependencies(root_path: &[Rc<WorkNode>]) -> FxHashMap<Rc<str>, Rc<WorkNode>> {
    let mut used = FxHashMap::default();
    let Some(root) = root_path.last() else { return used };
    let mut seen: FxHashSet<*const WorkNode> = FxHashSet::default();
    let mut pending: Vec<(Rc<WorkNode>, Rc<Names>)> = vec![(Rc::clone(root), Rc::default())];
    while let Some((node, hidden)) = pending.pop() {
        if !seen.insert(Rc::as_ptr(&node)) {
            continue;
        }
        add_reachable(&mut used, root_path, &node, &hidden);
        let children_hidden: Rc<Names> = Rc::new(
            node.dependencies
                .borrow()
                .keys()
                .cloned()
                .collect(),
        );
        pending.extend(
            regular_dependencies(&node)
                .into_iter()
                .rev()
                .map(|child| (child, Rc::clone(&children_hidden))),
        );
    }
    used
}

/// For each name hoisted out of `node` and not in `hidden`, the copy held
/// nearest the end of `root_path`. Upstream walks the path from the top
/// and lets each holder overwrite the last.
fn add_reachable(
    used: &mut FxHashMap<Rc<str>, Rc<WorkNode>>,
    root_path: &[Rc<WorkNode>],
    node: &WorkNode,
    hidden: &Names,
) {
    let hoisted = node.hoisted_dependencies.borrow();
    let visible = hoisted
        .keys()
        .filter(|name| !hidden.contains(*name));
    for name in visible {
        let reachable = root_path
            .iter()
            .rev()
            .find_map(|ancestor| ancestor.dependency(name));
        if let Some(reachable) = reachable {
            used.insert(Rc::clone(name), reachable);
        }
    }
}
