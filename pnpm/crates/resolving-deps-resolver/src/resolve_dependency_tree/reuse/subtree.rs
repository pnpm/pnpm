//! Whether a lockfile node's whole transitive subtree can be synthesized
//! from the snapshot graph ([`fn@subtree_fully_reusable`]).

use pnpm_lockfile::PkgNameVerPeer;
use rustc_hash::FxHashMap as HashMap;

use crate::lockfile_reuse::synthesize_reused_result;

use super::super::{
    lock_recoverable, tree_ctx::TreeCtx, update_scope::update_excludes,
    workspace_ctx::SubtreeReuseKey,
};

/// `true` when `key` and its entire transitive subtree can be
/// synthesized from `lockfile`: every node a plain-semver registry
/// package present in `packages:`, every snapshot child non-`link:`, and
/// no node a `pacquet update` or dedupe target. Memoised on
/// [`crate::resolve_dependency_tree::workspace_ctx::WorkspaceResolutionCache::subtree_reusable`]
/// so each package is checked once.
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
    SubtreeWalk { ctx, lockfile, path: HashMap::default(), undecided: Vec::new() }.visit(key, depth)
        .reusable
}

struct SubtreeWalk<'a> {
    ctx: &'a TreeCtx,
    lockfile: &'a pnpm_lockfile::Lockfile,
    /// The nodes on the walk's current path, by their position on it.
    path: HashMap<SubtreeReuseKey, usize>,
    /// The nodes found reusable that reach a node still on the path. They
    /// take that node's answer when its component is decided.
    undecided: Vec<SubtreeReuseKey>,
}

struct Visit {
    reusable: bool,
    /// The lowest path position the subtree reached through a cycle, or
    /// `usize::MAX` when it reached none.
    lowest_reached: usize,
}

impl Visit {
    const DECIDED_FALSE: Visit = Visit { reusable: false, lowest_reached: usize::MAX };
}

impl SubtreeWalk<'_> {
    fn visit(&mut self, key: &PkgNameVerPeer, depth: i32) -> Visit {
        let scope = self.ctx.update_scope();
        let memo_key =
            (self.ctx.update_cache_scope(), key.clone(), scope.max_depth.memo_bucket(depth));
        if let Some(&reusable) =
            lock_recoverable(&self.ctx.workspace.cache.subtree_reusable).get(&memo_key)
        {
            return Visit { reusable, lowest_reached: usize::MAX };
        }
        if let Some(&position) = self.path.get(&memo_key) {
            return Visit { reusable: true, lowest_reached: position };
        }
        let position = self.path.len();
        self.path.insert(memo_key.clone(), position);
        let undecided_from = self.undecided.len();
        let visit = self.visit_node(key, depth);
        self.path.remove(&memo_key);
        if visit.reusable && visit.lowest_reached < position {
            self.undecided.push(memo_key);
            return visit;
        }
        // Every undecided node above `undecided_from` reaches this one, so a
        // non-reusable node decides them all. A reusable node that reached
        // nothing above itself closes its component.
        self.decide(memo_key, undecided_from, visit.reusable);
        Visit { reusable: visit.reusable, lowest_reached: usize::MAX }
    }

    /// Whether `key` itself is reusable, then its snapshot children.
    fn visit_node(&mut self, key: &PkgNameVerPeer, depth: i32) -> Visit {
        let scope = self.ctx.update_scope();
        // A `pacquet update` target anywhere in the subtree forces the whole
        // subtree to re-resolve so the bump's new transitive deps are picked
        // up — update names match at every depth the update reaches.
        let name = key.name.to_string();
        let version = key.suffix.version_semver();
        if self.ctx.workspace.reuse.dedupe.covers(&name, version)
            || update_excludes(scope, &name, version, depth)
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
        let lockfile = self.lockfile;
        let Some(snapshot) = lockfile.snapshots
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
        let dep_maps = [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()];
        for (child_name, dep_ref) in dep_maps.into_iter().flatten().flatten() {
            let Some(child_key) = dep_ref.resolve(child_name) else {
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

    fn decide(&mut self, memo_key: SubtreeReuseKey, undecided_from: usize, reusable: bool) {
        let mut memo = lock_recoverable(&self.ctx.workspace.cache.subtree_reusable);
        for member in self.undecided.drain(undecided_from..) {
            memo.insert(member, reusable);
        }
        memo.insert(memo_key, reusable);
    }
}

#[cfg(test)]
mod tests;
