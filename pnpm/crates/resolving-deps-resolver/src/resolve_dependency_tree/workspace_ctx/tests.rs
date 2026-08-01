use pacquet_resolving_resolver_base::ResolveOptions;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::collections::BTreeMap;

use super::{super::test_support::manifest_result, WorkspaceTreeCtx};
use crate::{
    DirectDep, NodeId,
    resolved_tree::{DependenciesTreeNode, TreeChildren},
};

#[test]
fn importer_snapshot_excludes_other_importers_occurrence_nodes() {
    let workspace = WorkspaceTreeCtx::default();
    let root = NodeId::next();
    let child = NodeId::next();
    let unrelated = NodeId::next();
    workspace.dependencies_tree.lock().unwrap().extend([
        (
            root.clone(),
            DependenciesTreeNode::new(
                "root@1.0.0".to_string(),
                TreeChildren::Realized(BTreeMap::from([("child".to_string(), child.clone())])),
                0,
                true,
            ),
        ),
        (
            child.clone(),
            DependenciesTreeNode::new("child@1.0.0".to_string(), TreeChildren::empty(), 1, true),
        ),
        (
            unrelated.clone(),
            DependenciesTreeNode::new(
                "unrelated@1.0.0".to_string(),
                TreeChildren::empty(),
                0,
                true,
            ),
        ),
    ]);

    let snapshot = workspace.snapshot_reachable_from(vec![DirectDep {
        alias: "root".to_string(),
        node_id: root.clone(),
        id: "root@1.0.0".to_string(),
    }]);

    assert_eq!(snapshot.dependencies_tree.len(), 2);
    assert!(snapshot.dependencies_tree.contains_key(&root));
    assert!(snapshot.dependencies_tree.contains_key(&child));
    assert!(!snapshot.dependencies_tree.contains_key(&unrelated));
}

#[test]
fn importer_snapshot_follows_lazy_edges_for_the_package_closure() {
    use super::super::lock_recoverable;
    use crate::resolved_tree::ChildEdge;
    use std::sync::Arc;

    let workspace = WorkspaceTreeCtx::default();
    let root = NodeId::next();
    lock_recoverable(&workspace.dependencies_tree).insert(
        root.clone(),
        DependenciesTreeNode::new(
            "root@1.0.0".to_string(),
            TreeChildren::Lazy { parent_ids: Arc::new(Vec::new()).into() },
            0,
            true,
        ),
    );
    for pkg_id in ["root@1.0.0", "lazy-child@1.0.0", "foreign@1.0.0"] {
        lock_recoverable(&workspace.packages).insert(pkg_id.to_string(), snapshot_package(pkg_id));
    }
    lock_recoverable(&workspace.children_by_id).insert(
        "root@1.0.0".to_string(),
        Arc::new(vec![ChildEdge {
            alias: "lazy-child".to_string(),
            pkg_id: "lazy-child@1.0.0".to_string(),
            optional: false,
        }]),
    );
    lock_recoverable(&workspace.children_by_id)
        .insert("foreign@1.0.0".to_string(), Arc::new(Vec::new()));

    let snapshot = workspace.snapshot_reachable_from(vec![DirectDep {
        alias: "root".to_string(),
        node_id: root,
        id: "root@1.0.0".to_string(),
    }]);

    assert!(
        snapshot.packages.contains_key("lazy-child@1.0.0"),
        "a package reachable only through a lazy edge must stay in the snapshot",
    );
    assert!(snapshot.children_by_id.contains_key("root@1.0.0"));
    assert!(!snapshot.packages.contains_key("foreign@1.0.0"));
    assert!(!snapshot.children_by_id.contains_key("foreign@1.0.0"));
}

