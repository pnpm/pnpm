//! Whether one dependency may move to the hoisting root: upstream's
//! `getNodeHoistInfo` and the helpers it relies on.

use super::{
    NodeInput,
    WorkNode,
    graph::is_workspace,
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
use std::{
    cell::RefCell,
    rc::Rc,
};

/// Names a hoist would shadow below each parent, upstream's
/// `shadowedNodes`. A later pass that hoists one of them past that
/// parent needs another round.
///
/// Keyed by the parent's address: the run's arena keeps every work node
/// alive, so an address names one node for the whole run.
pub(super) type ShadowedNodes = FxHashMap<*const WorkNode, FxHashSet<Rc<str>>>;

/// Upstream's `Hoistable`, with `dependsOn` folded into the variant.
pub(super) enum Hoistable {
    Yes,
    No,
    /// Hoistable once these peer providers of the parent hoist too.
    Depends(Vec<Rc<WorkNode>>),
}

/// What [`node_hoist_info`] reads about the current hoisting root.
pub(super) struct RootContext<'ctx> {
    pub(super) root: &'ctx Rc<WorkNode>,
    pub(super) used: &'ctx FxHashMap<Rc<str>, Rc<WorkNode>>,
    pub(super) hoist_idents: &'ctx FxHashMap<Rc<str>, Rc<str>>,
    pub(super) shadowed: &'ctx RefCell<ShadowedNodes>,
    pub(super) fast_lookup_possible: bool,
    pub(super) arena: &'ctx super::Arena,
}

/// Upstream's `getNodeHoistInfo`. `node_path` runs from the root to the
/// parent of `node`.
pub(super) fn node_hoist_info(
    ctx: &RootContext<'_>,
    node_path: &[Rc<WorkNode>],
    node: &Rc<WorkNode>,
) -> Hoistable {
    let mut hoistable = may_leave_parent(ctx, node_path, node)
        && is_name_available(ctx, node_path, node)
        && is_preferred(ctx, node);
    let mut depends_on = Some(Vec::new());
    if hoistable {
        depends_on = peer_providers_to_hoist(ctx, node_path, node);
        hoistable = depends_on.is_some();
    }
    if hoistable && !ctx.fast_lookup_possible {
        hoistable = hoisted_dependencies_still_resolve(ctx, node);
    }
    // Upstream reports `DEPENDS` even when the check above failed.
    match depends_on {
        Some(providers) if !providers.is_empty() => Hoistable::Depends(providers),
        _ if hoistable => Hoistable::Yes,
        _ => Hoistable::No,
    }
}

/// The first gates: no self-reference, no workspace, no soft link with
/// dependencies left behind, no name the root takes as a peer, and not the
/// root's own package under another name.
///
/// Upstream drops a node it hoists onto a root of the same ident, as the
/// root serves it. Only a root installed under the node's name does: a
/// `webpack` under a root installed as `webpack-5` would no longer be
/// found by its name.
fn may_leave_parent(ctx: &RootContext<'_>, node_path: &[Rc<WorkNode>], node: &WorkNode) -> bool {
    let root = &ctx.root.input;
    let is_root_under_another_name =
        root.ident == node.input.ident && root.name != node.input.name;
    if is_root_under_another_name {
        return false;
    }
    let is_self_reference = node_path
        .last()
        .is_some_and(|parent| parent.input.ident == node.input.ident);
    let leaves_dependencies_behind = node.input.dependency_kind
        == HoisterDependencyKind::ExternalSoftLink
        && has_unhoisted_dependencies(node);
    !is_self_reference
        && !is_workspace(node)
        && !leaves_dependencies_behind
        && !root.peer_names.contains(&node.input.name)
}

/// No different version under this name is used from above the root, or
/// sits on the path between the root and the parent. A path conflict is
/// recorded in [`RootContext::shadowed`].
fn is_name_available(ctx: &RootContext<'_>, node_path: &[Rc<WorkNode>], node: &WorkNode) -> bool {
    let NodeInput { name, ident, .. } = &*node.input;
    if ctx.used
        .get(name)
        .is_some_and(|used| used.input.ident != *ident)
    {
        return false;
    }
    let below_root = node_path.get(1..).unwrap_or_default();
    let filled = below_root
        .iter()
        .rev()
        .any(|ancestor| {
            ancestor.dependencies
                .borrow()
                .get(name)
                .is_some_and(|dep| dep.input.ident != *ident)
        });
    if filled && let Some(parent) = node_path.last() {
        let mut shadowed = ctx.shadowed.borrow_mut();
        shadowed
            .entry(Rc::as_ptr(parent))
            .or_default()
            .insert(Rc::clone(name));
    }
    !filled
}

fn is_preferred(ctx: &RootContext<'_>, node: &WorkNode) -> bool {
    ctx.hoist_idents
        .get(&node.input.name)
        .is_some_and(|ident| *ident == node.input.ident)
}

