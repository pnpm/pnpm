//! A behavioural form of upstream's `selfCheck`. For every hoisted
//! directory and every input locator it serves, each dependency edge that
//! locator declares, peer edges included, must resolve by the Node.js
//! directory walk to a directory that behaves like the declared target.
//!
//! A directory behaves like a target when it serves the target's locator,
//! or when it holds the same package (the same ident) and each edge the
//! target declares resolves, from that directory, to a directory that
//! behaves like that edge's target. A deduplicated peer variant keeps no
//! subtree of its own, so its dependencies are served by the copies the
//! surviving node nests, and this is what has to hold of them. A cycle
//! counts as behaving alike.
//!
//! Upstream checks only the edges of the node that holds a directory, by
//! ident. Checking every served locator also proves that a peer variant
//! was only deduplicated into a copy whose position resolves its peers the
//! way it declared them.
//!
//! A served locator other than the directory's first is only checked when
//! its input node had peer names. Without them (`autoInstallPeers`, or a
//! lockfile without peer metadata) the hoister collapses peer variants
//! by design, and there is no declared binding for it to keep.
//!
//! Inputs that share a locator merge into one hoisted node. A name they
//! resolve to different locators has no single right answer, so it is not
//! checked.

use super::{
    HoisterResult,
    HoisterTree,
    Rc,
};
use std::collections::{
    HashMap,
    HashSet,
};

/// One input locator: its ident and its edges by name. `None` marks a
/// name that inputs sharing the locator resolve differently.
struct DeclaredNode {
    ident: String,
    has_peers: bool,
    edges: HashMap<String, Option<String>>,
}

type Declared = HashMap<String, DeclaredNode>;

/// A directory and the directories above it, nearest last.
struct Position {
    node: Rc<HoisterResult>,
    ancestors: Vec<Rc<HoisterResult>>,
}

struct Checker<'decl> {
    declared: &'decl Declared,
    /// `(directory, locator)` pairs already proven, or being proven up the
    /// current recursion.
    assumed: HashSet<(*const HoisterResult, String)>,
}

/// Panics listing every edge the hoisted layout resolves to a directory
/// that does not behave like the target `tree` declares.
pub(crate) fn assert_resolves_as_declared(tree: &HoisterTree, result: &HoisterResult) {
    let mut declared = Declared::new();
    collect_declared(tree, &mut declared, &mut HashSet::new());
    let mut checker = Checker { declared: &declared, assumed: HashSet::new() };
    let mut errors = Vec::new();
    check_node(&Rc::new(result.clone()), &mut Vec::new(), &mut checker, &mut errors);
    assert!(errors.is_empty(), "the hoisted layout breaks resolution:\n{}", errors.join("\n"));
}

fn collect_declared(
    node: &HoisterTree,
    declared: &mut Declared,
    seen: &mut HashSet<*const HoisterTree>,
) {
    if !seen.insert(node) {
        return;
    }
    let entry = declared
        .entry(tree_locator(node))
        .or_insert_with(|| DeclaredNode {
            ident: tree_ident(node),
            has_peers: !node.peer_names.is_empty(),
            edges: HashMap::new(),
        });
    for dep in node.dependencies.borrow().iter() {
        let target = tree_locator(&dep.0);
        entry.edges
            .entry(dep.0.name.clone())
            .and_modify(|existing| {
                if existing.as_ref() != Some(&target) {
                    *existing = None;
                }
            })
            .or_insert(Some(target));
    }
    for dep in node.dependencies.borrow().iter() {
        collect_declared(&dep.0, declared, seen);
    }
}

