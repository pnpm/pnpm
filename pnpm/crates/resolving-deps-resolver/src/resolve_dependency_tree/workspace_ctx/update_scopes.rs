//! Which `pacquet update` scope each importer is judged against, and the
//! [`TargetDistances`] of that scope.

use super::{Arc, UpdateReuseScope, WorkspaceTreeCtx, lock_recoverable};
use crate::resolve_dependency_tree::reuse::TargetDistances;

impl WorkspaceTreeCtx {
    pub(in super::super) fn update_reuse_scope_for(&self, importer_id: &str) -> &UpdateReuseScope {
        if matches!(self.reuse.scope, UpdateReuseScope::None) {
            return &self.reuse.scope;
        }
        self.reuse.scopes_by_importer.get(importer_id).unwrap_or(&self.reuse.scope)
    }

    /// The [`TargetDistances`] of the update scope `importer_id` is judged
    /// against, computed on first use and shared by every importer whose
    /// scope names the same targets.
    pub(in super::super) fn target_distances_for(&self, importer_id: &str) -> Arc<TargetDistances> {
        let scope = self.update_reuse_scope_for(importer_id);
        let mut cache = lock_recoverable(&self.cache.subtree.target_distances);
        if let Some(distances) = cache.get(scope) {
            return Arc::clone(distances);
        }
        let distances = Arc::new(TargetDistances::compute(self.reuse.lockfile.as_deref(), scope));
        cache.insert(scope.clone(), Arc::clone(&distances));
        distances
    }
}
