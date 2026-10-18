use pipe_trait::Pipe;
use pnpm_lockfile::{Lockfile, PkgNameVerPeer};
use pnpm_resolving_resolver_base::ResolveOptions;
use std::{path::Path, sync::Arc};

use super::subtree_fully_reusable;
use crate::resolve_dependency_tree::{TreeCtx, workspace_ctx::WorkspaceTreeCtx};

/// `a` and `b` depend on each other, and `a` also depends on `c`.
const CYCLE: &str = "lockfileVersion: '9.0'
packages:
  a@1.0.0:
    resolution: {integrity: sha512-a}
  b@1.0.0:
    resolution: {integrity: sha512-b}
  c@1.0.0:
    resolution: {integrity: sha512-c}
snapshots:
  a@1.0.0:
    dependencies:
      b: 1.0.0
      c: 1.0.0
  b@1.0.0:
    dependencies:
      a: 1.0.0
  c@1.0.0: {}
";

fn ctx(dedupe: &[&str]) -> (TreeCtx, Arc<Lockfile>) {
    let lockfile = Lockfile::parse(CYCLE, Path::new("pnpm-lock.yaml"))
        .expect("parse the lockfile")
        .expect("the lockfile has content")
        .pipe(Arc::new);
    let mut targets = crate::UpdateTargets::default();
    for name in dedupe {
        targets.insert((*name).to_string(), None);
    }
    let workspace = WorkspaceTreeCtx::default()
        .with_lockfile_reuse(crate::WorkspaceLockfileReuse {
            lockfile: Some(Arc::clone(&lockfile)),
            dedupe: targets,
            ..Default::default()
        });
    (TreeCtx::with_workspace(Arc::new(workspace), ResolveOptions::default()), lockfile)
}

fn reusable(ctx: &TreeCtx, lockfile: &Lockfile, key: &str) -> bool {
    let key: PkgNameVerPeer = key.parse().expect("parse the snapshot key");
    subtree_fully_reusable(ctx, lockfile, &key, 0)
}

#[test]
fn every_member_of_a_reusable_cycle_is_reusable() {
    let (ctx, lockfile) = ctx(&[]);
    assert!(reusable(&ctx, &lockfile, "a@1.0.0"));
    assert!(reusable(&ctx, &lockfile, "b@1.0.0"));
}

#[test]
fn a_node_reached_through_the_cycle_decides_every_member() {
    let (ctx, lockfile) = ctx(&["c"]);
    assert!(!reusable(&ctx, &lockfile, "a@1.0.0"));
    assert!(!reusable(&ctx, &lockfile, "b@1.0.0"), "`b` reaches `c` through `a`");
}

#[test]
fn a_cycle_entered_from_another_member_reaches_the_same_answer() {
    let (ctx, lockfile) = ctx(&["c"]);
    assert!(!reusable(&ctx, &lockfile, "b@1.0.0"));
    assert!(!reusable(&ctx, &lockfile, "a@1.0.0"));
}
