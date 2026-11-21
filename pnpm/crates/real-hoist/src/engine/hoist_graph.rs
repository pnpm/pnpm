//! One pass of moving dependencies to a hoisting root: upstream's
//! `hoistGraph`.

use super::{
    WorkNode,
    graph::decouple,
    hoist_info::{
        Hoistable,
        RootContext,
        ShadowedNodes,
        node_hoist_info,
        sorted_regular_dependencies,
    },
};
use crate::{
    HoisterDependencyKind,
    RcByPtr,
};
use indexmap::IndexSet;
use rustc_hash::{
    FxHashMap,
    FxHashSet,
};
use std::rc::Rc;

/// Upstream's `{ anotherRoundNeeded, isGraphChanged }`.
#[derive(Default, Clone, Copy)]
pub(super) struct PassOutcome {
    pub(super) another_round_needed: bool,
    pub(super) is_graph_changed: bool,
}

impl PassOutcome {
    pub(super) fn absorb(&mut self, other: PassOutcome) {
        self.another_round_needed |= other.another_round_needed;
        self.is_graph_changed |= other.is_graph_changed;
    }
}

/// State shared by every `hoistNodeDependencies` call of one pass.
struct Pass<'pass> {
    ctx: &'pass RootContext<'pass>,
    parent_shadowed: &'pass ShadowedNodes,
    /// Parents on the current recursion path, upstream's `seenNodes`.
    on_path: FxHashSet<*const WorkNode>,
    /// The hoisting root, then the nodes from it down to the parent
    /// whose children are being decided.
    node_path: Vec<Rc<WorkNode>>,
    /// The aliased locators from the top of the tree down to that parent.
    aliased_path: Vec<Rc<str>>,
    /// Nodes hoisted to the root in this wave, walked in the next one.
    next_wave: IndexSet<RcByPtr<WorkNode>>,
    outcome: PassOutcome,
}

/// Upstream's `hoistGraph`. `root_path` runs from the top of the tree to
/// the hoisting root, which is its last entry.
pub(super) fn hoist_graph(
    ctx: &RootContext<'_>,
    root_path: &[Rc<WorkNode>],
    parent_shadowed: &ShadowedNodes,
) -> PassOutcome {
    let mut pass = Pass {
        ctx,
        parent_shadowed,
        on_path: FxHashSet::default(),
        node_path: vec![Rc::clone(ctx.root)],
        aliased_path: root_path
            .iter()
            .map(|node| Rc::clone(&node.input.aliased_locator))
            .collect(),
        next_wave: sorted_regular_dependencies(ctx.root)
            .into_iter()
            .map(RcByPtr)
            .collect(),
        outcome: PassOutcome::default(),
    };
    while !pass.next_wave.is_empty() {
        let wave = std::mem::take(&mut pass.next_wave);
        for dep in wave {
            if dep.input.ident == ctx.root.input.ident || dep.is_hoist_border.get() {
                continue;
            }
            let decoupled = decouple(ctx.root, &dep.0, ctx.arena);
            hoist_node_dependencies(&mut pass, &decoupled);
        }
    }
    pass.outcome
}

/// Upstream's `hoistNodeDependencies`: move what can move from `parent`
/// to the root, then descend into the children that stayed.
fn hoist_node_dependencies(pass: &mut Pass<'_>, parent: &Rc<WorkNode>) {
    if pass.on_path.contains(&Rc::as_ptr(parent)) {
        return;
    }
    pass.node_path.push(Rc::clone(parent));
    pass.aliased_path.push(Rc::clone(&parent.input.aliased_locator));
    let children = sorted_regular_dependencies(parent);
    let unhoistable = decide(pass, parent, &children);
    move_hoistable(pass, parent, &children, &unhoistable);
    for child in staying_children(parent, children, &unhoistable) {
        let stays = unhoistable.contains(&RcByPtr(Rc::clone(&child)));
        if !stays
            || child.is_hoist_border.get()
            || pass.aliased_path.contains(&child.input.aliased_locator)
        {
            continue;
        }
        pass.on_path.insert(Rc::as_ptr(parent));
        let decoupled = decouple(parent, &child, pass.ctx.arena);
        hoist_node_dependencies(pass, &decoupled);
        pass.on_path.remove(&Rc::as_ptr(parent));
    }
    pass.aliased_path.pop();
    pass.node_path.pop();
}

