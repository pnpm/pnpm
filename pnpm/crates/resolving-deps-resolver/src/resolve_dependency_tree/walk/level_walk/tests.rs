use super::{children_context, settle_one_seed};
use crate::{
    NodeId,
    parent_pkg_aliases::ParentPkgAliases,
    resolve_dependency_tree::{
        TreeCtx,
        test_support::manifest_result,
        walk::{
            NodeKind, NodeSeed, PendingNode, PendingNodeAncestry, PendingNodeIdentity,
            PendingNodeLockfile,
        },
        workspace_ctx::{WorkspaceTreeCtx, claim_children_owner, record_children},
    },
    resolved_tree::LockedResolution,
};
use pnpm_resolving_resolver_base::ResolveOptions;
use rustc_hash::FxHashSet as HashSet;
use std::sync::Arc;

#[test]
fn recorded_children_fast_path_retains_the_pending_occurrence_context() {
    let workspace = Arc::new(WorkspaceTreeCtx::default());
    let ctx = TreeCtx::with_workspace(Arc::clone(&workspace), ResolveOptions::default());
    let id: Arc<str> = Arc::from("consumer@1.0.0");
    let node_id = NodeId::next();
    let claim = claim_children_owner(&ctx, &id, 0, &Arc::default(), HashSet::default());
    let previous = "consumer@1.0.0(peer@2.0.0)";
    let mut pending = PendingNode {
        result: Arc::new(manifest_result(
            serde_json::json!({"name":"consumer", "version":"1.0.0"}),
        )),
        kind: NodeKind::Branch,
        resolves_children_through_catalogs: false,
        peer_shadowed: HashSet::default(),
        claim: None,
        identity: PendingNodeIdentity {
            id: Arc::clone(&id),
            alias: "consumer".into(),
            node_id: node_id.clone(),
        },
        lockfile: PendingNodeLockfile {
            prior_key: Some(previous.parse().unwrap()),
            locked_resolution: Some(Box::new(LockedResolution {
                previous_dep_path: Some(previous.into()),
                locked_peer_context: Some([("peer".into(), "peer@2.0.0".into())].into()),
                ..Default::default()
            })),
        },
        ancestry: PendingNodeAncestry {
            parent_ancestors: Arc::default(),
            next_ancestors: Arc::default(),
            depth: 0,
            current_is_optional: false,
        },
    };
    let scope = ParentPkgAliases::root(HashSet::default());
    let context = children_context(&ctx, &pending, &claim, &scope);
    record_children(&ctx, &id, &claim.owner, Vec::new(), context);
    pending.claim = Some(claim);
    let frontier = settle_one_seed(&ctx, NodeSeed::Pending(Box::new(pending)), None, &scope);
    assert!(frontier.is_none(), "recorded children must skip the walk");
    let tree = ctx.snapshot(Vec::new());
    let nodes = &tree.dependencies_tree;
    assert_eq!(nodes[&node_id].previous_dep_path(), Some(&previous.into()));
    assert_eq!(nodes[&node_id].locked_peer_context().unwrap()["peer"].as_str(), "peer@2.0.0");
}
