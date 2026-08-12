//! Unit tests for the walk's cache and child-realization decisions.

use super::{ConsultedPkgs, CycleTruncationKey, PeersCacheItem, should_retain_materialized_node};
use crate::{
    node_id::NodeId,
    resolve_peers::{
        context::SharedChain,
        test_support::{package, tree_node, walker_for_tests},
        walker::NodeOutput,
    },
    resolved_tree::ResolvedTree,
};
use pacquet_deps_path::DepPath;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{collections::BTreeMap, sync::Arc};

#[test]
fn materialized_nodes_referenced_by_peer_outputs_are_retained() {
    let referenced = NodeId::next();
    let unreferenced = NodeId::next();
    let output = NodeOutput {
        dep_path: DepPath::from("consumer@1.0.0"),
        external_resolved_peers: Arc::new(HashMap::from_iter([(
            "peer".into(),
            referenced.clone(),
        )])),
        auto_install_resolved_peers: HashMap::default(),
        missing_peers: Arc::new(HashMap::default()),
        subtree_missing_by_pkg: None,
    };

    assert!(should_retain_materialized_node(&HashSet::default(), Some(&output), &referenced));
    assert!(!should_retain_materialized_node(&HashSet::default(), Some(&output), &unreferenced,));
    assert!(should_retain_materialized_node(
        &HashSet::from_iter([unreferenced.clone()]),
        None,
        &unreferenced,
    ));
}

#[test]
fn previously_resolved_children_prefers_closest_same_package_ancestor() {
    let far_parent = NodeId::next();
    let close_parent = NodeId::next();
    let far_child = NodeId::leaf("shared@1.0.0");
    let close_child = NodeId::leaf("shared@2.0.0");

    let mut far_children = BTreeMap::new();
    far_children.insert("shared".to_string(), far_child);
    let mut close_children = BTreeMap::new();
    close_children.insert("shared".to_string(), close_child.clone());

    let mut tree = ResolvedTree {
        direct: Vec::new(),
        packages: HashMap::from_iter([("loop@1.0.0".into(), package("loop", "1.0.0", &[], false))]),
        dependencies_tree: HashMap::from_iter([
            (far_parent.clone(), tree_node("loop@1.0.0", far_children, 0)),
            (close_parent.clone(), tree_node("loop@1.0.0", close_children, 2)),
        ]),
        all_peer_dep_names: HashSet::default(),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    };
    let mut walker = walker_for_tests(&mut tree);

    let parent_node_ids = SharedChain::default().pushed(far_parent).pushed(close_parent);
    let parent_pkg_ids = SharedChain::default().pushed("loop@1.0.0".to_string());
    let children =
        walker.previously_resolved_children(&parent_node_ids, &parent_pkg_ids, "loop@1.0.0");

    assert_eq!(children.get("shared"), Some(&close_child));
}

fn keyed_item(
    walker: &mut crate::resolve_peers::walker::Walker<'_>,
    chain: &[&str],
) -> PeersCacheItem {
    // Consulted set = exactly the ids in `chain`, registered through the
    // walker's bit index the way a real walk would.
    let mut bits = ConsultedPkgs::default();
    for id in chain {
        let next = walker.consulted_bit_by_pkg.len() as u32;
        let bit = *walker.consulted_bit_by_pkg.entry(Arc::from(*id)).or_insert(next);
        bits.set(bit);
    }
    PeersCacheItem {
        dep_path: DepPath::from("cyclic@1.0.0"),
        resolved_peers: Arc::new(HashMap::default()),
        missing_peers: Arc::new(HashMap::default()),
        missing_peers_of_children: Arc::new(HashMap::default()),
        subtree_missing_by_pkg: None,
        cycle_key: Some(CycleTruncationKey::new(
            Arc::new(bits),
            chain.iter().map(|id| Arc::from(*id)).collect(),
            0,
        )),
    }
}

fn ancestors(base: &[&str], appended: &[&str]) -> crate::resolved_tree::AncestorIds {
    appended.iter().fold(
        crate::resolved_tree::AncestorIds::from(Arc::new(
            base.iter().map(ToString::to_string).collect::<Vec<_>>(),
        )),
        |ids, id| ids.pushed((*id).to_string()),
    )
}

/// The heart of the cycle-verdict cache: a truncated verdict is handed
/// back exactly to the occurrences whose ancestors truncate the subtree
/// the same way — same consulted ids, same order — and to no others.
#[test]
fn a_cycle_verdict_is_scoped_to_ancestors_that_truncate_identically() {
    let mut tree = ResolvedTree::default();
    let mut walker = walker_for_tests(&mut tree);
    let item = keyed_item(&mut walker, &["cyclic@1.0.0", "dep@1.0.0"]);
    walker.peers_cache.insert("cyclic@1.0.0".to_string(), vec![item]);
    let refs = HashMap::default();

    let hit = |walker: &crate::resolve_peers::walker::Walker<'_>,
               ids: &crate::resolved_tree::AncestorIds| {
        walker.find_hit(&refs, "cyclic@1.0.0", Some(ids)).is_some()
    };

    // Unconsulted ancestors are noise: chains differing only in them
    // truncate identically.
    assert!(
        hit(&walker, &ancestors(&[], &["root@1.0.0", "cyclic@1.0.0", "dep@1.0.0"])),
        "a chain equal on every consulted id reuses the verdict",
    );
    assert!(
        hit(&walker, &ancestors(&[], &["other@2.0.0", "cyclic@1.0.0", "dep@1.0.0"])),
        "swapping an unconsulted ancestor changes nothing the gate saw",
    );

    // Everything the gate weighed is load-bearing.
    assert!(
        !hit(&walker, &ancestors(&[], &["root@1.0.0"])),
        "an occurrence whose ancestors do not truncate the subtree must re-walk — \
         reusing the truncated verdict here is exactly pnpm/pnpm#5108",
    );
    assert!(
        !hit(&walker, &ancestors(&[], &["root@1.0.0", "dep@1.0.0", "cyclic@1.0.0"])),
        "the gate is order-sensitive, so the key is too",
    );
    assert!(
        !hit(&walker, &ancestors(&[], &["root@1.0.0", "cyclic@1.0.0", "dep@1.0.0", "dep@1.0.0"]),),
        "an extra occurrence of a consulted id is a different truncation",
    );
    assert!(
        !hit(&walker, &ancestors(&["cyclic@1.0.0", "dep@1.0.0"], &[])),
        "the same ids on the other side of the base/appended boundary answer \
         `forms_cycle` differently, so they do not compare equal",
    );
    assert!(
        walker.find_hit(&refs, "cyclic@1.0.0", None).is_none(),
        "without ancestor ids there is nothing to validate the truncation against",
    );
}

#[test]
fn consulted_pkgs_equality_ignores_capacity_history() {
    let mut low_first = ConsultedPkgs::default();
    let mut high_first = ConsultedPkgs::default();
    low_first.set(3);
    low_first.set(200);
    high_first.set(200);
    high_first.set(3);
    assert_eq!(low_first, high_first, "insertion order does not change the set");
    assert!(low_first.contains(3) && low_first.contains(200) && !low_first.contains(64));

    let mut merged = ConsultedPkgs::default();
    merged.set(3);
    let mut other = ConsultedPkgs::default();
    other.set(200);
    merged.or(&other);
    assert_eq!(merged, low_first, "or() reaches the same set as direct insertion");
}
