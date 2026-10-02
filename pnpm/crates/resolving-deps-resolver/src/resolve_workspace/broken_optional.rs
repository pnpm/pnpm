//! npm's rule for a dependency that fails to resolve inside an optional
//! subtree: the nearest optional dependency above it is left out, together
//! with everything below it. Applying it to the settled tree rather than
//! during the walk keeps the outcome the same whichever occurrence of a
//! shared package walked its children.

use crate::{
    NodeId, ResolveDependencyTreeError,
    resolve_peers::ImporterPeerInput,
    resolved_tree::{ChildEdge, ResolvedTree, TreeChildren},
};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::sync::Arc;

/// Drop every optional edge, and every optional direct dependency, that
/// leads to a broken package. A package is broken when the walk recorded it
/// in `broken` or one of its regular dependencies is broken.
///
/// `optional_direct` holds each importer's `optionalDependencies` names, in
/// the order of `importers`. A broken package that an importer reaches
/// through a regular dependency fails the install with the recorded error.
pub(super) fn drop_broken_optional_dependencies(
    tree: &mut ResolvedTree,
    importers: &mut [ImporterPeerInput],
    optional_direct: &[HashSet<String>],
    mut broken: HashMap<Arc<str>, ResolveDependencyTreeError>,
) -> Result<(), ResolveDependencyTreeError> {
    if broken.is_empty() {
        return Ok(());
    }
    let cause_by_id = propagate_through_regular_edges(&tree.children_by_id, &broken);
    for (importer, optional_names) in importers.iter_mut().zip(optional_direct) {
        if let Some(required) = importer.direct
            .iter()
            .find(|dep| cause_by_id.contains_key(&dep.id) && !optional_names.contains(&dep.alias))
        {
            let cause = &cause_by_id[&required.id];
            return Err(broken.remove(cause).expect("every cause is a recorded broken package"));
        }
        importer.direct.retain(|dep| !cause_by_id.contains_key(&dep.id));
    }
    drop_edges_to(tree, &cause_by_id);
    Ok(())
}

/// Every broken package, mapped to the recorded package whose failure
/// broke it.
fn propagate_through_regular_edges(
    children_by_id: &HashMap<Arc<str>, Arc<Vec<ChildEdge>>>,
    broken: &HashMap<Arc<str>, ResolveDependencyTreeError>,
) -> HashMap<Arc<str>, Arc<str>> {
    let mut regular_parents: HashMap<&str, Vec<&Arc<str>>> = HashMap::default();
    for (parent, edges) in children_by_id {
        for edge in edges.iter().filter(|edge| !edge.optional) {
            regular_parents
                .entry(&edge.pkg_id)
                .or_default()
                .push(parent);
        }
    }
    let mut cause_by_id: HashMap<Arc<str>, Arc<str>> = broken
        .keys()
        .map(|id| (Arc::clone(id), Arc::clone(id)))
        .collect();
    let mut pending: Vec<Arc<str>> = broken.keys().cloned().collect();
    while let Some(id) = pending.pop() {
        let cause = Arc::clone(&cause_by_id[&id]);
        for parent in regular_parents
            .get(&*id)
            .into_iter()
            .flatten()
        {
            if !cause_by_id.contains_key(*parent) {
                cause_by_id.insert(Arc::clone(parent), Arc::clone(&cause));
                pending.push(Arc::clone(parent));
            }
        }
    }
    cause_by_id
}

/// Remove the edges into `dropped` from the recorded children lists and
/// from every realized occurrence. Only optional edges can be left pointing
/// there: a regular edge makes its parent broken too.
fn drop_edges_to(tree: &mut ResolvedTree, dropped: &HashMap<Arc<str>, Arc<str>>) {
    for edges in tree.children_by_id.values_mut() {
        if edges
            .iter()
            .any(|edge| dropped.contains_key(&edge.pkg_id))
        {
            *edges = Arc::new(
                edges
                    .iter()
                    .filter(|edge| !dropped.contains_key(&edge.pkg_id))
                    .cloned()
                    .collect(),
            );
        }
    }
    let dropped_nodes: HashSet<NodeId> = tree.dependencies_tree
        .iter()
        .filter(|(_, node)| dropped.contains_key(&node.resolved_package_id))
        .map(|(node_id, _)| node_id.clone())
        .collect();
    for node in tree.dependencies_tree.values_mut() {
        if let TreeChildren::Realized(children) = &mut node.children
            && children
                .values()
                .any(|child| dropped_nodes.contains(child))
        {
            *children = Arc::new(
                children
                    .iter()
                    .filter(|(_, child)| !dropped_nodes.contains(*child))
                    .map(|(alias, child)| (alias.clone(), child.clone()))
                    .collect(),
            );
        }
    }
}
