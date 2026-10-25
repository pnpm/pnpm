//! Which `pacquet update` scope each importer is judged against, and the
//! [`TargetDistances`] of that scope.

use super::{Arc, UpdateReuseScope, WorkspaceTreeCtx, lock_recoverable};
use crate::resolve_dependency_tree::reuse::TargetDistances;

impl WorkspaceTreeCtx {
    /// Which update scope `importer_id` is judged against: `Some(importer_id)`
    /// for an importer with a scope of its own, `None` for the
    /// workspace-wide scope. A workspace-wide update of everything
    /// ([`UpdateReuseScope::None`]) overrides every importer's own scope.
    fn update_scope_owner<'a>(&self, importer_id: &'a str) -> Option<&'a str> {
        if matches!(self.reuse.scope, UpdateReuseScope::None) {
            return None;
        }
        self.reuse.scopes_by_importer.contains_key(importer_id).then_some(importer_id)
    }

    pub(in super::super) fn update_reuse_scope_for(&self, importer_id: &str) -> &UpdateReuseScope {
        self.update_scope_owner(importer_id)
            .and_then(|importer_id| self.reuse.scopes_by_importer.get(importer_id))
            .unwrap_or(&self.reuse.scope)
    }

    /// The [`TargetDistances`] of the update scope `importer_id` is judged
    /// against, computed on first use.
    pub(in super::super) fn target_distances_for(&self, importer_id: &str) -> Arc<TargetDistances> {
        let owner = self.update_scope_owner(importer_id).map(str::to_owned);
        let mut cache = lock_recoverable(&self.cache.subtree.target_distances);
        let distances = cache
            .entry(owner)
            .or_insert_with(|| {
                let scope = self.update_reuse_scope_for(importer_id);
                Arc::new(TargetDistances::compute(self.reuse.lockfile.as_deref(), scope))
            });
        Arc::clone(distances)
    }
}