#[test]
fn ownership_rewrite_of_existing_nodes_bumps_children_rewrites() {
    use super::super::{lock_recoverable, tree_ctx::TreeCtx};
    use super::{insert_tree_node, make_non_owner_nodes_lazy};
    use std::sync::Arc;

    let workspace = Arc::new(WorkspaceTreeCtx::default());
    let ctx = TreeCtx::with_workspace(Arc::clone(&workspace), ResolveOptions::default());
    let owner = NodeId::next();
    let other = NodeId::next();
    insert_tree_node(&ctx, owner.clone(), "pkg@1.0.0", TreeChildren::empty(), 0);
    insert_tree_node(&ctx, other.clone(), "pkg@1.0.0", TreeChildren::empty(), 1);
    lock_recoverable(&workspace.node_parent_ids_by_id)
        .insert(other.clone(), Arc::new(vec!["parent@1.0.0".to_string()]));

    make_non_owner_nodes_lazy(&ctx, "absent@1.0.0", &owner);
    assert_eq!(workspace.children_rewrites(), 0, "no occurrence rewritten, nothing to invalidate");

    make_non_owner_nodes_lazy(&ctx, "pkg@1.0.0", &owner);
    assert_eq!(workspace.children_rewrites(), 1);
    assert!(
        matches!(
            lock_recoverable(&workspace.dependencies_tree).get(&other).unwrap().children,
            TreeChildren::Lazy { .. },
        ),
        "the non-owner occurrence flips to lazy",
    );
}

#[test]
fn owner_missing_record_is_written_once_per_generation() {
    use super::super::lock_recoverable;
    use super::{ChildrenOwner, ChildrenOwnerEntry, WorkspaceTreeCtx};
    use std::sync::Arc;

    let ctx = WorkspaceTreeCtx::default();
    let owner = ChildrenOwner {
        update_active: false,
        depth: 1,
        importer_order: 0,
        parent_path: vec!["root-dep@1.0.0".to_string()],
        importer_id: ".".to_string(),
    };
    let entry = |owner: ChildrenOwner| ChildrenOwnerEntry {
        owner,
        peer_shadowed: Arc::new(HashSet::default()),
    };
    lock_recoverable(&ctx.children_owner_by_id)
        .insert("pkg@1.0.0".to_string(), entry(owner.clone()));

    let miss = |names: &[&str]| {
        let mut map: HashMap<String, HashSet<String>> = HashMap::default();
        map.insert("pkg@1.0.0".to_string(), names.iter().map(|name| (*name).to_string()).collect());
        map
    };

    ctx.record_first_walk_missing("pkg-a", &miss(&["peer"]));
    assert_eq!(
        ctx.first_walk_missing_by_pkg().get("pkg@1.0.0"),
        Some(&miss(&["peer"]).remove("pkg@1.0.0").unwrap()),
    );

    ctx.record_first_walk_missing(".", &miss(&["peer", "other-peer"]));
    let recorded = ctx.first_walk_missing_by_pkg();
    assert_eq!(recorded.get("pkg@1.0.0").map(HashSet::len), Some(2));

    ctx.record_first_walk_missing(".", &miss(&[]));
    let recorded = ctx.first_walk_missing_by_pkg();
    assert!(
        recorded.get("pkg@1.0.0").is_some_and(|names| names.contains("peer")),
        "the owner's post-hoist pass must not refresh the generation's record",
    );

    let new_owner = ChildrenOwner { depth: 0, ..owner };
    lock_recoverable(&ctx.children_owner_by_id).insert("pkg@1.0.0".to_string(), entry(new_owner));
    ctx.record_first_walk_missing(".", &miss(&[]));
    assert_eq!(
        ctx.first_walk_missing_by_pkg().get("pkg@1.0.0").map(HashSet::len),
        Some(0),
        "a new ownership generation records afresh",
    );
}

#[test]
fn importer_scoped_update_owner_wins_before_discovery_order() {
    use super::ChildrenOwner;

    let ordinary = ChildrenOwner {
        update_active: false,
        depth: 0,
        importer_order: 0,
        parent_path: Vec::new(),
        importer_id: "unselected".to_string(),
    };
    let update_active = ChildrenOwner {
        update_active: true,
        depth: 10,
        importer_order: 10,
        parent_path: vec!["later".to_string()],
        importer_id: "selected".to_string(),
    };

    assert!(update_active.wins_over(&ordinary));
    assert!(!ordinary.wins_over(&update_active));
}

fn snapshot_package(pkg_id: &str) -> super::ResolvedPackage {
    super::ResolvedPackage {
        id: pkg_id.to_string(),
        result: std::sync::Arc::new(manifest_result(serde_json::json!({}))),
        peer_dependencies: BTreeMap::new(),
        optional: false,
        is_leaf: false,
    }
}
