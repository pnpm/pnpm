//! The per-level sweep behind [`super::FinalizedPackageFn`]: after a level
//! settles, every package whose whole subtree is now settled and
//! peer-free is announced once, so the install layer can materialize
//! it before peer resolution.

use pipe_trait::Pipe;
use rustc_hash::{
    FxHashMap as HashMap,
    FxHashSet as HashSet,
};
use std::sync::Arc;

use super::{
    FinalizedChild,
    FinalizedPackage,
    TreeCtx,
    lock_recoverable,
    region::{
        self,
        Inspected,
    },
    workspace_ctx::RecordedChildren,
};
use crate::resolved_tree::ResolvedPackage;

/// Announce every package that became finalized since the last sweep.
///
/// A package is finalized when it declares no `peerDependencies` (nor
/// `peerDependenciesMeta`), its children are recorded, and every child
/// is finalized. Nothing after the walk changes such a package: peer
/// resolution suffixes only nodes whose subtree carries peers, and the
/// dedupe passes only merge suffixed or injected nodes. A later
/// occurrence that re-records the package's children (a shallower
/// importer, a hoist round) can still change its edges; the install
/// layer's final pass re-links every slot's edges from the lockfile,
/// so an announcement is a head start, not a promise about edges.
///
/// A verdict can only change for a package written or re-recorded
/// since the last sweep, or for a package depending on one that just
/// became finalized, so the sweep starts from the former and walks up
/// the recorded parent edges from every new announcement. Over the
/// whole walk each package is inspected once per change to itself or
/// to a direct child, not once per level.
///
/// The members of a cycle reach each other, so they share one verdict,
/// whichever member the sweep inspects first.
pub(super) fn announce_finalized_packages(ctx: &TreeCtx) {
    let Some(finalized_package) = ctx.workspace.hooks.finalized_package.as_ref() else { return };
    let announcements = collect_finalized(ctx);
    for package in announcements {
        finalized_package(package);
    }
}

fn collect_finalized(ctx: &TreeCtx) -> Vec<FinalizedPackage> {
    let mut worklist =
        std::mem::take(&mut *lock_recoverable(&ctx.workspace.finalization.finalization_pending));
    if worklist.is_empty() {
        return Vec::new();
    }
    let packages = lock_recoverable(&ctx.workspace.tree.packages);
    let children_by_id = lock_recoverable(&ctx.workspace.children.by_id);
    let parents_by_id = lock_recoverable(&ctx.workspace.finalization.parents_by_id);
    let mut finalized_ids = lock_recoverable(&ctx.workspace.finalization.finalized_ids);
    let mut sweep = Sweep {
        packages: &packages,
        children_by_id: &children_by_id,
        finalized_ids: &finalized_ids,
        verdicts: HashMap::default(),
    };
    let mut newly_finalized: Vec<Arc<str>> = Vec::new();
    let mut seen: HashSet<Arc<str>> = HashSet::default();
    while let Some(pkg_id) = worklist.pop() {
        if finalized_ids.contains(&pkg_id) || !seen.insert(Arc::clone(&pkg_id)) {
            continue;
        }
        if !sweep.is_finalized(&pkg_id) {
            continue;
        }
        if let Some(parents) = parents_by_id.get(&pkg_id) {
            worklist.extend(parents.iter().cloned());
        }
        newly_finalized.push(pkg_id);
    }
    // Announce in a stable order so the install layer's work queue does
    // not depend on hash-map iteration.
    newly_finalized.sort();
    let announcements = newly_finalized
        .iter()
        .map(|pkg_id| announcement(pkg_id, &packages[pkg_id], children_by_id.get(pkg_id)))
        .collect();
    finalized_ids.extend(newly_finalized);
    announcements
}

fn announcement(
    pkg_id: &Arc<str>,
    package: &ResolvedPackage,
    recorded: Option<&RecordedChildren>,
) -> FinalizedPackage {
    let children = recorded
        .map(|recorded| recorded.edges.as_slice())
        .unwrap_or_default()
        .iter()
        .map(|edge| FinalizedChild {
            alias: edge.alias.clone(),
            pkg_id: Arc::clone(&edge.pkg_id),
            optional: edge.optional,
        })
        .collect();
    FinalizedPackage { pkg_id: Arc::clone(pkg_id), result: Arc::clone(package.result()), children }
}

/// One sweep's view of the graph. `verdicts` memoises this sweep's
/// answers.
struct Sweep<'a> {
    packages: &'a HashMap<Arc<str>, ResolvedPackage>,
    children_by_id: &'a HashMap<Arc<str>, RecordedChildren>,
    finalized_ids: &'a HashSet<Arc<str>>,
    verdicts: HashMap<Arc<str>, bool>,
}

impl Sweep<'_> {
    fn is_finalized(&mut self, pkg_id: &Arc<str>) -> bool {
        if let Some(verdict) = self.memoised(pkg_id) {
            return verdict;
        }
        let verdicts = region::decide(pkg_id, |pkg_id| match self.memoised(pkg_id) {
            Some(verdict) => Inspected::Settled(verdict),
            None => self
                .own_children(pkg_id)
                .map(Inspected::Children)
                .unwrap_or(Inspected::Settled(false)),
        });
        let verdict = verdicts[0].1;
        self.verdicts.extend(verdicts);
        verdict
    }

    /// The verdict an earlier sweep or this one reached on `pkg_id`.
    fn memoised(&self, pkg_id: &Arc<str>) -> Option<bool> {
        if self.finalized_ids.contains(pkg_id) {
            return Some(true);
        }
        self.verdicts.get(pkg_id).copied()
    }

    /// `pkg_id`'s recorded children, or `None` when it is not finalized
    /// whatever they are: it declares peers, is a `link:` package, or has
    /// not settled.
    fn own_children(&self, pkg_id: &Arc<str>) -> Option<Vec<Arc<str>>> {
        let package = self.packages.get(pkg_id)?;
        if !package.peer_set.taken().is_empty()
            || package
                .result()
                .id
                .as_str()
                .starts_with("link:")
        {
            return None;
        }
        match self.children_by_id.get(pkg_id) {
            Some(recorded) => recorded.edges
                .iter()
                .map(|edge| Arc::clone(&edge.pkg_id))
                .collect::<Vec<_>>()
                .pipe(Some),
            // A leaf never records children; anything else without a
            // record has not settled yet.
            None => package.is_leaf.then(Vec::new),
        }
    }
}

#[cfg(test)]
mod tests;