fn check_node(
    node: &Rc<HoisterResult>,
    ancestors: &mut Vec<Rc<HoisterResult>>,
    checker: &mut Checker<'_>,
    errors: &mut Vec<String>,
) {
    if ancestors.iter().any(|ancestor| Rc::ptr_eq(ancestor, node)) {
        return;
    }
    for (index, locator) in served_locators(node).into_iter().enumerate() {
        let declared = checker.declared.get(&locator);
        if index > 0 && !declared.is_some_and(|declared| declared.has_peers) {
            continue;
        }
        let edges = declared.into_iter().flat_map(DeclaredNode::checked_edges);
        for (name, expected) in edges {
            let error = checker.check_edge(node, ancestors, (&locator, &name, &expected));
            errors.extend(error);
        }
    }
    ancestors.push(Rc::clone(node));
    let children: Vec<Rc<HoisterResult>> = node.dependencies
        .borrow()
        .iter()
        .map(|dep| Rc::clone(&dep.0))
        .collect();
    for child in &children {
        check_node(child, ancestors, checker, errors);
    }
    ancestors.pop();
}

impl DeclaredNode {
    /// The edges with a single declared target, as `(name, target)`.
    fn checked_edges(&self) -> Vec<(String, String)> {
        self.edges
            .iter()
            .filter_map(|(name, target)| Some((name.clone(), target.clone()?)))
            .collect()
    }
}

impl Checker<'_> {
    /// The error for the edge `name` of `locator`, served by `node`, when it
    /// does not resolve to a directory that behaves like `expected`.
    fn check_edge(
        &mut self,
        node: &Rc<HoisterResult>,
        ancestors: &[Rc<HoisterResult>],
        (locator, name, expected): (&str, &str, &str),
    ) -> Option<String> {
        let found = resolve(node, ancestors, name);
        if found
            .as_ref()
            .is_some_and(|found| self.behaves_like(found, expected))
        {
            return None;
        }
        let path: Vec<&str> = ancestors
            .iter()
            .map(|ancestor| ancestor.name.as_str())
            .chain([node.name.as_str()])
            .collect();
        let resolved = found.map(|found| served_locators(&found.node));
        Some(format!(
            "  {} ({locator}) requires {name}: declared {expected}, resolved {resolved:?}",
            path.join(" > "),
        ))
    }

    fn behaves_like(&mut self, found: &Position, expected: &str) -> bool {
        let served = served_locators(&found.node);
        if served
            .iter()
            .any(|locator| locator == expected)
        {
            return true;
        }
        let Some(target) = self.declared.get(expected) else { return false };
        let same_package = served
            .iter()
            .any(|locator| {
                self.declared
                    .get(locator)
                    .is_some_and(|node| node.ident == target.ident)
            });
        if !same_package {
            return false;
        }
        if !self.assumed.insert((Rc::as_ptr(&found.node), expected.to_string())) {
            return true;
        }
        let mut ancestors = found.ancestors.clone();
        ancestors.push(Rc::clone(&found.node));
        target
            .checked_edges()
            .into_iter()
            .all(|(name, expected)| {
                resolve_from(&ancestors, &name)
                    .is_some_and(|next| self.behaves_like(&next, &expected))
            })
    }
}

/// The directory `name` resolves to from `node`: its own `node_modules`,
/// then each ancestor's, nearest first.
fn resolve(
    node: &Rc<HoisterResult>,
    ancestors: &[Rc<HoisterResult>],
    name: &str,
) -> Option<Position> {
    let mut path = ancestors.to_vec();
    path.push(Rc::clone(node));
    resolve_from(&path, name)
}

/// [`resolve`] from the last directory of `path`.
fn resolve_from(path: &[Rc<HoisterResult>], name: &str) -> Option<Position> {
    (0..path.len())
        .rev()
        .find_map(|index| {
            let dir = &path[index];
            let child = dir.dependencies
                .borrow()
                .iter()
                .find(|dep| dep.0.name == name)
                .map(|dep| Rc::clone(&dep.0))?;
            Some(Position { node: child, ancestors: path[..=index].to_vec() })
        })
}

fn served_locators(node: &HoisterResult) -> Vec<String> {
    node.references
        .borrow()
        .iter()
        .map(|reference| format!("{}@{reference}", node.ident_name))
        .collect()
}

fn tree_locator(node: &HoisterTree) -> String {
    format!("{}@{}", node.ident_name, node.reference)
}

fn tree_ident(node: &HoisterTree) -> String {
    crate::engine::graph::tree_ident(node)
}
