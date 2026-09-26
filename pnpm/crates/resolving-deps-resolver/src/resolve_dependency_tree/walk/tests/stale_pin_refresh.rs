use pnpm_lockfile::SnapshotEntry;
use pnpm_resolving_resolver_base::ResolveOptions;
use std::sync::Arc;

use super::super::child_seeds::{ChildSeedScope, child_wanted};
use crate::resolve_dependency_tree::{
    TreeCtx,
    workspace_ctx::{DirectDepVersions, WorkspaceTreeCtx},
};

fn ctx(dedupe: crate::UpdateTargets) -> TreeCtx {
    let workspace = WorkspaceTreeCtx::default()
        .with_lockfile_reuse(crate::WorkspaceLockfileReuse { dedupe, ..Default::default() });
    TreeCtx::with_workspace(Arc::new(workspace), ResolveOptions::default())
}

/// A parent whose lockfile snapshot pins `react@17.0.2`, in an importer
/// whose direct dependencies resolved `react` to both 17.0.2 and 18.2.0.
fn child_wanted_for_react(ctx: &TreeCtx) -> (Option<String>, Option<String>) {
    let snapshot: SnapshotEntry =
        serde_json::from_value(serde_json::json!({ "dependencies": { "react": "17.0.2" } }))
            .expect("parse snapshot entry");
    let mut direct_versions = DirectDepVersions::default();
    direct_versions.insert(
        "react".to_string(),
        vec!["17.0.2".parse().expect("parse version"), "18.2.0".parse().expect("parse version")],
    );
    let scope = ChildSeedScope {
        prior_children_snapshot: Some(&snapshot),
        direct_versions: Some(Arc::new(direct_versions)),
        declaring_dir: None,
        parent_is_workspace: false,
        parent_is_directory: true,
    };
    let spec = ("react".to_string(), "^17.0.0 || ^18.0.0".to_string(), false, false);
    let (wanted, prior) = child_wanted(ctx, &scope, &spec, 1);
    (wanted.bare_specifier, prior.map(|key| key.to_string()))
}

#[test]
fn a_kept_pin_is_refreshed_onto_the_higher_direct_dependency_version() {
    assert_eq!(
        child_wanted_for_react(&ctx(crate::UpdateTargets::default())),
        (Some("18.2.0".to_string()), None),
    );
}

#[test]
fn a_dedupe_target_is_left_to_the_preferred_versions() {
    let mut dedupe = crate::UpdateTargets::default();
    dedupe.insert("react".to_string(), None);
    assert_eq!(
        child_wanted_for_react(&ctx(dedupe)),
        (Some("^17.0.0 || ^18.0.0".to_string()), Some("react@17.0.2".to_string())),
    );
}
