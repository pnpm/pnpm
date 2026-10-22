use pipe_trait::Pipe;
use pnpm_lockfile::{Lockfile, PkgNameVerPeer};
use pnpm_resolving_resolver_base::ResolveOptions;
use std::{fmt::Write, path::Path, sync::Arc};

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
    ctx_for(CYCLE, dedupe)
}

fn ctx_for(lockfile: &str, dedupe: &[&str]) -> (TreeCtx, Arc<Lockfile>) {
    let lockfile = Lockfile::parse(lockfile, Path::new("pnpm-lock.yaml"))
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

/// `l{i}` depends on `a{i}` and `b{i}`, which both depend on `l{i+1}`, and
/// the last layer depends on `l0`. A walk that visits an undecided node
/// once per path to it takes 2^`LAYERS` visits.
#[test]
fn a_node_reached_twice_inside_an_undecided_cycle_is_walked_once() {
    const LAYERS: usize = 64;
    let mut packages = String::new();
    let mut snapshots = String::new();
    for layer in 0..LAYERS {
        let next = (layer + 1) % LAYERS;
        for name in [format!("l{layer}"), format!("a{layer}"), format!("b{layer}")] {
            writeln!(packages, "  {name}@1.0.0:\n    resolution: {{integrity: sha512-{name}}}")
                .expect("write to a string");
        }
        writeln!(snapshots, "  l{layer}@1.0.0:\n    dependencies:")
            .and_then(|()| writeln!(snapshots, "      a{layer}: 1.0.0\n      b{layer}: 1.0.0"))
            .expect("write to a string");
        for name in [format!("a{layer}"), format!("b{layer}")] {
            writeln!(snapshots, "  {name}@1.0.0:\n    dependencies:\n      l{next}: 1.0.0").expect(
                "write to a string",
            );
        }
    }
    let lockfile = format!("lockfileVersion: '9.0'\npackages:\n{packages}snapshots:\n{snapshots}");
    let (ctx, lockfile) = ctx_for(&lockfile, &[]);
    assert!(reusable(&ctx, &lockfile, "l0@1.0.0"));
}
