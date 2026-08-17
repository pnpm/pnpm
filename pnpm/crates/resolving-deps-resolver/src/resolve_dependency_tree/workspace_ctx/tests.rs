use pnpm_resolving_resolver_base::ResolveOptions;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use super::{
    super::{lock_recoverable, test_support::manifest_result},
    WorkspaceTreeCtx,
};
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
                Arc::from("root@1.0.0".to_string()),
                TreeChildren::Realized(
                    BTreeMap::from([("child".to_string(), child.clone())]).into(),
                ),
                0,
                true,
            ),
        ),
        (
            child.clone(),
            DependenciesTreeNode::new(
                Arc::from("child@1.0.0".to_string()),
                TreeChildren::empty(),
                1,
                true,
            ),
        ),
        (
            unrelated.clone(),
            DependenciesTreeNode::new(
                Arc::from("unrelated@1.0.0".to_string()),
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
            Arc::from("root@1.0.0".to_string()),
            TreeChildren::Lazy { parent_ids: Arc::new(Vec::new()).into() },
            0,
            true,
        ),
    );
    for pkg_id in ["root@1.0.0", "lazy-child@1.0.0", "foreign@1.0.0"] {
        lock_recoverable(&workspace.packages)
            .insert(Arc::from(pkg_id.to_string()), snapshot_package(pkg_id));
    }
    lock_recoverable(&workspace.children_by_id).insert(
        Arc::from("root@1.0.0".to_string()),
        recorded(vec![ChildEdge {
            alias: "lazy-child".to_string(),
            pkg_id: Arc::from("lazy-child@1.0.0".to_string()),
            optional: false,
        }]),
    );
    lock_recoverable(&workspace.children_by_id)
        .insert(Arc::from("foreign@1.0.0".to_string()), recorded(Vec::new()));

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

    make_non_owner_nodes_lazy(&ctx, "pkg@1.0.0", &owner);
    assert_eq!(
        workspace.children_rewrites(),
        1,
        "an occurrence already reading the owner's children is not a rewrite",
    );
}

#[test]
fn owner_missing_record_is_written_once_per_generation() {
    use super::super::lock_recoverable;
    use super::{ChildrenOwner, ChildrenOwnerEntry, WorkspaceTreeCtx};
    use crate::resolve_peers::MissingNames;
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
        .insert(Arc::from("pkg@1.0.0".to_string()), entry(owner.clone()));

    let names = |names: &[&str]| -> HashSet<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    };
    fn miss(names: &HashSet<String>) -> HashMap<&str, MissingNames<'_>> {
        HashMap::from_iter([("pkg@1.0.0", MissingNames::One(names))])
    }

    let one_peer = names(&["peer"]);
    let two_peers = names(&["peer", "other-peer"]);
    let none = names(&[]);

    ctx.record_first_walk_missing("pkg-a", &miss(&one_peer));
    assert_eq!(ctx.first_walk_missing_by_pkg().get("pkg@1.0.0"), Some(&one_peer));

    ctx.record_first_walk_missing(".", &miss(&two_peers));
    let recorded = ctx.first_walk_missing_by_pkg();
    assert_eq!(recorded.get("pkg@1.0.0").map(HashSet::len), Some(2));

    ctx.record_first_walk_missing(".", &miss(&none));
    let recorded = ctx.first_walk_missing_by_pkg();
    assert!(
        recorded.get("pkg@1.0.0").is_some_and(|names| names.contains("peer")),
        "the owner's post-hoist pass must not refresh the generation's record",
    );

    let new_owner = ChildrenOwner { depth: 0, ..owner };
    lock_recoverable(&ctx.children_owner_by_id)
        .insert(Arc::from("pkg@1.0.0".to_string()), entry(new_owner));
    ctx.record_first_walk_missing(".", &miss(&none));
    assert_eq!(
        ctx.first_walk_missing_by_pkg().get("pkg@1.0.0").map(HashSet::len),
        Some(0),
        "a new ownership generation records afresh",
    );
}

