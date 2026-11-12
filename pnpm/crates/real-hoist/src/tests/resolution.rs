//! A port of upstream's `selfCheck`: every dependency edge of the input
//! tree must still resolve, by the Node.js directory walk over the
//! hoisted layout, to the locator the input names.
//!
//! Inputs that share a locator merge into one hoisted node. A name they
//! resolve to different locators has no single right answer, so it is
//! not checked.

use super::{HoisterResult, HoisterTree, Rc};
use std::collections::{HashMap, HashSet};

/// Declared dependency locators by node locator, then by name. `None`
/// marks a name that inputs sharing the locator resolve differently.
type Declared = HashMap<String, HashMap<String, Option<String>>>;

/// Panics listing every edge the hoisted layout resolves to a locator
/// other than the one `tree` declares, or does not resolve at all.
pub(crate) fn assert_resolves_as_declared(tree: &HoisterTree, result: &HoisterResult) {
    let mut declared = Declared::new();
    collect_declared(tree, &mut declared, &mut HashSet::new());
    let mut errors = Vec::new();
    check_node(&Rc::new(result.clone()), &mut Vec::new(), &declared, &mut errors);
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
    let edges = declared
        .entry(tree_locator(node))
        .or_default();
    for dep in node.dependencies.borrow().iter() {
        let target = tree_locator(&dep.0);
        edges
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
    declared: &Declared,
    errors: &mut Vec<String>,
) {
    if ancestors.iter().any(|ancestor| Rc::ptr_eq(ancestor, node)) {
        return;
    }
    let edges = declared
        .get(&result_locator(node))
        .into_iter()
        .flatten();
    for (name, expected) in edges {
        let Some(expected) = expected else { continue };
        let resolved = resolve(node, ancestors, name).map(|found| result_locator(&found));
        if resolved.as_ref() != Some(expected) {
            let path: Vec<&str> = ancestors
                .iter()
                .map(|ancestor| ancestor.name.as_str())
                .chain([node.name.as_str()])
                .collect();
            errors.push(format!(
                "  {} requires {name}: declared {expected}, resolved {resolved:?}",
                path.join(" > "),
            ));
        }
    }
    ancestors.push(Rc::clone(node));
    let children: Vec<Rc<HoisterResult>> = node.dependencies
        .borrow()
        .iter()
        .map(|dep| Rc::clone(&dep.0))
        .collect();
    for child in &children {
        check_node(child, ancestors, declared, errors);
    }
    ancestors.pop();
}

/// The node `name` resolves to from `node`: its own `node_modules`, then
/// each ancestor's, nearest first.
fn resolve(
    node: &Rc<HoisterResult>,
    ancestors: &[Rc<HoisterResult>],
    name: &str,
) -> Option<Rc<HoisterResult>> {
    std::iter::once(node)
        .chain(ancestors.iter().rev())
        .find_map(|dir| {
            dir.dependencies
                .borrow()
                .iter()
                .find(|dep| dep.0.name == name)
                .map(|dep| Rc::clone(&dep.0))
        })
}

fn tree_locator(node: &HoisterTree) -> String {
    format!("{}@{}", node.ident_name, node.reference)
}

fn result_locator(node: &HoisterResult) -> String {
    let references = node.references.borrow();
    format!(
        "{}@{}",
        node.ident_name,
        references
            .iter()
            .next()
            .map_or("", String::as_str),
    )
}
