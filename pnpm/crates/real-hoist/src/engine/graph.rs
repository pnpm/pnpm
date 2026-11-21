//! Building the work graph from the input tree and the result tree from
//! the work graph.

use super::{
    Arena,
    EngineOptions,
    WorkNode,
    ident_reference,
};
use crate::{
    HoisterDependencyKind,
    HoisterResult,
    HoisterTree,
    RcByPtr,
};
use indexmap::{
    IndexMap,
    IndexSet,
};
use std::{
    cell::{
        Cell,
        RefCell,
    },
    collections::{
        HashMap,
        HashSet,
    },
    rc::Rc,
};

pub(crate) fn tree_ident(tree: &HoisterTree) -> String {
    let identity = tree.identity.clone().unwrap_or_else(|| ident_reference(&tree.reference));
    format!("{}@{identity}", tree.ident_name)
}

/// Peer names by ident, the union over every input node with that ident.
///
/// Upstream takes a package's peer names from its manifest, so every
/// instance of one package has the same set, and its algorithm relies on
/// that: a copy that lacks a name its variants take as a peer could have a
/// different version hoisted into it, which a variant deduplicated into
/// the copy would then resolve. A lockfile gives every peer variant the
/// same declared and transitive peer names, so this only fills gaps in a
/// hand-built input.
pub(super) type PeerNamesByIdent = HashMap<String, IndexSet<String>>;

pub(super) fn peer_names_by_ident(tree: &HoisterTree) -> PeerNamesByIdent {
    let mut by_ident = PeerNamesByIdent::new();
    let mut seen: HashSet<*const HoisterTree> = HashSet::new();
    let mut pending: Vec<Rc<HoisterTree>> = tree.dependencies
        .borrow()
        .iter()
        .map(|child| Rc::clone(&child.0))
        .collect();
    while let Some(node) = pending.pop() {
        if !seen.insert(Rc::as_ptr(&node)) {
            continue;
        }
        by_ident
            .entry(tree_ident(&node))
            .or_default()
            .extend(node.peer_names.iter().cloned());
        pending.extend(
            node.dependencies
                .borrow()
                .iter()
                .map(|child| Rc::clone(&child.0)),
        );
    }
    by_ident
}

/// The node's peer names: those of its ident, except a name it holds as a
/// regular dependency of its own.
fn peer_names_of(tree: &HoisterTree, by_ident: &PeerNamesByIdent) -> IndexSet<String> {
    let children = tree.dependencies.borrow();
    let own_regular = |name: &String| {
        !tree.peer_names.contains(name)
            && children
                .iter()
                .any(|child| child.0.name == *name)
    };
    let mut names = tree.peer_names.clone();
    let shared = by_ident
        .get(&tree_ident(tree))
        .into_iter()
        .flatten();
    names.extend(
        shared
            .filter(|name| !own_regular(name))
            .cloned(),
    );
    names
}

fn work_node(
    tree: &HoisterTree,
    is_hoist_border: bool,
    (by_ident, arena): (&PeerNamesByIdent, &Arena),
) -> Rc<WorkNode> {
    let reference = tree.reference.clone();
    let node = Rc::new(WorkNode {
        name: tree.name.clone(),
        ident_name: tree.ident_name.clone(),
        ident: tree_ident(tree).into(),
        references: RefCell::new(IndexSet::from([reference])),
        dependencies: RefCell::default(),
        original_dependencies: RefCell::default(),
        hoisted_dependencies: RefCell::default(),
        peer_names: Rc::new(peer_names_of(tree, by_ident)),
        decoupled: Cell::new(true),
        is_hoist_border: Cell::new(is_hoist_border),
        hoist_priority: tree.hoist_priority,
        dependency_kind: tree.dependency_kind,
    });
    arena.track(&node);
    node
}

/// A pending `addNode(node, parentNode)` call of upstream's `cloneTree`:
/// the input node's children, the work node they attach to, and how far
/// the loop over them has got.
struct CloneFrame {
    children: Vec<Rc<HoisterTree>>,
    parent: Rc<WorkNode>,
    next: usize,
}