#[test]
fn owner_scope_snapshots_are_shared_until_a_write_changes_the_map() {
    use super::super::lock_recoverable;
    use super::{ChildrenOwner, ChildrenOwnerEntry, WorkspaceTreeCtx};
    use crate::resolve_peers::MissingNames;
    use std::sync::Arc;

    let ctx = WorkspaceTreeCtx::default();
    let owner = ChildrenOwner {
        update_active: false,
        depth: 0,
        importer_order: 0,
        parent_path: Vec::new(),
        importer_id: ".".to_string(),
    };
    lock_recoverable(&ctx.children_owner_by_id).insert(
        Arc::from("pkg@1.0.0".to_string()),
        ChildrenOwnerEntry { owner, peer_shadowed: Arc::new(HashSet::default()) },
    );

    let peers: HashSet<String> = HashSet::from_iter(["peer".to_string()]);
    let missing = HashMap::from_iter([("pkg@1.0.0", MissingNames::One(&peers))]);

    let before = ctx.first_walk_missing_by_pkg();
    ctx.record_first_walk_missing(".", &missing);
    let after_write = ctx.first_walk_missing_by_pkg();
    assert!(!Arc::ptr_eq(&before, &after_write), "a write must invalidate the shared snapshot");
    assert!(before.is_empty(), "an issued snapshot keeps what it was built from");
    assert_eq!(after_write.get("pkg@1.0.0"), Some(&peers));

    ctx.record_first_walk_missing(".", &missing);
    assert!(
        Arc::ptr_eq(&after_write, &ctx.first_walk_missing_by_pkg()),
        "a re-record that changes nothing must reuse the snapshot",
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
        id: Arc::from(pkg_id.to_string()),
        result: std::sync::Arc::new(manifest_result(serde_json::json!({}))),
        peer_dependencies: BTreeMap::new(),
        optional: false,
        is_leaf: false,
    }
}

#[test]
fn run_preferred_versions_cover_only_packages_reachable_from_recorded_roots() {
    let workspace = WorkspaceTreeCtx::default();
    for (name, version) in [("root", "1.0.0"), ("child", "2.0.0"), ("stray", "9.9.9")] {
        insert_named_package(&workspace, name, version);
    }
    insert_child_edge(&workspace, "root@1.0.0", "child", "child@2.0.0");
    workspace.record_preferred_version_roots(std::iter::once("root@1.0.0"));
    workspace.bump_revision();

    let cache = workspace.run_preferred_versions();
    assert_eq!(bucket_versions(&cache.versions, "root"), ["1.0.0"]);
    assert_eq!(bucket_versions(&cache.versions, "child"), ["2.0.0"]);
    assert!(
        !cache.versions.contains_key("stray"),
        "a resolved but unreachable package must not become a pick candidate",
    );
}

#[test]
fn run_preferred_versions_grow_with_new_roots_and_rebuild_on_children_rewrites() {
    let workspace = WorkspaceTreeCtx::default();
    for (name, version) in [("root", "1.0.0"), ("child", "2.0.0"), ("late", "3.0.0")] {
        insert_named_package(&workspace, name, version);
    }
    insert_child_edge(&workspace, "root@1.0.0", "child", "child@2.0.0");
    workspace.record_preferred_version_roots(std::iter::once("root@1.0.0"));
    workspace.bump_revision();
    assert!(workspace.run_preferred_versions().versions.contains_key("child"));

    workspace.record_preferred_version_roots(std::iter::once("late@3.0.0"));
    workspace.bump_revision();
    assert_eq!(bucket_versions(&workspace.run_preferred_versions().versions, "late"), ["3.0.0"]);

    // A children-ownership rewrite can drop edges, so the closure is
    // rebuilt rather than grown.
    lock_recoverable(&workspace.children_by_id)
        .insert(Arc::from("root@1.0.0".to_string()), recorded(vec![]));
    workspace.record_children_rewrite();
    workspace.bump_revision();
    let cache = workspace.run_preferred_versions();
    assert!(!cache.versions.contains_key("child"), "the rewritten-away child must drop out");
    assert_eq!(bucket_versions(&cache.versions, "root"), ["1.0.0"]);
    assert_eq!(bucket_versions(&cache.versions, "late"), ["3.0.0"]);
}

