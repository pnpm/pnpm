use super::{
    super::{
        HashMap, HashSet, ParentPkgAliases, RecordedChildrenContext, TreeCtx, claim_children_owner,
        record_children,
    },
    Occurrence, Scopes, next_level,
};
use crate::resolve_dependency_tree::workspace_ctx::WorkspaceTreeCtx;
use pnpm_resolving_resolver_base::ResolveOptions;
use std::sync::Arc;

fn record(ctx: &TreeCtx, pkg_id: &str, children: &[(&str, &str)]) {
    let pkg_id: Arc<str> = Arc::from(pkg_id);
    let claim = claim_children_owner(ctx, &pkg_id, 0, &Arc::default(), HashSet::default());
    let edges = children
        .iter()
        .map(|(alias, child_id)| crate::resolved_tree::ChildEdge {
            alias: (*alias).to_string(),
            pkg_id: Arc::from(*child_id),
            optional: false,
        })
        .collect();
    let context = RecordedChildrenContext {
        peer_shadowed: Arc::default(),
        prior_key: None,
        update_active: false,
        children_scope: ParentPkgAliases::root(HashSet::default()),
        changes_locked_split: false,
    };
    record_children(ctx, &pkg_id, &claim.owner, edges, context);
}

fn occurrence(pkg_id: &str, alias_in_scope: &str) -> Occurrence {
    let scope = ParentPkgAliases::root(HashSet::from_iter([alias_in_scope.to_string()]));
    Occurrence {
        pkg_id: Arc::from(pkg_id),
        children_scopes: Scopes { current: Arc::clone(&scope), pinned: scope },
        parent_path: Arc::new(vec![Arc::from("root@1.0.0")]),
    }
}

/// Among occurrences on one level, a fresh walk gives a package's children
/// to the smallest parent path, whatever order the scan meets them in.
#[test]
fn expands_a_shared_package_from_the_occurrence_that_owns_its_children() {
    let ctx =
        TreeCtx::with_workspace(Arc::new(WorkspaceTreeCtx::default()), ResolveOptions::default());
    record(&ctx, "late@1.0.0", &[("shared", "shared@1.0.0")]);
    record(&ctx, "early@1.0.0", &[("shared", "shared@1.0.0")]);

    let (checks, next) = next_level(
        &ctx,
        vec![occurrence("late@1.0.0", "late-only"), occurrence("early@1.0.0", "early-only")],
        &mut HashMap::default(),
        1,
    );

    assert_eq!(checks.len(), 2, "every occurrence on the level is checked");
    let [shared] = next.as_slice() else { panic!("one expansion per package") };
    assert_eq!(shared.parent_path.last().map(|id| &**id), Some("early@1.0.0"));
    assert!(shared.children_scopes.current.contains("early-only"));
    assert!(!shared.children_scopes.current.contains("late-only"));
}
