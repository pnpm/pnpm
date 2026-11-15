use std::{path::Path, sync::Arc};

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use super::{
    ImporterPeerInput, ResolvePeersOptions, WorkspacePeerSettings, resolve_peers_workspace,
};
use crate::{
    resolve_peers::{
        test_support::{add_lazy_direct_dep, child_edge, package, walker_for_tests},
        walker::RootWalk,
    },
    resolved_tree::ResolvedTree,
};

fn deep_chain_tree(len: usize) -> ResolvedTree {
    let mut packages = HashMap::default();
    let mut children_by_id = HashMap::default();
    for idx in 0..len {
        let name = format!("chain-{idx}");
        let pkg_id = format!("{name}@1.0.0");
        let is_leaf = idx + 1 == len;
        packages.insert(Arc::from(pkg_id.as_str()), package(&name, "1.0.0", &[], is_leaf));
        let edges = if is_leaf {
            Vec::new()
        } else {
            let next_name = format!("chain-{}", idx + 1);
            let next_id = format!("{next_name}@1.0.0");
            vec![child_edge(&next_name, &next_id)]
        };
        children_by_id.insert(Arc::from(pkg_id.as_str()), Arc::new(edges));
    }
    let mut dependencies_tree = HashMap::default();
    let mut direct = Vec::new();
    add_lazy_direct_dep(&mut dependencies_tree, &mut direct, "chain-0", "chain-0@1.0.0");
    ResolvedTree {
        direct,
        packages,
        dependencies_tree,
        all_peer_dep_names: HashSet::default(),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id,
    }
}

#[test]
fn active_ancestors_unwind_without_growing_graph_past_max_snapshots() {
    let mut tree = deep_chain_tree(6);
    let direct = tree.direct.clone();
    let mut walker = walker_for_tests(&mut tree);
    walker.traversal.snapshot_budget.max_snapshots = Some(1);
    let root = RootWalk::of(&walker, &direct);

    walker.resolve_importer_dep(&direct[0], &root.context());

    assert_eq!(
        walker.traversal.snapshot_budget.abort,
        Some(crate::PeerWalkAbort::SnapshotLimitExceeded),
    );
    let graph_len = walker.output.graph.len();
    assert!(graph_len <= 1, "active ancestors must unwind at cap, got {graph_len} entries");
    let record_len = walker.output.node_records.len();
    assert!(record_len <= 1, "node_records must stay bounded at cap, got {record_len} entries");
    assert!(walker.traversal.in_progress.is_empty());
}

#[test]
fn resolve_peers_workspace_aborts_on_deep_chain_with_max_snapshots() {
    let mut tree = deep_chain_tree(6);
    let importers = [ImporterPeerInput {
        declared_direct_dependencies: HashSet::default(),
        explicitly_requested_direct_dependencies: HashSet::default(),
        id: ".".to_string(),
        direct: tree.direct.clone(),
        root_dir: Path::new("/workspace").to_path_buf(),
        modules_dir: None,
        rejected_optional_peers: HashSet::default(),
    }];
    let result = resolve_peers_workspace(
        &mut tree,
        &importers,
        Path::new("/workspace"),
        WorkspacePeerSettings { max_snapshots: Some(1), ..Default::default() },
        ResolvePeersOptions {
            links: crate::PeerLinkOptions {
                exclude_links_from_lockfile: false,
                lockfile_dir: Some(Path::new("/workspace").to_path_buf()),
                modules_dir: None,
            },
            ..ResolvePeersOptions::default()
        },
    );
    assert_eq!(result.abort, Some(crate::PeerWalkAbort::SnapshotLimitExceeded));
    assert!(result.graph.is_empty());
}