/// Upstream's `cloneTree`. The root is a workspace and a hoist border,
/// as upstream makes it, whatever the input says.
pub(super) fn clone_tree(tree: &HoisterTree, options: &EngineOptions<'_>) -> Rc<WorkNode> {
    let by_ident = peer_names_by_ident(tree);
    let root = work_node(tree, true, (&by_ident, &options.nodes));
    let mut seen: HashMap<*const HoisterTree, Rc<WorkNode>> =
        HashMap::from([(std::ptr::from_ref(tree), Rc::clone(&root))]);
    let mut stack = vec![clone_frame(tree, &root)];
    while let Some(frame) = stack.last_mut() {
        let Some(child) = frame.children.get(frame.next).map(Rc::clone) else {
            stack.pop();
            continue;
        };
        frame.next += 1;
        let parent = Rc::clone(&frame.parent);
        if let Some(added) = add_node(&child, &parent, &mut seen, (options, &by_ident)) {
            stack.push(clone_frame(&child, &added));
        }
    }
    root
}

fn clone_frame(tree: &HoisterTree, parent: &Rc<WorkNode>) -> CloneFrame {
    let children = tree.dependencies
        .borrow()
        .iter()
        .map(|child| Rc::clone(&child.0))
        .collect();
    CloneFrame { children, parent: Rc::clone(parent), next: 0 }
}

