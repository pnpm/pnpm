//! Whether a lockfile node's whole transitive subtree can be synthesized
//! from the snapshot graph ([`fn@subtree_fully_reusable`]), and how far
//! each node is from a `pacquet update` target ([`TargetDistances`]).

pub(crate) use target_distance::TargetDistances;

mod target_distance;

use pnpm_lockfile::{PkgNameVerPeer, SnapshotEntry};
use rustc_hash::FxHashMap as HashMap;
use std::sync::{Arc, Mutex};

use crate::{UpdateReuseScope, lockfile_reuse::synthesize_reused_result};

use super::super::{lock_recoverable, tree_ctx::TreeCtx};

/// `true` when `key` and its entire transitive subtree can be
/// synthesized from `lockfile`: every node a plain-semver registry
/// package present in `packages:`, every snapshot child non-`link:`, no
/// node a dedupe target, and no `pacquet update` target at a depth the
/// update reaches. Memoised on [`SubtreeReuseCache::reusable`] so each
/// package is checked once.
///
/// The members of a dependency cycle reach each other, so they share one
/// answer. The walk decides each strongly connected component of the
/// snapshot graph once its first-visited member finishes, and memoises
/// only decided answers.
pub(super) fn subtree_fully_reusable(
    ctx: &TreeCtx,
    lockfile: &pnpm_lockfile::Lockfile,
    key: &PkgNameVerPeer,
    depth: i32,
) -> bool {
    SubtreeWalk {
        ctx,
        lockfile,
        distances: ctx.target_distances(),
        open: HashMap::default(),
        undecided: Vec::new(),
        discovered: 0,
    }
    .visit(key, depth)
    .reusable
}

/// What [`fn@subtree_fully_reusable`] memoises on the workspace.
#[derive(Default)]
pub(crate) struct SubtreeReuseCache {
    /// Per snapshot key: `true` means the package and its entire transitive
    /// subtree can be synthesized from the prior lockfile. Whether a
    /// `pacquet update` reaches a target inside the subtree is settled
    /// before the memo, by [`Self::target_distances`], so the answer holds
    /// for every importer and depth.
    pub(crate) reusable: Mutex<HashMap<PkgNameVerPeer, bool>>,
    /// [`TargetDistances`] per update scope, computed on first use.
    pub(crate) target_distances: Mutex<HashMap<UpdateReuseScope, Arc<TargetDistances>>>,
}

/// `snapshot`'s children: the snapshot key of each entry in
/// `dependencies` and `optionalDependencies`, or `None` for a `link:`
/// child, which has none.
pub(super) fn snapshot_children(
    snapshot: &SnapshotEntry,
) -> impl Iterator<Item = Option<PkgNameVerPeer>> + '_ {
    [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()]
        .into_iter()
        .flatten()
        .flatten()
        .map(|(child_name, dep_ref)| dep_ref.resolve(child_name))
}

struct SubtreeWalk<'a> {
    ctx: &'a TreeCtx,
    lockfile: &'a pnpm_lockfile::Lockfile,
    distances: &'a TargetDistances,
    /// The nodes discovered and not yet decided, by the order they were
    /// discovered in: the walk's current path and the [`Self::undecided`]
    /// nodes.
    open: HashMap<PkgNameVerPeer, usize>,
    /// The nodes found reusable that reach an open node discovered before
    /// them. They take the answer of their component, decided when its
    /// first-discovered member finishes.
    undecided: Vec<PkgNameVerPeer>,
    discovered: usize,
}

struct Visit {
    reusable: bool,
    /// The earliest discovered open node the subtree reached, or
    /// `usize::MAX` when it reached none.
    lowest_reached: usize,
}

impl Visit {
    const DECIDED_FALSE: Visit = Visit { reusable: false, lowest_reached: usize::MAX };
}

