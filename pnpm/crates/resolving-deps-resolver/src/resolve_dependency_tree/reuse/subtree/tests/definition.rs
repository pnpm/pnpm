//! Generated lockfiles, against a statement of what each answer must be:
//! the root's subtree holds no update target within the `--depth` ceiling,
//! and every snapshot the root reaches is reusable on its own.

use super::{
    ctx_for,
    update_ctx,
};
use crate::resolve_dependency_tree::{
    TreeCtx,
    lock_recoverable,
};
use pnpm_lockfile::{
    Lockfile,
    PkgNameVerPeer,
};
use std::{
    collections::VecDeque,
    fmt::Write,
    sync::Arc,
};

/// A fixed linear congruential generator, so a failure names a case that
/// reruns.
struct Rng(u64);

impl Rng {
    fn below(&mut self, bound: usize) -> usize {
        self.0 =
            self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        usize::try_from(self.0 >> 33).expect("31 bits fit a usize") % bound
    }

    fn flags(&mut self, count: usize, percent: usize) -> Vec<bool> {
        (0..count)
            .map(|_| self.below(100) < percent)
            .collect()
    }
}

/// What makes each snapshot unusable on its own.
struct Flaws {
    peers: Vec<bool>,
    no_package: Vec<bool>,
    no_snapshot: Vec<bool>,
    link_child: Vec<bool>,
    dedupe: Vec<bool>,
}

/// A lockfile of snapshots `p0`, `p1`, ..., each depending on
/// `children[node]`, and the `pacquet update` scope it is reused under:
/// `targets` up to `depth`, or no update and the `dedupe` targets.
struct Case {
    children: Vec<Vec<usize>>,
    flaws: Flaws,
    targets: Vec<bool>,
    depth: Option<usize>,
}

impl Case {
    fn generate(rng: &mut Rng) -> Case {
        let count = 1 + rng.below(7);
        let flaws = Flaws {
            peers: rng.flags(count, 8),
            no_package: rng.flags(count, 5),
            no_snapshot: rng.flags(count, 6),
            link_child: rng.flags(count, 6),
            dedupe: rng.flags(count, 8),
        };
        let targets = rng.flags(count, 15);
        let children = (0..count)
            .map(|_| {
                let mut chosen: Vec<usize> = (0..rng.below(4))
                    .map(|_| rng.below(count))
                    .collect();
                chosen.sort_unstable();
                chosen.dedup();
                chosen
            })
            .collect();
        let depth = (rng.below(2) == 0).then(|| rng.below(5));
        Case { children, flaws, targets, depth }
    }

    fn lockfile(&self) -> String {
        let mut packages = String::new();
        let mut snapshots = String::new();
        for node in 0..self.children.len() {
            self.write_package(&mut packages, node);
            self.write_snapshot(&mut snapshots, node);
        }
        format!("lockfileVersion: '9.0'\npackages:\n{packages}snapshots:\n{snapshots}")
    }

    fn write_package(&self, packages: &mut String, node: usize) {
        if self.flaws.no_package[node] {
            return;
        }
        writeln!(packages, "  p{node}@1.0.0:\n    resolution: {{integrity: sha512-p{node}}}")
            .expect("write to a string");
        if self.flaws.peers[node] {
            writeln!(packages, "    peerDependencies: {{ provider: '*' }}").expect(
                "write to a string",
            );
        }
    }

    fn write_snapshot(&self, snapshots: &mut String, node: usize) {
        if self.flaws.no_snapshot[node] {
            return;
        }
        if self.children[node].is_empty() && !self.flaws.link_child[node] {
            writeln!(snapshots, "  p{node}@1.0.0: {{}}").expect("write to a string");
            return;
        }
        writeln!(snapshots, "  p{node}@1.0.0:\n    dependencies:").expect("write to a string");
        for child in &self.children[node] {
            writeln!(snapshots, "      p{child}: 1.0.0").expect("write to a string");
        }
        if self.flaws.link_child[node] {
            writeln!(snapshots, "      linked: link:../linked").expect("write to a string");
        }
    }