/// The body of upstream's `addNode`. Returns the new work node when its
/// children still have to be added.
fn add_node(
    tree: &Rc<HoisterTree>,
    parent: &Rc<WorkNode>,
    seen: &mut HashMap<*const HoisterTree, Rc<WorkNode>>,
    (options, by_ident): (&EngineOptions<'_>, &PeerNamesByIdent),
) -> Option<Rc<WorkNode>> {
    let key = Rc::as_ptr(tree);
    let existing = seen.get(&key).map(Rc::clone);
    let is_seen = existing.is_some();
    let node = existing.unwrap_or_else(|| work_node(tree, false, (by_ident, &options.nodes)));
    seen.entry(key).or_insert_with(|| Rc::clone(&node));
    let parent_limits = options.hoisting_limits.get(parent.ident.as_ref());
    if parent_limits.is_some_and(|names| names.contains(&node.name)) {
        node.is_hoist_border.set(true);
    }
    parent.dependencies
        .borrow_mut()
        .insert(node.name.clone(), Rc::clone(&node));
    parent.original_dependencies
        .borrow_mut()
        .insert(node.name.clone(), Rc::clone(&node));
    if is_seen {
        mark_coupled(&node);
        return None;
    }
    Some(node)
}

/// Upstream's `markNodeCoupled`: a node reached through a second parent,
/// and everything below it, may no longer be mutated in place.
fn mark_coupled(node: &Rc<WorkNode>) {
    let mut seen: HashSet<*const WorkNode> = HashSet::new();
    let mut pending = vec![Rc::clone(node)];
    while let Some(node) = pending.pop() {
        if !seen.insert(Rc::as_ptr(&node)) {
            continue;
        }
        node.decoupled.set(false);
        pending.extend(regular_dependencies(&node));
    }
}

/// The children that are not peer edges.
pub(super) fn regular_dependencies(node: &WorkNode) -> Vec<Rc<WorkNode>> {
    node.dependencies
        .borrow()
        .values()
        .filter(|dep| !node.peer_names.contains(&dep.name))
        .map(Rc::clone)
        .collect()
}

/// Upstream's `decoupleGraphNode`: a single-parent copy of `node` under
/// `parent`, so mutating it cannot leak into other paths.
pub(super) fn decouple(parent: &WorkNode, node: &Rc<WorkNode>, arena: &Arena) -> Rc<WorkNode> {
    if node.decoupled.get() {
        return Rc::clone(node);
    }
    let copy = |cell: &RefCell<IndexMap<String, Rc<WorkNode>>>| RefCell::new(cell.borrow().clone());
    let clone = Rc::new(WorkNode {
        name: node.name.clone(),
        ident_name: node.ident_name.clone(),
        references: RefCell::new(node.references.borrow().clone()),
        ident: Rc::clone(&node.ident),
        dependencies: copy(&node.dependencies),
        original_dependencies: copy(&node.original_dependencies),
        hoisted_dependencies: copy(&node.hoisted_dependencies),
        peer_names: Rc::clone(&node.peer_names),
        decoupled: Cell::new(true),
        is_hoist_border: Cell::new(node.is_hoist_border.get()),
        hoist_priority: node.hoist_priority,
        dependency_kind: node.dependency_kind,
    });
    arena.track(&clone);
    let self_dependency = clone.dependency(&clone.name);
    if self_dependency.is_some_and(|dep| dep.ident == clone.ident) {
        clone.dependencies
            .borrow_mut()
            .insert(clone.name.clone(), Rc::clone(&clone));
    }
    parent.dependencies
        .borrow_mut()
        .insert(clone.name.clone(), Rc::clone(&clone));
    clone
}

/// A pending `addNode(node, parentWorkNode, parentNode)` call of
/// upstream's `shrinkTree`.
struct ShrinkFrame {
    node: Rc<WorkNode>,
    result: Rc<HoisterResult>,
    children: Vec<Rc<WorkNode>>,
    next: usize,
}

/// Upstream's `shrinkTree`: the hoisted graph unfolded into a tree,
/// without peer edges. A node met again on its own path gets an empty
/// copy, as upstream gives it. A package that depends on itself gets no
/// edge to itself: upstream would add the parent to its own children,
/// which only makes the result cyclic.
pub(super) fn shrink_tree(root: &Rc<WorkNode>) -> HoisterResult {
    let top = result_node(root);
    let mut on_path: HashSet<*const WorkNode> = HashSet::from([Rc::as_ptr(root)]);
    let mut stack = vec![shrink_frame(root, &top)];
    while let Some(frame) = stack.last_mut() {
        let Some(child) = frame.children.get(frame.next).map(Rc::clone) else {
            if let Some(done) = stack.pop() {
                on_path.remove(&Rc::as_ptr(&done.node));
            }
            continue;
        };
        frame.next += 1;
        if Rc::ptr_eq(&child, &frame.node) {
            continue;
        }
        let result = Rc::new(result_node(&child));
        frame.result.dependencies
            .borrow_mut()
            .insert(RcByPtr(Rc::clone(&result)));
        if on_path.insert(Rc::as_ptr(&child)) {
            stack.push(shrink_frame(&child, &result));
        }
    }
    let top = stack_root(top);
    top
}

fn stack_root(top: Rc<HoisterResult>) -> HoisterResult {
    Rc::try_unwrap(top).unwrap_or_else(|shared| (*shared).clone())
}

fn shrink_frame(node: &Rc<WorkNode>, result: &Rc<HoisterResult>) -> ShrinkFrame {
    ShrinkFrame {
        node: Rc::clone(node),
        result: Rc::clone(result),
        children: regular_dependencies(node),
        next: 0,
    }
}

fn result_node(node: &WorkNode) -> Rc<HoisterResult> {
    Rc::new(HoisterResult {
        name: node.name.clone(),
        ident_name: node.ident_name.clone(),
        references: RefCell::new(node.references.borrow().clone()),
        peer_names: (*node.peer_names).clone(),
        dependencies: RefCell::default(),
    })
}

/// Empty every work node's child maps, breaking the reference cycles that
/// would otherwise keep the run's graph alive.
pub(super) fn tear_down(arena: &Arena) {
    for node in arena.0.take() {
        for cell in [&node.dependencies, &node.original_dependencies, &node.hoisted_dependencies] {
            cell.borrow_mut().clear();
        }
    }
}

/// Whether a node is a workspace project, which never hoists.
pub(super) fn is_workspace(node: &WorkNode) -> bool {
    node.dependency_kind == HoisterDependencyKind::Workspace
}
