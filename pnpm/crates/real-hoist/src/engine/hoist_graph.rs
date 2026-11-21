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
use indexmap::{
    IndexMap,
    IndexSet,
};
use std::{
    collections::{
        HashMap,
        HashSet,
    },
    rc::Rc,
};

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
    on_path: HashSet<*const WorkNode>,
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
        on_path: HashSet::new(),
        next_wave: sorted_regular_dependencies(ctx.root)
            .into_iter()
            .map(RcByPtr)
            .collect(),
        outcome: PassOutcome::default(),
    };
    let aliased_root_path: Vec<String> = root_path
        .iter()
        .map(|node| node.aliased_locator())
        .collect();
    while !pass.next_wave.is_empty() {
        let wave = std::mem::take(&mut pass.next_wave);
        for dep in wave {
            if dep.ident == ctx.root.ident || dep.is_hoist_border.get() {
                continue;
            }
            let decoupled = decouple(ctx.root, &dep.0, ctx.arena);
            hoist_node_dependencies(&mut pass, &[], &aliased_root_path, &decoupled);
        }
    }
    pass.outcome
}

/// Upstream's `hoistNodeDependencies`: move what can move from `parent`
/// to the root, then descend into the children that stayed.
fn hoist_node_dependencies(
    pass: &mut Pass<'_>,
    node_path: &[Rc<WorkNode>],
    aliased_path: &[String],
    parent: &Rc<WorkNode>,
) {
    if pass.on_path.contains(&Rc::as_ptr(parent)) {
        return;
    }
    let mut next_aliased_path = aliased_path.to_vec();
    next_aliased_path.push(parent.aliased_locator());
    let mut full_path: Vec<Rc<WorkNode>> = vec![Rc::clone(pass.ctx.root)];
    full_path.extend(node_path.iter().map(Rc::clone));
    full_path.push(Rc::clone(parent));
    let unhoistable = decide(pass, &full_path, parent);
    move_hoistable(pass, parent, &unhoistable);
    let mut next_path = node_path.to_vec();
    next_path.push(Rc::clone(parent));
    for child in sorted_regular_dependencies(parent) {
        let stays = unhoistable.contains(&RcByPtr(Rc::clone(&child)));
        if !stays
            || child.is_hoist_border.get()
            || next_aliased_path.contains(&child.aliased_locator())
        {
            continue;
        }
        pass.on_path.insert(Rc::as_ptr(parent));
        let decoupled = decouple(parent, &child, pass.ctx.arena);
        hoist_node_dependencies(pass, &next_path, &next_aliased_path, &decoupled);
        pass.on_path.remove(&Rc::as_ptr(parent));
    }
}

/// The hoist decision for every regular child of `parent`. Returns the
/// children that stay, in decision order, after spreading a refusal to
/// every sibling that waited on the refused node as a peer provider.
fn decide(
    pass: &Pass<'_>,
    full_path: &[Rc<WorkNode>],
    parent: &WorkNode,
) -> IndexSet<RcByPtr<WorkNode>> {
    let mut decisions: IndexMap<RcByPtr<WorkNode>, bool> = IndexMap::new();
    let mut dependants: HashMap<String, Vec<String>> = HashMap::new();
    for child in sorted_regular_dependencies(parent) {
        let info = node_hoist_info(pass.ctx, full_path, &child);
        if let Hoistable::Depends(providers) = &info {
            for provider in providers {
                dependants
                    .entry(provider.name.clone())
                    .or_default()
                    .push(child.name.clone());
            }
        }
        decisions.insert(RcByPtr(child), !matches!(info, Hoistable::No));
    }
    let mut unhoistable: IndexSet<RcByPtr<WorkNode>> = IndexSet::new();
    let refused: Vec<RcByPtr<WorkNode>> = decisions
        .iter()
        .filter(|(_, hoistable)| !**hoistable)
        .map(|(node, _)| node.clone())
        .collect();
    for node in refused {
        refuse(parent, &node, &dependants, &mut unhoistable);
    }
    unhoistable
}

/// Upstream's `addUnhoistableNode`: a refused node takes down every
/// sibling that waited for it.
fn refuse(
    parent: &WorkNode,
    node: &RcByPtr<WorkNode>,
    dependants: &HashMap<String, Vec<String>>,
    unhoistable: &mut IndexSet<RcByPtr<WorkNode>>,
) {
    let mut pending = vec![node.clone()];
    while let Some(node) = pending.pop() {
        if !unhoistable.insert(node.clone()) {
            continue;
        }
        for dependant in dependants
            .get(&node.name)
            .into_iter()
            .flatten()
        {
            if let Some(waiting) = parent.dependency(dependant) {
                pending.push(RcByPtr(waiting));
            }
        }
    }
}

/// Move every child of `parent` that may leave it to the root, or merge
/// it into the copy the root already holds.
fn move_hoistable(
    pass: &mut Pass<'_>,
    parent: &Rc<WorkNode>,
    unhoistable: &IndexSet<RcByPtr<WorkNode>>,
) {
    let mut moved = false;
    for child in sorted_regular_dependencies(parent) {
        if unhoistable.contains(&RcByPtr(Rc::clone(&child))) {
            continue;
        }
        moved = true;
        pass.outcome.is_graph_changed = true;
        let shadowed = pass.parent_shadowed.get(&RcByPtr(Rc::clone(parent)));
        if shadowed.is_some_and(|names| names.contains(&child.name)) {
            pass.outcome.another_round_needed = true;
        }
        parent.dependencies.borrow_mut().shift_remove(&child.name);
        parent.hoisted_dependencies
            .borrow_mut()
            .insert(child.name.clone(), Rc::clone(&child));
        place_at_root(pass, &child);
    }
    if moved && parent.dependency_kind == HoisterDependencyKind::ExternalSoftLink {
        pass.outcome.another_round_needed = true;
    }
}

fn place_at_root(pass: &mut Pass<'_>, child: &Rc<WorkNode>) {
    let root = pass.ctx.root;
    if let Some(existing) = root.dependency(&child.name) {
        let references: Vec<String> = child.references
            .borrow()
            .iter()
            .cloned()
            .collect();
        existing.references.borrow_mut().extend(references);
    } else if root.ident != child.ident {
        root.dependencies
            .borrow_mut()
            .insert(child.name.clone(), Rc::clone(child));
        pass.next_wave.insert(RcByPtr(Rc::clone(child)));
    }
}