/// The children to descend into, in the order upstream gets by sorting
/// what stayed again. Only a child that takes a peer can change that order
/// from the one `children` has, so the sort is repeated only then.
fn staying_children(
    parent: &WorkNode,
    children: Vec<Rc<WorkNode>>,
    unhoistable: &IndexSet<RcByPtr<WorkNode>>,
) -> Vec<Rc<WorkNode>> {
    if unhoistable.is_empty() {
        return Vec::new();
    }
    let has_peers = children
        .iter()
        .any(|child| !child.input.peer_names.is_empty());
    if has_peers { sorted_regular_dependencies(parent) } else { children }
}

/// The hoist decision for each of `children`, the regular children of
/// `parent`. Returns the children that stay, after spreading a refusal to
/// every sibling that waited on the refused node as a peer provider.
fn decide(
    pass: &Pass<'_>,
    parent: &WorkNode,
    children: &[Rc<WorkNode>],
) -> IndexSet<RcByPtr<WorkNode>> {
    let mut refused: Vec<RcByPtr<WorkNode>> = Vec::new();
    let mut dependants: FxHashMap<Rc<str>, Vec<Rc<str>>> = FxHashMap::default();
    for child in children {
        match node_hoist_info(pass.ctx, &pass.node_path, child) {
            Hoistable::Yes => {}
            Hoistable::No => refused.push(RcByPtr(Rc::clone(child))),
            Hoistable::Depends(providers) => await_providers(&mut dependants, child, &providers),
        }
    }
    let mut unhoistable: IndexSet<RcByPtr<WorkNode>> = IndexSet::new();
    for node in refused {
        refuse(parent, &node, &dependants, &mut unhoistable);
    }
    unhoistable
}

/// Record `child` as waiting on each of `providers`, by name.
fn await_providers(
    dependants: &mut FxHashMap<Rc<str>, Vec<Rc<str>>>,
    child: &WorkNode,
    providers: &[Rc<WorkNode>],
) {
    for provider in providers {
        dependants
            .entry(Rc::clone(&provider.input.name))
            .or_default()
            .push(Rc::clone(&child.input.name));
    }
}

/// Upstream's `addUnhoistableNode`: a refused node takes down every
/// sibling that waited for it.
fn refuse(
    parent: &WorkNode,
    node: &RcByPtr<WorkNode>,
    dependants: &FxHashMap<Rc<str>, Vec<Rc<str>>>,
    unhoistable: &mut IndexSet<RcByPtr<WorkNode>>,
) {
    let mut pending = vec![node.clone()];
    while let Some(node) = pending.pop() {
        if !unhoistable.insert(node.clone()) {
            continue;
        }
        for dependant in dependants
            .get(&node.input.name)
            .into_iter()
            .flatten()
        {
            if let Some(waiting) = parent.dependency(dependant) {
                pending.push(RcByPtr(waiting));
            }
        }
    }
}

/// Move each of `children` that may leave `parent` to the root, or merge
/// it into the copy the root already holds.
fn move_hoistable(
    pass: &mut Pass<'_>,
    parent: &Rc<WorkNode>,
    children: &[Rc<WorkNode>],
    unhoistable: &IndexSet<RcByPtr<WorkNode>>,
) {
    let mut moved = false;
    for child in children {
        if unhoistable.contains(&RcByPtr(Rc::clone(child))) {
            continue;
        }
        let name = &child.input.name;
        moved = true;
        pass.outcome.is_graph_changed = true;
        let shadowed = pass.parent_shadowed.get(&Rc::as_ptr(parent));
        if shadowed.is_some_and(|names| names.contains(name)) {
            pass.outcome.another_round_needed = true;
        }
        parent.dependencies.borrow_mut().shift_remove(name);
        parent.hoisted_dependencies
            .borrow_mut()
            .insert(Rc::clone(name), Rc::clone(child));
        place_at_root(pass, child);
    }
    if moved && parent.input.dependency_kind == HoisterDependencyKind::ExternalSoftLink {
        pass.outcome.another_round_needed = true;
    }
}

fn place_at_root(pass: &mut Pass<'_>, child: &Rc<WorkNode>) {
    let root = pass.ctx.root;
    let name = &child.input.name;
    if let Some(existing) = root.dependency(name) {
        if !Rc::ptr_eq(&existing, child) {
            let references = child.references.borrow();
            existing.references
                .borrow_mut()
                .extend(references.iter().map(Rc::clone));
        }
    } else if root.input.ident != child.input.ident {
        root.dependencies
            .borrow_mut()
            .insert(Rc::clone(name), Rc::clone(child));
        pass.next_wave.insert(RcByPtr(Rc::clone(child)));
    }
}
