//! Unit tests for reading a walk's subtree missing-peer summaries.

use super::{MissingSummary, index_missing_names};
use rustc_hash::FxHashSet as HashSet;
use std::sync::Arc;

fn summary(
    own: Option<(&str, &[&str])>,
    children: Vec<Arc<MissingSummary>>,
) -> Arc<MissingSummary> {
    Arc::new(MissingSummary {
        own: own.map(|(pkg_id, names)| {
            (
                pkg_id.to_string(),
                names
                    .iter()
                    .map(|name| (*name).to_string())
                    .collect(),
            )
        }),
        children,
    })
}

fn names_of<'a>(
    index: &'a rustc_hash::FxHashMap<&str, super::MissingNames<'_>>,
    pkg_id: &str,
) -> HashSet<&'a str> {
    index
        .get(pkg_id)
        .expect("package reported missing peers")
        .iter()
        .collect()
}

#[test]
fn descendants_of_every_root_are_indexed() {
    let shared = summary(Some(("deep@1.0.0", &["deep-peer"])), Vec::new());
    let roots = vec![
        summary(Some(("a@1.0.0", &["a-peer"])), vec![Arc::clone(&shared)]),
        summary(Some(("b@1.0.0", &["b-peer"])), vec![shared]),
    ];

    let index = index_missing_names(&roots);

    assert_eq!(index.len(), 3);
    assert_eq!(names_of(&index, "a@1.0.0"), HashSet::from_iter(["a-peer"]));
    assert_eq!(names_of(&index, "b@1.0.0"), HashSet::from_iter(["b-peer"]));
    assert_eq!(names_of(&index, "deep@1.0.0"), HashSet::from_iter(["deep-peer"]));
}

#[test]
fn occurrences_of_one_package_report_the_union_of_their_missing_peers() {
    let roots = vec![
        summary(Some(("pkg@1.0.0", &["first"])), Vec::new()),
        summary(Some(("pkg@1.0.0", &["second"])), Vec::new()),
        summary(Some(("pkg@1.0.0", &["third", "first"])), Vec::new()),
    ];

    let index = index_missing_names(&roots);

    assert_eq!(names_of(&index, "pkg@1.0.0"), HashSet::from_iter(["first", "second", "third"]));
}

#[test]
fn discovery_reuses_nested_providers_with_retained_bindings() {
    use super::{HashMap, NodeId, ParentRef, PeerWalkMode, WalkResult};
    use crate::{ResolvedTree, resolve_peers::test_support::walker_for_tests};
    use pnpm_deps_path::DepPath;

    let mut tree = ResolvedTree::default();
    let mut walker = walker_for_tests(&mut tree);
    walker.traversal.mode = PeerWalkMode::Discovery;
    let providers = [NodeId::next(), NodeId::next()];
    let ignored = NodeId::next();
    let inner = NodeId::leaf("inner@1.0.0");
    let bindings = Arc::new(HashMap::from_iter([("inner".into(), inner)]));
    let missing = Arc::new(HashMap::default());
    let path = DepPath::from("provider@1.0.0(inner@1.0.0)");
    for node_id in providers.iter().chain([&ignored]) {
        if node_id != &ignored {
            walker.caches.parent_pkgs_of_node.insert(node_id.clone(), Arc::new(HashMap::default()));
        }
        walker.remember_resolved_node(node_id, &path);
        walker.record_walk_result(
            node_id,
            &Arc::from("provider@1.0.0"),
            &WalkResult {
                dep_path: &path,
                all_resolved_peers: &bindings,
                all_missing_peers: &missing,
                missing_peers_of_children: &missing,
                subtree_missing_by_pkg: &None,
                is_pure: false,
            },
        );
    }
    assert_eq!(walker.nodes.external_peers.len(), 2);
    assert!(!walker.nodes.external_peers.contains_key(&ignored));
    let consumer_peers = Arc::new(HashMap::from_iter([("provider".into(), providers[0].clone())]));
    walker.record_walk_result(
        &NodeId::next(),
        &Arc::from("consumer@1.0.0"),
        &WalkResult {
            dep_path: &DepPath::from("consumer@1.0.0(provider@1.0.0(inner@1.0.0))"),
            all_resolved_peers: &consumer_peers,
            all_missing_peers: &missing,
            missing_peers_of_children: &missing,
            subtree_missing_by_pkg: &None,
            is_pure: false,
        },
    );
    let refs = HashMap::from_iter([(
        "provider".into(),
        ParentRef { version: "1.0.0".into(), node_id: Some(providers[1].clone()), alias: None },
    )]);
    assert!(walker.find_hit(&refs, "consumer@1.0.0").is_some());
}