#[test]
fn run_preferred_versions_fold_workspace_manifest_identities_once_reachable() {
    let workspace = WorkspaceTreeCtx::default();
    lock_recoverable(&workspace.packages)
        .insert(Arc::from("link:packages/opt".to_string()), snapshot_package("link:packages/opt"));
    workspace.record_workspace_manifest_identity("link:packages/opt", "opt", "1.0.0");
    insert_named_package(&workspace, "root", "1.0.0");
    workspace.record_preferred_version_roots(std::iter::once("root@1.0.0"));
    workspace.bump_revision();
    assert!(
        !workspace.run_preferred_versions().versions.contains_key("opt"),
        "an unreachable workspace project's version must not become a pick candidate",
    );

    insert_child_edge(&workspace, "root@1.0.0", "opt", "link:packages/opt");
    workspace.record_children_rewrite();
    workspace.bump_revision();
    assert_eq!(bucket_versions(&workspace.run_preferred_versions().versions, "opt"), ["1.0.0"]);
}

#[test]
fn run_preferred_versions_pick_up_an_identity_recorded_after_the_first_visit() {
    let workspace = WorkspaceTreeCtx::default();
    insert_named_package(&workspace, "root", "1.0.0");
    lock_recoverable(&workspace.packages)
        .insert(Arc::from("link:packages/opt".to_string()), snapshot_package("link:packages/opt"));
    insert_child_edge(&workspace, "root@1.0.0", "opt", "link:packages/opt");
    workspace.record_preferred_version_roots(std::iter::once("root@1.0.0"));
    workspace.bump_revision();
    assert!(!workspace.run_preferred_versions().versions.contains_key("opt"));

    workspace.record_workspace_manifest_identity("link:packages/opt", "opt", "1.0.0");
    workspace.bump_revision();
    assert_eq!(bucket_versions(&workspace.run_preferred_versions().versions, "opt"), ["1.0.0"]);
}

fn insert_named_package(workspace: &WorkspaceTreeCtx, name: &str, version: &str) {
    let name_ver = pnpm_lockfile::PkgNameVer::new(
        pnpm_lockfile::PkgName::parse(name).expect("parse package name"),
        version.parse::<node_semver::Version>().expect("parse package version"),
    );
    let mut result = manifest_result(serde_json::json!({}));
    result.id = (&name_ver).into();
    result.name_ver = Some(name_ver);
    let pkg_id = format!("{name}@{version}");
    let package = super::ResolvedPackage {
        id: Arc::from(pkg_id.clone()),
        result: std::sync::Arc::new(result),
        peer_dependencies: BTreeMap::new(),
        optional: false,
        is_leaf: false,
    };
    lock_recoverable(&workspace.packages).insert(Arc::from(pkg_id), package);
}

fn insert_child_edge(workspace: &WorkspaceTreeCtx, parent_id: &str, alias: &str, child_id: &str) {
    lock_recoverable(&workspace.children_by_id).insert(
        Arc::from(parent_id.to_string()),
        recorded(vec![crate::resolved_tree::ChildEdge {
            alias: alias.to_string(),
            pkg_id: Arc::from(child_id.to_string()),
            optional: false,
        }]),
    );
}

fn bucket_versions(
    versions: &pnpm_resolving_resolver_base::PreferredVersions,
    name: &str,
) -> Vec<String> {
    versions.get(name).map(|bucket| bucket.keys().cloned().collect()).unwrap_or_default()
}

