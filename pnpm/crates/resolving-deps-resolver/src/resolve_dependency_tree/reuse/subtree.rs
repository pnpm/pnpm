//! Whether a lockfile node's whole transitive subtree can be synthesized
//! from the snapshot graph ([`fn@subtree_fully_reusable`]), and how far
//! each node is from a `pacquet update` target ([`TargetDistances`]).

pub(crate) use target_distance::TargetDistances;

mod target_distance;

use pnpm_lockfile::{
    PkgNameVerPeer,
    SnapshotEntry,
};
use rustc_hash::FxHashMap as HashMap;
use std::sync::{
    Arc,
    Mutex,
};

use crate::{
    UpdateReuseScope,
    lockfile_reuse::synthesize_reused_result,
};

use super::super::{
    lock_recoverable,
    region::{
        self,
        Inspected,
    },
    tree_ctx::TreeCtx,
};

/// `true` when `key` and its entire transitive subtree can be
/// synthesized from `lockfile`: every node a plain-semver registry
/// package without declared peers present in `packages:`, every snapshot child non-`link:`, no
/// node a dedupe target, and no `pacquet update` target at a depth the
/// update reaches. Memoised on [`LockfileReuseCache::reusable`] so each
/// package is checked once.
///
/// Only `key`'s own depth is checked against the update. A target within
/// the ceiling of a descendant is within the ceiling of its ancestors, so
/// below a node that passes the check every node passes it, and what is
/// memoised is independent of depth.
pub(super) fn subtree_fully_reusable(
    ctx: &TreeCtx,
    lockfile: &pnpm_lockfile::Lockfile,
    key: &PkgNameVerPeer,
    depth: i32,
) -> bool {
    if update_reaches_target_below(ctx, key, depth) {
        return false;
    }
    if let Some(&reusable) = lock_recoverable(&ctx.workspace.cache.lockfile_reuse.reusable).get(key)
    {
        return reusable;
    }
    let answers = region::decide(key, |key| {
        let memoised =
            lock_recoverable(&ctx.workspace.cache.lockfile_reuse.reusable).get(key).copied();
        match memoised {
            Some(reusable) => Inspected::Settled(reusable),
            None => own_children(ctx, lockfile, key)
                .map(Inspected::Children)
                .unwrap_or(Inspected::Settled(false)),
        }
    });
    let reusable = answers[0].1;
    lock_recoverable(&ctx.workspace.cache.lockfile_reuse.reusable).extend(answers);
    reusable
}

/// Whether the `pacquet update` reaches a target in `key`'s subtree:
/// `key` itself or a node below it, at a depth within the `--depth`
/// ceiling. Such a subtree re-resolves whole, so the bump's new
/// transitive deps are picked up.
fn update_reaches_target_below(ctx: &TreeCtx, key: &PkgNameVerPeer, depth: i32) -> bool {
    let Some(distance) = ctx.target_distances().to_target(key) else { return false };
    ctx.update_scope().max_depth
        .reaches(depth.saturating_add(distance))
}

/// Memoized decisions about the wanted lockfile, shared across importers.
#[derive(Default)]
pub(crate) struct LockfileReuseCache {
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

/// `key`'s snapshot children, or `None` when `key` is not reusable whatever
/// its children are.
///
/// A package whose scope was routed to another registry since the
/// lockfile was resolved is not: its children were pinned from the old
/// registry, and only its parent's manifest, which a reused parent
/// does not read, carries the ranges they re-resolve from. See
/// [`pnpm_lockfile::RegistryContext::moved_registry`].
fn own_children(
    ctx: &TreeCtx,
    lockfile: &pnpm_lockfile::Lockfile,
    key: &PkgNameVerPeer,
) -> Option<Vec<PkgNameVerPeer>> {
    let name = key.name.to_string();
    // Snapshot edges merge children and peer providers. Their manifest
    // must be read before the current scope can split them again.
    if declares_peers(lockfile, key)
        || ctx.workspace.reuse.dedupe.covers(&name, key.suffix.version_semver())
        || ctx.workspace.cache.registry_context.moved_registry(key).is_some()
        || synthesize_reused_result(lockfile, key, &name).is_none()
    {
        return None;
    }
    // No snapshot entry → the lockfile doesn't record this node's
    // children, so the reuse walk can't reproduce its subtree. Force a
    // fresh resolve rather than risk silently dropping transitive deps. A
    // genuine leaf has an empty-but-*present* snapshot entry (`{}`); a
    // missing one means an inconsistent lockfile, which `try_reuse_node`'s
    // contract sends to a fresh resolve.
    let snapshot = lockfile.snapshots.as_ref()?.get(key)?;
    // A `link:` child (no snapshot key) makes the subtree non-reusable:
    // the linked importer resolves its own deps, which this reuse path
    // doesn't model.
    snapshot_children(snapshot).collect()
}

fn declares_peers(lockfile: &pnpm_lockfile::Lockfile, key: &PkgNameVerPeer) -> bool {
    lockfile.packages
        .as_ref()
        .and_then(|packages| packages.get(&key.without_peer()))
        .is_some_and(|metadata| {
            metadata.peer_dependencies
                .as_ref()
                .is_some_and(|peers| !peers.is_empty())
                || metadata.peer_dependencies_meta
                    .as_ref()
                    .is_some_and(|peers| !peers.is_empty())
        })
}

#[cfg(test)]
mod tests;
