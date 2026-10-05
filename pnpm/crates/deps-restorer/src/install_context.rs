use crate::AllowBuildPolicy;
use pnpm_config::{Config, NodeLinker};
use pnpm_lockfile::{PackageKey, SnapshotEntry};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, OnceLock, atomic::AtomicU8},
};

/// What every phase of one install reads and none of them changes.
///
/// The install's driver owns the values; the fetch, the linker, the
/// builds, and the per-snapshot work each of them fans out to borrow
/// them through one `&InstallContext`. Adding an install-wide value
/// means one field here, not one on every phase.
///
/// Only values fixed for the whole install belong here. The skip set is
/// the near miss: phases mutate it between one another, so it stays a
/// parameter of its own.
#[derive(Clone)]
pub struct InstallContext<'a> {
    pub linker: crate::ModuleLinkerContext<'a>,
    pub caches: InstallCaches<'a>,
    pub config: &'static Config,
    /// Install root — the directory containing `pnpm-lock.yaml`. For a
    /// real workspace this is the workspace root; for a single project,
    /// that project's directory.
    pub workspace_root: &'a Path,
    /// [`Self::workspace_root`] as the reporter spells it, for the
    /// `prefix` field of every emitted event.
    pub requester: &'a str,
    /// The `allowBuilds` gate, shared by the fetch phase's git fetcher
    /// and the build phase's lifecycle scripts.
    pub allow_build_policy: &'a AllowBuildPolicy,
}

#[derive(Clone)]
pub struct InstallCaches<'a> {
    /// The immutable materialized selection shared by all phases and context clones.
    pub materialized_graph: Arc<OnceLock<HashMap<PackageKey, SnapshotEntry>>>,
    /// Install-scoped dedupe state for `pnpm:package-import-method`.
    /// See `link_file::log_method_once`.
    pub logged_methods: &'a AtomicU8,
    /// Install-scoped [`pnpm_git_fetcher::GitSourceCache`] shared by the
    /// resolve-time manifest read and the fetch phase's git fetcher.
    pub git_source_cache: &'a pnpm_git_fetcher::GitSourceCache,
    pub dir_clone_cache: Option<&'a crate::DirCloneCache<'a>>,
}

impl InstallContext<'_> {
    #[must_use]
    pub fn select_loaded_snapshots(
        &self,
        snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
    ) -> Option<&HashMap<PackageKey, SnapshotEntry>> {
        (self.config.node_linker == NodeLinker::Loaded).then(|| {
            self.caches.materialized_graph.get_or_init(|| {
                crate::create_virtual_store::cas::materialized_snapshots(self.config, snapshots)
            })
        })
    }

    /// Whether this install writes package directories into the project
    /// trees rather than into a virtual store.
    #[must_use]
    pub fn is_hoisted(&self) -> bool {
        matches!(self.linker.kind, NodeLinker::Hoisted)
    }
}

#[cfg(test)]
mod tests;
