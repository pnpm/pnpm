//! Which version of each name the root prefers: upstream's
//! `buildPreferenceMap` and `getHoistIdentMap`.

use super::WorkNode;
use indexmap::IndexMap;
use std::{
    collections::{
        HashMap,
        HashSet,
        VecDeque,
    },
    rc::Rc,
};

/// Candidate idents per name, most preferred first.
pub(super) type HoistIdentMap = IndexMap<String, VecDeque<Rc<str>>>;

#[derive(Default)]
struct PreferenceEntry {
    dependents: HashSet<Rc<str>>,
    peer_dependents: HashSet<Rc<str>>,
    hoist_priority: u32,
}

impl PreferenceEntry {
    fn usages(&self) -> usize {
        self.dependents.len() + self.peer_dependents.len()
    }
}

/// Keyed by `(name, ident)`, in the order upstream's map inserts them.
type PreferenceMap = IndexMap<(String, Rc<str>), PreferenceEntry>;

fn entry<'map>(map: &'map mut PreferenceMap, node: &WorkNode) -> &'map mut PreferenceEntry {
    map.entry((node.name.clone(), Rc::clone(&node.ident)))
        .or_default()
}

/// A pending `addDependent(dependent, node)` call whose loop over
/// `node`'s children has reached `next`.
struct Frame {
    node: Rc<WorkNode>,
    children: Vec<Rc<WorkNode>>,
    next: usize,
}

/// Upstream's `buildPreferenceMap`.
fn build_preference_map(root: &Rc<WorkNode>) -> PreferenceMap {
    let mut map = PreferenceMap::new();
    let mut seen: HashSet<*const WorkNode> = HashSet::from([Rc::as_ptr(root)]);
    for dep in super::graph::regular_dependencies(root) {
        if let Some(frame) = add_dependent(&mut map, &mut seen, root, &dep) {
            walk_dependents(&mut map, &mut seen, frame);
        }
    }
    map
}

/// The part of `addDependent` before its loop. Returns the frame for the
/// loop when `node` is seen for the first time.
fn add_dependent(
    map: &mut PreferenceMap,
    seen: &mut HashSet<*const WorkNode>,
    dependent: &WorkNode,
    node: &Rc<WorkNode>,
) -> Option<Frame> {
    let is_new = seen.insert(Rc::as_ptr(node));
    entry(map, node).dependents.insert(Rc::clone(&dependent.ident));
    is_new.then(|| Frame { node: Rc::clone(node), children: node.dependency_list(), next: 0 })
}

/// `addDependent`'s loop, run on an explicit stack in recursion order.
fn walk_dependents(map: &mut PreferenceMap, seen: &mut HashSet<*const WorkNode>, first: Frame) {
    let mut stack = vec![first];
    while let Some(frame) = stack.last_mut() {
        let Some(dep) = frame.children.get(frame.next).map(Rc::clone) else {
            stack.pop();
            continue;
        };
        frame.next += 1;
        let node = Rc::clone(&frame.node);
        let dep_entry = entry(map, &dep);
        dep_entry.hoist_priority = dep_entry.hoist_priority.max(dep.hoist_priority);
        if node.peer_names.contains(&dep.name) {
            dep_entry.peer_dependents.insert(Rc::clone(&node.ident));
        } else if let Some(next) = add_dependent(map, seen, &node, &dep) {
            stack.push(next);
        }
    }
}

/// Upstream's `getHoistIdentMap`: for each name, the idents that may
/// claim the root slot, most preferred first. A name the root already
/// holds keeps only that ident at the front.
pub(super) fn hoist_ident_map(root: &Rc<WorkNode>) -> HoistIdentMap {
    let preferences = build_preference_map(root);
    let mut idents = HoistIdentMap::new();
    idents.insert(root.name.clone(), VecDeque::from([Rc::clone(&root.ident)]));
    for dep in super::graph::regular_dependencies(root) {
        idents.insert(dep.name.clone(), VecDeque::from([Rc::clone(&dep.ident)]));
    }
    let mut ranked: Vec<(&(String, Rc<str>), &PreferenceEntry)> = preferences.iter().collect();
    // A stable sort, as JavaScript's `Array.prototype.sort` is.
    ranked.sort_by(|(_, left), (_, right)| {
        right.hoist_priority
            .cmp(&left.hoist_priority)
            .then_with(|| right.usages().cmp(&left.usages()))
    });
    for ((name, ident), _) in ranked {
        if root.peer_names.contains(name) {
            continue;
        }
        let candidates = idents.entry(name.clone()).or_default();
        if !candidates.contains(ident) {
            candidates.push_back(Rc::clone(ident));
        }
    }
    idents
}

/// The front ident of each name, upstream's `hoistIdents`.
pub(super) fn front_idents(map: &HoistIdentMap) -> HashMap<String, Rc<str>> {
    map.iter()
        .filter_map(|(name, idents)| {
            idents
                .front()
                .map(|ident| (name.clone(), Rc::clone(ident)))
        })
        .collect()
}

/// The names whose preferred ident did not reach the root: each drops
/// it and promotes the next. Returns whether any did, upstream's
/// `wasStateChanged`.
pub(super) fn shift_unplaced(
    map: &mut HoistIdentMap,
    root: &WorkNode,
    front: &mut HashMap<String, Rc<str>>,
) -> bool {
    let mut shifted = false;
    for (name, idents) in map.iter_mut() {
        if idents.len() > 1 && !root.dependencies.borrow().contains_key(name) {
            idents.pop_front();
            if let Some(next) = idents.front() {
                front.insert(name.clone(), Rc::clone(next));
            }
            shifted = true;
        }
    }
    shifted
}

#[cfg(test)]
mod tests;
