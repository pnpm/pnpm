use super::collect_finalized;
use crate::{
    resolve_dependency_tree::{
        TreeCtx,
        lock_recoverable,
        test_support::manifest_result,
        workspace_ctx::{
            RecordedChildren,
            WorkspaceTreeCtx,
        },
    },
    resolved_tree::{
        ChildEdge,
        ResolvedPackage,
        ResolvedPackageInput,
    },
};
use pnpm_resolving_resolver_base::ResolveOptions;
use pretty_assertions::assert_eq;
use std::sync::Arc;

/// A package of the graph a sweep runs on: settled with its children
/// recorded, or not settled.
enum Package<'a> {
    Recorded(&'a [&'a str]),
    Unsettled,
}

/// A context whose workspace holds `packages`, none of them declaring
/// peers, with `pending` queued for the next sweep in that order.
fn ctx(packages: &[(&str, Package<'_>)], pending: &[&str]) -> TreeCtx {
    let workspace = WorkspaceTreeCtx::default();
    for (pkg_id, package) in packages {
        lock_recoverable(&workspace.tree.packages).insert(Arc::from(*pkg_id), resolved(pkg_id));
        let Package::Recorded(children) = package else { continue };
        record(&workspace, pkg_id, children);
    }
    lock_recoverable(&workspace.finalization.finalization_pending)
        .extend(pending.iter().map(|&pkg_id| Arc::from(pkg_id)));
    TreeCtx::with_workspace(Arc::new(workspace), ResolveOptions::default())
}

fn resolved(pkg_id: &str) -> ResolvedPackage {
    ResolvedPackage::new(ResolvedPackageInput {
        id: Arc::from(pkg_id),
        result: Arc::new(manifest_result(serde_json::json!({}))),
        peer_set: crate::PeerSet::default(),
        optional: false,
        is_leaf: false,
    })
}

/// Record `children` as `pkg_id`'s, and `pkg_id` as a parent of each.
fn record(workspace: &WorkspaceTreeCtx, pkg_id: &str, children: &[&str]) {
    let edges = children
        .iter()
        .map(|&child| ChildEdge {
            alias: child.to_string(),
            pkg_id: Arc::from(child),
            optional: false,
        })
        .collect();
    lock_recoverable(&workspace.children.by_id)
        .insert(Arc::from(pkg_id), RecordedChildren::with_edges(edges));
    let mut parents_by_id = lock_recoverable(&workspace.finalization.parents_by_id);
    for &child in children {
        parents_by_id
            .entry(Arc::from(child))
            .or_default()
            .insert(Arc::from(pkg_id));
    }
}

fn announced(ctx: &TreeCtx) -> Vec<String> {
    collect_finalized(ctx)
        .into_iter()
        .map(|package| package.pkg_id.to_string())
        .collect()
}

/// `a` depends on `b` and then on the unsettled `c`, and `b` back on `a`.
/// Neither is announced, whichever the sweep meets first. When that is
/// `a`, its walk reaches `b` while `a` is still undecided, and taking the
/// back edge to `a` as finalized would leave `b` memoised finalized, and
/// announced when the sweep meets it.
#[test]
fn a_cycle_member_reached_before_an_unsettled_package_is_not_announced() {
    for pending in [["b", "a"], ["a", "b"]] {
        let ctx = ctx(
            &[
                ("a", Package::Recorded(&["b", "c"])),
                ("b", Package::Recorded(&["a"])),
                ("c", Package::Unsettled),
            ],
            &pending,
        );
        assert_eq!(announced(&ctx), Vec::<String>::new(), "pending {pending:?}");
    }
}

/// A cycle whose members reach only each other is announced whole from
/// either member, the other found through its recorded parent.
#[test]
fn a_settled_cycle_is_announced_whole() {
    for pending in ["a", "b"] {
        let ctx =
            ctx(&[("a", Package::Recorded(&["b"])), ("b", Package::Recorded(&["a"]))], &[pending]);
        assert_eq!(announced(&ctx), ["a", "b"], "pending {pending}");
    }
}

/// A package is announced once its subtree settles, and not before: the
/// second sweep finds `b` recorded and announces `b` and `a`, which
/// depends on it.
#[test]
fn a_package_is_announced_once_its_child_settles() {
    let ctx = ctx(&[("a", Package::Recorded(&["b"])), ("b", Package::Unsettled)], &["a"]);
    assert_eq!(announced(&ctx), Vec::<String>::new());
    record(&ctx.workspace, "b", &[]);
    lock_recoverable(&ctx.workspace.finalization.finalization_pending).push(Arc::from("b"));
    assert_eq!(announced(&ctx), ["a", "b"]);
}

/// A chain tens of thousands of packages deep is swept on a test thread's
/// stack: the walk keeps its own frames rather than recursing per package.
#[test]
fn a_deep_chain_does_not_overflow_the_stack() {
    const LENGTH: usize = 50_000;
    let names: Vec<String> = (0..LENGTH)
        .map(|index| format!("p{index}"))
        .collect();
    let children: Vec<[&str; 1]> = names
        .iter()
        .skip(1)
        .map(|name| [name.as_str()])
        .collect();
    let packages: Vec<(&str, Package<'_>)> = names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let children = children
                .get(index)
                .map(|child| &child[..])
                .unwrap_or(&[]);
            (name.as_str(), Package::Recorded(children))
        })
        .collect();
    let ctx = ctx(&packages, &["p0"]);
    assert_eq!(announced(&ctx), ["p0"]);
}
