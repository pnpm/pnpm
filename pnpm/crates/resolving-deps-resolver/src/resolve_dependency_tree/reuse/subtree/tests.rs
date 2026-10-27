use pipe_trait::Pipe;
use pnpm_lockfile::{Lockfile, PkgNameVerPeer};
use pnpm_resolving_resolver_base::ResolveOptions;
use std::{fmt::Write, path::Path, sync::Arc};

use super::subtree_fully_reusable;
use crate::resolve_dependency_tree::{TreeCtx, lock_recoverable, workspace_ctx::WorkspaceTreeCtx};

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

/// A context for `pacquet update <targets> --depth <depth>` over `lockfile`.
fn update_ctx(lockfile: &str, targets: &[&str], depth: usize) -> (TreeCtx, Arc<Lockfile>) {
    let lockfile = Lockfile::parse(lockfile, Path::new("pnpm-lock.yaml"))
        .expect("parse the lockfile")
        .expect("the lockfile has content")
        .pipe(Arc::new);
    let targets = targets
        .iter()
        .map(|name| ((*name).to_string(), None))
        .collect::<crate::UpdateTargets>();
    let workspace = WorkspaceTreeCtx::default()
        .with_lockfile_reuse(crate::WorkspaceLockfileReuse {
            lockfile: Some(Arc::clone(&lockfile)),
            scope: crate::UpdateReuseScope::Except(targets),
            depth: crate::UpdateDepth::new(depth),
            ..Default::default()
        });
    (TreeCtx::with_workspace(Arc::new(workspace), ResolveOptions::default()), lockfile)
}

fn reusable(ctx: &TreeCtx, lockfile: &Lockfile, key: &str) -> bool {
    reusable_at(ctx, lockfile, key, 0)
}

fn reusable_at(ctx: &TreeCtx, lockfile: &Lockfile, key: &str, depth: i32) -> bool {
    let key: PkgNameVerPeer = key.parse().expect("parse the snapshot key");
    subtree_fully_reusable(ctx, lockfile, &key, depth)
}

fn memoised_answers(ctx: &TreeCtx) -> usize {
    lock_recoverable(&ctx.workspace.cache.subtree.reusable).len()
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

/// Each lap of the cycle is one level deeper. Far above the ceiling the
/// laps share one answer, so memory and stack stay independent of
/// `--depth` (<https://github.com/pnpm/pnpm/issues/17329>).
#[test]
fn a_cycle_far_above_the_depth_ceiling_is_walked_once() {
    let (ctx, lockfile) = update_ctx(CYCLE, &["d"], 200);
    assert!(reusable(&ctx, &lockfile, "a@1.0.0"));
    assert_eq!(memoised_answers(&ctx), 3);
}

/// `a` depends on `b`, which depends on the update target `c`.
const CHAIN: &str = "lockfileVersion: '9.0'
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
  b@1.0.0:
    dependencies:
      c: 1.0.0
  c@1.0.0: {}
";

/// Under `--depth 4`, `c` sits two levels below `a`, so it is an update
/// target only while `a` is at depth 2 or above. The memo holds one
/// answer per node however many depths ask.
#[test]
fn a_depth_near_the_ceiling_keeps_its_own_answer() {
    let (ctx, lockfile) = update_ctx(CHAIN, &["c"], 4);
    assert!(reusable_at(&ctx, &lockfile, "a@1.0.0", 3));
    assert!(!reusable_at(&ctx, &lockfile, "a@1.0.0", 2));
    assert!(!reusable_at(&ctx, &lockfile, "a@1.0.0", 1));
    assert!(!reusable_at(&ctx, &lockfile, "a@1.0.0", 0));
    assert!(reusable_at(&ctx, &lockfile, "a@1.0.0", 4));
    assert_eq!(memoised_answers(&ctx), 3);
}

/// The depth of a target is its shortest path from the node: under
/// `--depth 1`, `c` is one level below `a` but two below `b`, whose
/// subtree the update therefore leaves alone. `b`'s walk memoises `a` as
/// synthesizable, which must not stand in for `a`'s own answer.
#[test]
fn a_target_counts_at_its_shortest_depth() {
    let (ctx, lockfile) = update_ctx(CYCLE, &["c"], 1);
    assert!(reusable_at(&ctx, &lockfile, "b@1.0.0", 0));
    assert!(!reusable_at(&ctx, &lockfile, "a@1.0.0", 0));
}

/// The walk's cost does not depend on how many snapshots the lockfile
/// holds beyond the subtree: a small cycle in a lockfile with more
/// snapshots than `--depth` levels is still walked once
/// (<https://github.com/pnpm/pnpm/issues/17329>).
#[test]
fn a_small_cycle_in_a_lockfile_larger_than_the_depth_is_walked_once() {
    let mut lockfile = CYCLE.to_string();
    for index in 0..300 {
        writeln!(lockfile, "  unrelated-{index}@1.0.0: {{}}").expect("write the snapshot");
    }
    let (ctx, lockfile) = update_ctx(&lockfile, &["d"], 100);
    assert!(reusable(&ctx, &lockfile, "a@1.0.0"));
    assert_eq!(memoised_answers(&ctx), 3);
}

/// A filtered workspace update gives every selected importer a scope of
/// its own. Importers whose scopes name the same targets share one set of
/// target distances.
#[test]
fn importers_updating_the_same_targets_share_their_target_distances() {
    let lockfile = Lockfile::parse(CHAIN, Path::new("pnpm-lock.yaml"))
        .expect("parse the lockfile")
        .expect("the lockfile has content")
        .pipe(Arc::new);
    let targets = std::iter::once(("c".to_string(), None)).collect::<crate::UpdateTargets>();
    let workspace = WorkspaceTreeCtx::default()
        .with_lockfile_reuse(crate::WorkspaceLockfileReuse {
            lockfile: Some(Arc::clone(&lockfile)),
            scopes_by_importer: ["first", "second"]
                .into_iter()
                .map(|importer| {
                    (importer.to_string(), crate::UpdateReuseScope::Except(targets.clone()))
                })
                .collect(),
            depth: crate::UpdateDepth::new(4),
            ..Default::default()
        })
        .pipe(Arc::new);
    for importer in ["first", "second"] {
        let ctx = TreeCtx::with_workspace(Arc::clone(&workspace), ResolveOptions::default())
            .with_importer_id(importer);
        assert!(!reusable(&ctx, &lockfile, "a@1.0.0"));
    }
    assert_eq!(lock_recoverable(&workspace.cache.subtree.target_distances).len(), 1);
}

#[test]
fn peers_require_the_manifest_even_when_the_suffix_is_absent() {
    for declaration in [
        "peerDependencies: { provider: '*' }",
        "peerDependenciesMeta: { provider: { optional: true } }",
    ] {
        let input = CYCLE.replace(
            "resolution: {integrity: sha512-c}",
            &format!("resolution: {{integrity: sha512-c}}\n    {declaration}"),
        );
        let (ctx, lockfile) = ctx_for(&input, &[]);
        for name in ["a", "b", "c"] {
            let key = format!("{name}@1.0.0").parse().unwrap();
            assert!(!dbg!(subtree_fully_reusable(&ctx, &lockfile, &key, 0)));
        }
    }
}