    fn context(&self) -> (TreeCtx, Arc<Lockfile>) {
        let named = |chosen: &[bool]| -> Vec<String> {
            (0..chosen.len())
                .filter(|&node| chosen[node])
                .map(|node| format!("p{node}"))
                .collect()
        };
        let lockfile = self.lockfile();
        let (names, depth) = match self.depth {
            Some(depth) => (named(&self.targets), Some(depth)),
            None => (named(&self.flaws.dedupe), None),
        };
        let names: Vec<&str> = names
            .iter()
            .map(String::as_str)
            .collect();
        match depth {
            Some(depth) => update_ctx(&lockfile, &names, depth),
            None => ctx_for(&lockfile, &names),
        }
    }

    /// The distance from `node` to each snapshot it reaches, itself
    /// included at 0.
    fn distances_from(&self, node: usize) -> Vec<Option<usize>> {
        let mut distance = vec![None; self.children.len()];
        distance[node] = Some(0);
        let mut queue = VecDeque::from([node]);
        while let Some(current) = queue.pop_front() {
            let reached = distance[current].expect("queued nodes have a distance") + 1;
            let children =
                if self.flaws.no_snapshot[current] { &[][..] } else { &self.children[current] };
            for &child in children {
                if distance[child].is_none() {
                    distance[child] = Some(reached);
                    queue.push_back(child);
                }
            }
        }
        distance
    }

    fn reusable_on_its_own(&self, node: usize) -> bool {
        let flaws = &self.flaws;
        let deduped = self.depth.is_none() && flaws.dedupe[node];
        !(flaws.peers[node]
            || flaws.no_package[node]
            || flaws.no_snapshot[node]
            || flaws.link_child[node]
            || deduped)
    }

    /// Every snapshot `node` reaches is reusable on its own.
    fn synthesizable(&self, node: usize) -> bool {
        let distances = self.distances_from(node);
        (0..distances.len())
            .filter(|&other| distances[other].is_some())
            .all(|other| self.reusable_on_its_own(other))
    }

    /// What `subtree_fully_reusable` must answer for `node` at `depth`.
    fn expected(&self, node: usize, depth: usize) -> bool {
        let distances = self.distances_from(node);
        let reaches_target = self.depth.is_some_and(|ceiling| {
            (0..distances.len()).any(|other| {
                self.targets[other]
                    && distances[other].is_some_and(|distance| depth + distance <= ceiling)
            })
        });
        !reaches_target && self.synthesizable(node)
    }
}

fn key(node: usize) -> PkgNameVerPeer {
    format!("p{node}@1.0.0").parse().expect("parse the snapshot key")
}

/// Queries run in a random order on one context, so later answers come
/// from the memo earlier walks left, and every memoised answer is checked
/// against the definition too.
#[test]
fn the_walk_agrees_with_the_definition() {
    let mut rng = Rng(2026);
    for case_index in 0..4_000 {
        let case = Case::generate(&mut rng);
        let (ctx, lockfile) = case.context();
        for _ in 0..8 {
            let node = rng.below(case.children.len());
            let depth = rng.below(5);
            let answer = super::super::subtree_fully_reusable(
                &ctx,
                &lockfile,
                &key(node),
                i32::try_from(depth).expect("a small depth"),
            );
            assert_eq!(
                answer,
                case.expected(node, depth),
                "case {case_index}: p{node} at depth {depth}",
            );
        }
        for (memo_key, &memoised) in
            lock_recoverable(&ctx.workspace.cache.lockfile_reuse.reusable).iter()
        {
            let node: usize = memo_key.name.to_string()[1..].parse().expect("a generated name");
            assert_eq!(memoised, case.synthesizable(node), "case {case_index}: memoised p{node}");
        }
    }
}
