use super::{
    ChildStep, HashMap, HoistCtx, HoisterResult, Rc, RcByPtr, absorb_decision, apply_decision,
    decouple_child, same_locator,
};

/// Depth-first hoist driver. `ancestor_path` is the path from
/// `root` down to (but *excluding*) `node`, so for the root
/// itself it is empty and for a child of root it is `[root]`.
/// Returns whether this subtree moved at least one node in the
/// current round — the outer multi-round loop uses that to
/// decide whether another round can unlock further hoists.
///
/// `node` must be decoupled — the walk mutates its dependency set.
/// The recursion keeps that invariant: every child is decoupled
/// (relative to its post-decision parent) before it is descended
/// into, so a package shared by several parents is walked — and
/// decided — once per path, on that path's own copy. The walk tree
/// therefore has the shape of the final materialized layout, and
/// termination follows from the cycle cut below: no locator repeats
/// on a path, so paths (and the walk) are finite.
pub(super) fn hoist_subtree(
    node: &Rc<HoisterResult>,
    ancestor_path: &[Rc<HoisterResult>],
    ctx: &HoistCtx<'_>,
    root_index: &mut HashMap<String, RcByPtr<HoisterResult>>,
    under_border: bool,
) -> bool {
    let mut changed_in_subtree = false;

    // A node whose name is in `border_names` is a hoisting border:
    // its descendants are kept nested beneath it rather than hoisted
    // to the root. `under_border` carries that boundary down the
    // recursion — once any proper ancestor of a node is a border,
    // the node (and everything below it) stays put. Mirrors
    // upstream's `isHoistBorder` flag, which blocks a bordered
    // node's *children* from hoisting past it, not the bordered
    // node itself.
    let children_blocked = under_border || ctx.border_names.contains(&node.name);

    // Snapshot the current children so we can mutate
    // `node.dependencies` mid-iteration without invalidating the
    // borrow. `RcByPtr::clone` just bumps refcounts.
    let children: Vec<RcByPtr<HoisterResult>> = node.dependencies
        .borrow()
        .iter()
        .cloned()
        .collect();

    // Path from root down to and including `node` — i.e. the
    // ancestor path for `node`'s direct children. Used for the
    // peer-shadow checks, the cycle cut, and as the starting point
    // for the path passed into recursion when a child stays
    // nested.
    let mut path_for_children: Vec<Rc<HoisterResult>> = ancestor_path.to_vec();
    path_for_children.push(Rc::clone(node));

    for child in children {
        changed_in_subtree |= hoist_child(
            &child,
            node,
            &path_for_children,
            ctx,
            root_index,
            children_blocked,
        );
    }
    changed_in_subtree
}

/// Whether the edge to `child` closes a cycle (or is a self-reference): a
/// package with this alias *and* locator already materializes as an ancestor
/// of this position, so requiring the alias here resolves to that ancestor.
/// The edge carries no additional layout and would send the walkers into
/// unbounded recursion, so the caller cuts it.
///
/// The alias name must match too: an edge exposing the same package under a
/// *different* alias is the only `node_modules/<alias>` entry for that name
/// and stays, just like upstream, whose `aliasedLocatorPath` guard compares
/// `name@locator`. Upstream merely skips descending into cycle edges; pacquet
/// removes them outright because its layout walkers require the result to be
/// a DAG. The parent is decoupled, so the cut is per-path.
pub(super) fn is_cycle_edge(child: &Rc<HoisterResult>, path: &[Rc<HoisterResult>]) -> bool {
    path
        .iter()
        .any(|ancestor| ancestor.name == child.name && same_locator(ancestor, child))
}

pub(super) fn descend_hoist_child(
    child: &RcByPtr<HoisterResult>,
    child_recursion_path: &[Rc<HoisterResult>],
    ctx: &HoistCtx<'_>,
    root_index: &mut HashMap<String, RcByPtr<HoisterResult>>,
    children_blocked: bool,
) -> bool {
    // Decouple before descending: the recursion mutates the
    // child's dependency set, which must not leak into other
    // paths that share the child. The child's current parent is
    // the last element of its recursion path — root for a
    // just-moved (or root-direct) child, `node` otherwise.
    let parent =
        child_recursion_path.last().expect("the recursion path ends at the child's parent");
    let child = decouple_child(parent, child);
    if Rc::ptr_eq(parent, ctx.root) {
        root_index.insert(child.0.name.clone(), child.clone());
    }

    hoist_subtree(
        &child.0,
        child_recursion_path,
        ctx,
        root_index,
        children_blocked,
    )
}

pub(super) fn decide_hoist_child(
    child: &RcByPtr<HoisterResult>,
    node: &Rc<HoisterResult>,
    path: &[Rc<HoisterResult>],
    ctx: &HoistCtx<'_>,
    root_index: &mut HashMap<String, RcByPtr<HoisterResult>>,
    children_blocked: bool,
) -> ChildStep {
    let decision = absorb_decision(&child.0, path, ctx, root_index, children_blocked);
    apply_decision(decision, child, node, ctx, path, root_index)
}

pub(super) fn hoist_child(
    child: &RcByPtr<HoisterResult>,
    node: &Rc<HoisterResult>,
    path_for_children: &[Rc<HoisterResult>],
    ctx: &HoistCtx<'_>,
    root_index: &mut HashMap<String, RcByPtr<HoisterResult>>,
    children_blocked: bool,
) -> bool {
    let mut changed_in_subtree = false;
    if is_cycle_edge(&child.0, path_for_children) {
        node.dependencies.borrow_mut().shift_remove(child);
        return true;
    }

    let child_recursion_path = match decide_hoist_child(
        child,
        node,
        path_for_children,
        ctx,
        root_index,
        children_blocked,
    ) {
        ChildStep::Dropped => {
            return true;
        }
        ChildStep::Descend { path, moved } => {
            changed_in_subtree |= moved;
            path
        }
    };

    let child_changed = descend_hoist_child(
        child,
        &child_recursion_path,
        ctx,
        root_index,
        children_blocked,
    );
    changed_in_subtree |= child_changed;
    changed_in_subtree
}