impl SubtreeWalk<'_> {
    fn visit(&mut self, key: &PkgNameVerPeer, depth: i32) -> Visit {
        if self.update_reaches_target_below(key, depth) {
            return Visit::DECIDED_FALSE;
        }
        if let Some(&reusable) =
            lock_recoverable(&self.ctx.workspace.cache.subtree.reusable).get(key)
        {
            return Visit { reusable, lowest_reached: usize::MAX };
        }
        if let Some(&discovered) = self.open.get(key) {
            return Visit { reusable: true, lowest_reached: discovered };
        }
        let discovered = self.discovered;
        self.discovered += 1;
        self.open.insert(key.clone(), discovered);
        let undecided_from = self.undecided.len();
        let visit = self.visit_node(key, depth);
        if visit.reusable && visit.lowest_reached < discovered {
            self.undecided.push(key.clone());
            return visit;
        }
        // Every undecided node above `undecided_from` reaches this one, so a
        // non-reusable node decides them all. A reusable node that reached
        // nothing discovered before it closes its component.
        self.decide(key.clone(), undecided_from, visit.reusable);
        Visit { reusable: visit.reusable, lowest_reached: usize::MAX }
    }

    /// Whether the `pacquet update` reaches a target in `key`'s subtree:
    /// `key` itself or a node below it, at a depth within the `--depth`
    /// ceiling. Such a subtree re-resolves whole, so the bump's new
    /// transitive deps are picked up.
    ///
    /// A target within the ceiling of a descendant is within the ceiling
    /// of its ancestors, so below a node this check passes it passes
    /// everywhere. The rest of the walk therefore never meets an update
    /// target, which keeps the memoised answers independent of depth.
    fn update_reaches_target_below(&self, key: &PkgNameVerPeer, depth: i32) -> bool {
        let Some(distance) = self.distances.to_target(key) else { return false };
        self.ctx
            .update_scope()
            .max_depth
            .reaches(depth.saturating_add(distance))
    }

    /// Whether `key` itself is reusable, then its snapshot children.
    fn visit_node(&mut self, key: &PkgNameVerPeer, depth: i32) -> Visit {
        let name = key.name.to_string();
        if self.ctx.workspace.reuse.dedupe.covers(&name, key.suffix.version_semver())
            || synthesize_reused_result(self.lockfile, key, &name).is_none()
        {
            return Visit::DECIDED_FALSE;
        }
        self.visit_children(key, depth)
    }

    /// Visit `key`'s snapshot children. A `link:` child (no snapshot key)
    /// makes the subtree non-reusable: the linked importer resolves its own
    /// deps, which this reuse path doesn't model.
    fn visit_children(&mut self, key: &PkgNameVerPeer, depth: i32) -> Visit {
        let Some(snapshot) = self.lockfile.snapshots
            .as_ref()
            .and_then(|snaps| snaps.get(key))
        else {
            // No snapshot entry → the lockfile doesn't record this node's
            // children, so the reuse walk can't reproduce its subtree.
            // Force a fresh resolve rather than risk silently dropping
            // transitive deps. A genuine leaf has an empty-but-*present*
            // snapshot entry (`{}`); a missing one means an inconsistent
            // lockfile, which `try_reuse_node`'s contract sends to a fresh
            // resolve.
            return Visit::DECIDED_FALSE;
        };
        let mut lowest_reached = usize::MAX;
        for child_key in snapshot_children(snapshot) {
            let Some(child_key) = child_key else {
                return Visit::DECIDED_FALSE;
            };
            let child = self.visit(&child_key, depth + 1);
            if !child.reusable {
                return Visit::DECIDED_FALSE;
            }
            lowest_reached = lowest_reached.min(child.lowest_reached);
        }
        Visit { reusable: true, lowest_reached }
    }

    fn decide(&mut self, key: PkgNameVerPeer, undecided_from: usize, reusable: bool) {
        let mut memo = lock_recoverable(&self.ctx.workspace.cache.subtree.reusable);
        for member in self.undecided
            .drain(undecided_from..)
            .chain([key])
        {
            self.open.remove(&member);
            memo.insert(member, reusable);
        }
    }
}

#[cfg(test)]
mod tests;