/// The discovery engine keeps one view across every hoist round, so a
/// view carried through several waves of writes has to end up where a
/// view built after the last wave would.
#[test]
fn a_view_synced_wave_by_wave_matches_one_built_from_scratch() {
    let workspace = WorkspaceTreeCtx::default();
    let root = NodeId::next();
    let child = NodeId::next();
    let mut carried = crate::resolved_tree::ResolvedTree::default();
    let mut carried_cursor = super::SyncCursor::default();

    record_package(&workspace, "root@1.0.0", &["peer-a"]);
    record_tree_node(&workspace, &root, "root@1.0.0", 0);
    assert!(workspace.sync_discovery_tree(&mut carried, &mut carried_cursor));

    record_package(&workspace, "child@1.0.0", &["peer-b"]);
    record_tree_node(&workspace, &child, "child@1.0.0", 3);
    // A shallower revisit of a node the first wave already recorded.
    record_tree_node(&workspace, &root, "root@1.0.0", -1);
    assert!(workspace.sync_discovery_tree(&mut carried, &mut carried_cursor));

    let mut from_scratch = crate::resolved_tree::ResolvedTree::default();
    workspace.rebuild_discovery_tree(&mut from_scratch, &mut super::SyncCursor::default());

    let depths = |tree: &crate::resolved_tree::ResolvedTree| -> BTreeMap<String, i32> {
        tree.dependencies_tree
            .iter()
            .map(|(node_id, node)| (node_id.to_string(), node.depth))
            .collect()
    };
    dbg!(depths(&carried), depths(&from_scratch));
    assert_eq!(depths(&carried), depths(&from_scratch));
    assert_eq!(depths(&carried), BTreeMap::from([(root.to_string(), -1), (child.to_string(), 3)]));
    assert_eq!(
        carried.packages.keys().collect::<BTreeSet<_>>(),
        from_scratch.packages.keys().collect::<BTreeSet<_>>(),
    );
    assert_eq!(
        carried.all_peer_dep_names.iter().collect::<BTreeSet<_>>(),
        from_scratch.all_peer_dep_names.iter().collect::<BTreeSet<_>>(),
    );
}

/// A children-owner handover can re-split a package into children and
/// peers. The walk verdicts a carried view already produced were derived
/// from the old split, so the sync has to report the view unmergeable
/// rather than fold the new one in.
#[test]
fn a_changed_peer_dependency_split_makes_the_view_unmergeable() {
    let workspace = WorkspaceTreeCtx::default();
    let mut carried = crate::resolved_tree::ResolvedTree::default();
    let mut cursor = super::SyncCursor::default();

    record_package(&workspace, "pkg@1.0.0", &["peer-a"]);
    assert!(workspace.sync_discovery_tree(&mut carried, &mut cursor));

    record_package(&workspace, "pkg@1.0.0", &["peer-a", "peer-b"]);
    assert!(!workspace.sync_discovery_tree(&mut carried, &mut cursor));
}

fn record_package(workspace: &WorkspaceTreeCtx, pkg_id: &str, peer_names: &[&str]) {
    use super::super::lock_recoverable;
    let peer_dependencies = peer_names
        .iter()
        .map(|name| {
            (
                (*name).to_string(),
                crate::resolved_tree::PeerDep { version: "*".to_string(), optional: false },
            )
        })
        .collect();
    lock_recoverable(&workspace.packages).insert(
        Arc::from(pkg_id.to_string()),
        super::ResolvedPackage { peer_dependencies, ..snapshot_package(pkg_id) },
    );
    let mut all_peers = lock_recoverable(&workspace.all_peer_dep_names);
    for name in peer_names {
        if all_peers.insert((*name).to_string()) {
            workspace.record_peer_dep_name(name);
        }
    }
    drop(all_peers);
    workspace.record_package_write(pkg_id);
}

fn record_tree_node(workspace: &WorkspaceTreeCtx, node_id: &NodeId, pkg_id: &str, depth: i32) {
    use super::super::lock_recoverable;
    let mut tree = lock_recoverable(&workspace.dependencies_tree);
    match tree.entry(node_id.clone()) {
        std::collections::hash_map::Entry::Occupied(mut entry) => {
            if entry.get().depth > depth {
                entry.get_mut().depth = depth;
            }
        }
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(DependenciesTreeNode::new(
                Arc::from(pkg_id.to_string()),
                TreeChildren::empty(),
                depth,
                true,
            ));
        }
    }
    drop(tree);
    workspace.record_tree_node_write(node_id);
}