/// Upstream's peer check. Each peer is looked up at the nearest ancestor
/// that does not take it as a peer itself. If that ancestor holds a
/// provider the root does not, the node cannot move unless the ancestor
/// is its parent, whose provider may move with it: the providers to wait
/// for are returned. `None` means a peer pins the node in place.
///
/// Unlike upstream, an ancestor that only looks empty does not settle the
/// peer (see [`passes_through`]): its provider was hoisted higher, or it is
/// the provider. Upstream treats the peer as satisfied there, and the node
/// can then move to a root copy that resolves the peer to another version.
fn peer_providers_to_hoist(
    ctx: &RootContext<'_>,
    node_path: &[Rc<WorkNode>],
    node: &WorkNode,
) -> Option<Vec<Rc<WorkNode>>> {
    if node.input.peer_names.is_empty() {
        return Some(Vec::new());
    }
    let mut unchecked: IndexSet<Rc<str>> = node.input.peer_names.clone();
    let mut nearest_first = node_path
        .get(1..)
        .unwrap_or_default()
        .iter()
        .rev();
    let depends_on = nearest_first
        .next()
        .map(|parent| unhoisted_providers(ctx, parent, &mut unchecked))
        .unwrap_or_default();
    let is_pinned = nearest_first.any(|ancestor| {
        !unhoisted_providers(ctx, ancestor, &mut unchecked).is_empty()
    });
    (!is_pinned).then_some(depends_on)
}

/// The providers `ancestor` holds for the `unchecked` peer names and the
/// root does not. Each name the ancestor settles leaves `unchecked`.
fn unhoisted_providers(
    ctx: &RootContext<'_>,
    ancestor: &WorkNode,
    unchecked: &mut IndexSet<Rc<str>>,
) -> Vec<Rc<WorkNode>> {
    let settled: Vec<Rc<str>> = unchecked
        .iter()
        .filter(|name| !passes_through(ancestor, name))
        .cloned()
        .collect();
    let mut providers = Vec::new();
    for name in settled {
        unchecked.shift_remove(&name);
        let Some(provider) = ancestor.dependency(&name) else { continue };
        let root_dependency = ctx.root.dependency(&name);
        if !root_dependency.is_some_and(|at_root| Rc::ptr_eq(&at_root, &provider)) {
            providers.push(provider);
        }
    }
    providers
}

/// Whether `ancestor` leaves `name` to be provided above it: it takes the
/// name as a peer itself (upstream's rule), or it holds no `name` of its
/// own and either a pass hoisted its provider away or it is the provider.
/// A package that is its dependency's peer is found from below through
/// its parent's `node_modules`, where the next ancestor holds it.
fn passes_through(ancestor: &WorkNode, name: &str) -> bool {
    let takes_as_peer = ancestor.input.peer_names.contains(name)
        && ancestor.original_dependencies.borrow().contains_key(name);
    let found_higher = !ancestor.dependencies.borrow().contains_key(name)
        && (ancestor.hoisted_dependencies.borrow().contains_key(name)
            || &*ancestor.input.name == name);
    takes_as_peer || found_higher
}

/// After the first round: every dependency hoisted out of the node must
/// still resolve to the same ident from the root.
fn hoisted_dependencies_still_resolve(ctx: &RootContext<'_>, node: &WorkNode) -> bool {
    node.hoisted_dependencies
        .borrow()
        .values()
        .all(|hoisted| {
            let name = &hoisted.input.name;
            let available = ctx.used
                .get(name)
                .map(Rc::clone)
                .or_else(|| ctx.root.dependency(name));
            available.is_some_and(|dep| dep.input.ident == hoisted.input.ident)
        })
}

/// Upstream's `hasUnhoistedDependencies`.
fn has_unhoisted_dependencies(node: &WorkNode) -> bool {
    node.dependencies
        .borrow()
        .iter()
        .any(|(name, dep)| {
            !node.input.peer_names.contains(name)
                && dep.input.ident != node.input.ident
        })
}

/// Upstream's `getSortedRegularDependencies`: the children that are not
/// peer edges, each after the siblings that provide its peers.
///
/// Without a child that takes a peer there is nothing to reorder.
pub(super) fn sorted_regular_dependencies(node: &WorkNode) -> Vec<Rc<WorkNode>> {
    let regular = super::graph::regular_dependencies(node);
    if regular.iter().all(|dep| dep.input.peer_names.is_empty()) {
        return regular;
    }
    let mut sorted: IndexSet<RcByPtr<WorkNode>> = IndexSet::new();
    for dep in regular {
        add_after_peers(node, &dep, &mut sorted, &mut FxHashSet::default());
    }
    sorted
        .into_iter()
        .map(|dep| dep.0)
        .collect()
}

fn add_after_peers(
    node: &WorkNode,
    dep: &Rc<WorkNode>,
    sorted: &mut IndexSet<RcByPtr<WorkNode>>,
    seen: &mut FxHashSet<*const WorkNode>,
) {
    if !seen.insert(Rc::as_ptr(dep)) {
        return;
    }
    for peer_name in &dep.input.peer_names {
        if node.input.peer_names.contains(peer_name) {
            continue;
        }
        let Some(peer_dep) = node.dependency(peer_name) else { continue };
        if !sorted.contains(&RcByPtr(Rc::clone(&peer_dep))) {
            add_after_peers(node, &peer_dep, sorted, seen);
        }
    }
    sorted.insert(RcByPtr(Rc::clone(dep)));
}