/// Children recorded by a walk whose context these tests do not vary.
fn recorded(edges: Vec<crate::resolved_tree::ChildEdge>) -> super::RecordedChildren {
    super::RecordedChildren {
        edges: Arc::new(edges),
        context: super::RecordedChildrenContext {
            peer_shadowed: Arc::default(),
            prior_key: None,
            update_active: false,
        },
    }
}

/// Reuse is offered against what the recording walk resolved under, so
/// each field of the recorded context has to be able to withhold it.
#[test]
fn recorded_children_match_only_under_the_recording_context() {
    use super::super::{UpdateReuseScope, tree_ctx::TreeCtx};
    use super::{
        ChildrenRecording, RecordedChildrenContext, claim_children_owner, record_children,
        recorded_children_match,
    };
    use std::sync::Arc;

    let workspace = Arc::new(WorkspaceTreeCtx::default());
    let ctx = TreeCtx::with_workspace(Arc::clone(&workspace), ResolveOptions::default());
    let claim = claim_children_owner(&ctx, "pkg@1.0.0", 1, &[], HashSet::default());
    let context = || RecordedChildrenContext {
        peer_shadowed: Arc::default(),
        prior_key: None,
        update_active: false,
    };
    assert!(!recorded_children_match(&ctx, "pkg@1.0.0", &context()), "nothing recorded yet");
    assert_ne!(
        record_children(&ctx, "pkg@1.0.0", &claim.owner, Vec::new(), context()),
        ChildrenRecording::Declined,
    );

    assert!(recorded_children_match(&ctx, "pkg@1.0.0", &context()));
    let shadowed = RecordedChildrenContext {
        peer_shadowed: Arc::new(HashSet::from_iter(["peer".to_string()])),
        ..context()
    };
    assert!(!recorded_children_match(&ctx, "pkg@1.0.0", &shadowed), "a different shadow set");
    let updating = RecordedChildrenContext { update_active: true, ..context() };
    assert!(!recorded_children_match(&ctx, "pkg@1.0.0", &updating), "a different update policy");
    let pinned = RecordedChildrenContext {
        prior_key: Some("pkg@1.0.0".parse().expect("parse snapshot key")),
        ..context()
    };
    assert!(!recorded_children_match(&ctx, "pkg@1.0.0", &pinned), "a different snapshot key");
    assert!(matches!(ctx.update_reuse_scope(), UpdateReuseScope::All));
}

/// Which of a package's dependencies its own peers supply is a
/// property of the occurrence, and pinned children were filtered under
/// the recording occurrence's set — so the pins stand in for a fresh
/// walk only where the two shadow the same names.
#[test]
fn a_pin_stands_in_only_for_a_walk_that_shadows_the_same_dependencies() {
    use super::RecordedChildrenContext;
    use std::sync::Arc;

    let fresh = RecordedChildrenContext {
        peer_shadowed: Arc::default(),
        prior_key: None,
        update_active: false,
    };
    let pinned = RecordedChildrenContext {
        prior_key: Some("pkg@1.0.0".parse().expect("parse snapshot key")),
        ..fresh.clone()
    };
    assert!(pinned.pins_children_over(&fresh));

    let shadowed = RecordedChildrenContext {
        peer_shadowed: Arc::new(HashSet::from_iter(["peer".to_string()])),
        ..pinned.clone()
    };
    assert!(!shadowed.pins_children_over(&fresh), "a different shadow set filtered them");
    assert!(shadowed.pins_children_over(&RecordedChildrenContext {
        peer_shadowed: Arc::clone(&shadowed.peer_shadowed),
        ..fresh.clone()
    }));

    let updating = RecordedChildrenContext { update_active: true, ..fresh };
    assert!(!pinned.pins_children_over(&updating), "an update re-resolves pins on purpose");
}

/// A walk that lost the claim while it ran must not overwrite the
/// children the occurrence that outranked it published.
#[test]
fn children_are_published_only_by_the_standing_owner() {
    use super::super::tree_ctx::TreeCtx;
    use super::{
        ChildrenRecording, RecordedChildrenContext, claim_children_owner, record_children,
    };
    use std::sync::Arc;

    let workspace = Arc::new(WorkspaceTreeCtx::default());
    let ctx = TreeCtx::with_workspace(Arc::clone(&workspace), ResolveOptions::default());
    let context = || RecordedChildrenContext {
        peer_shadowed: Arc::default(),
        prior_key: None,
        update_active: false,
    };
    let deep = claim_children_owner(&ctx, "pkg@1.0.0", 5, &[], HashSet::default());
    let shallow = claim_children_owner(&ctx, "pkg@1.0.0", 0, &[], HashSet::default());
    assert!(deep.owns_children && shallow.owns_children, "each claim won when it was taken");

    assert_eq!(
        record_children(&ctx, "pkg@1.0.0", &deep.owner, Vec::new(), context()),
        ChildrenRecording::Declined,
        "the deeper walk lost the claim before it published",
    );
    assert_ne!(
        record_children(&ctx, "pkg@1.0.0", &shallow.owner, Vec::new(), context()),
        ChildrenRecording::Declined,
    );
}

/// A later owner re-walks whenever its resolution context differs from
/// the recorded one, which the claim's own `peer_shadowed` comparison
/// cannot see. Only edges that actually moved stale the realized
/// children every other occurrence node still holds — and never the
/// ones the prior lockfile pinned, whose whole point is to survive a
/// fresh walk's different answer.
#[test]
fn re_recording_reports_whether_the_child_edges_moved() {
    use super::super::tree_ctx::TreeCtx;
    use super::{
        ChildrenRecording, RecordedChildrenContext, claim_children_owner, record_children,
    };
    use std::sync::Arc;

    let workspace = Arc::new(WorkspaceTreeCtx::default());
    let ctx = TreeCtx::with_workspace(Arc::clone(&workspace), ResolveOptions::default());
    let context = || RecordedChildrenContext {
        peer_shadowed: Arc::default(),
        prior_key: None,
        update_active: false,
    };
    let edges = |pkg_id: &str| {
        vec![crate::resolved_tree::ChildEdge {
            alias: "dep".to_string(),
            pkg_id: Arc::from(pkg_id.to_string()),
            optional: false,
        }]
    };
    let owner = claim_children_owner(&ctx, "pkg@1.0.0", 0, &[], HashSet::default());

    assert_eq!(
        record_children(&ctx, "pkg@1.0.0", &owner.owner, edges("dep@1.0.0"), context()),
        ChildrenRecording::Published,
        "no occurrence node can hold children of a package nothing recorded yet",
    );
    assert_eq!(
        record_children(&ctx, "pkg@1.0.0", &owner.owner, edges("dep@1.0.0"), context()),
        ChildrenRecording::Published,
    );
    assert_eq!(
        record_children(&ctx, "pkg@1.0.0", &owner.owner, edges("dep@2.0.0"), context()),
        ChildrenRecording::PublishedOverStale,
    );

    let pinned = RecordedChildrenContext {
        prior_key: Some("pkg@1.0.0".parse().expect("parse snapshot key")),
        ..context()
    };
    assert_eq!(
        record_children(&ctx, "pkg@1.0.0", &owner.owner, edges("dep@1.0.0"), pinned),
        ChildrenRecording::PublishedOverStale,
    );
    assert_eq!(
        record_children(&ctx, "pkg@1.0.0", &owner.owner, edges("dep@1.0.0"), context()),
        ChildrenRecording::Declined,
        "a fresh walk that agrees does not republish the pin away either",
    );
    assert_eq!(
        record_children(&ctx, "pkg@1.0.0", &owner.owner, edges("dep@2.0.0"), context()),
        ChildrenRecording::Declined,
        "a fresh walk does not unpin the subtree the lockfile-reusing occurrences realized",
    );
    let standing = lock_recoverable(&workspace.children_by_id);
    let standing: Vec<&str> = standing
        .get("pkg@1.0.0")
        .expect("pinned children")
        .edges
        .iter()
        .map(|edge| &*edge.pkg_id)
        .collect();
    assert_eq!(standing, ["dep@1.0.0"], "the pinned children are what every occurrence reads");
}
