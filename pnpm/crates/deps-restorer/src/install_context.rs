use crate::{AllowBuildPolicy, VirtualStoreLayout};
use pnpm_cmd_shim::LinkBinsOptions;
use pnpm_config::{Config, NodeLinker};
use std::{path::Path, sync::atomic::AtomicU8};

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
pub struct InstallContext<'a> {
    pub config: &'static Config,
    /// Install root — the directory containing `pnpm-lock.yaml`. For a
    /// real workspace this is the workspace root; for a single project,
    /// that project's directory.
    pub workspace_root: &'a Path,
    /// [`Self::workspace_root`] as the reporter spells it, for the
    /// `prefix` field of every emitted event.
    pub requester: &'a str,
    /// Install-scoped slot-directory mapping (GVS-aware). Every consumer
    /// that needs to know where a snapshot's slot is routes through it.
    pub layout: &'a VirtualStoreLayout,
    pub node_linker: NodeLinker,
    /// The `allowBuilds` gate, shared by the fetch phase's git fetcher
    /// and the build phase's lifecycle scripts.
    pub allow_build_policy: &'a AllowBuildPolicy,
    pub link_options: &'a LinkBinsOptions,
    /// Install-scoped dedupe state for `pnpm:package-import-method`.
    /// See `link_file::log_method_once`.
    pub logged_methods: &'a AtomicU8,
    /// Install-scoped [`pnpm_git_fetcher::GitSourceCache`] shared by the
    /// resolve-time manifest read and the fetch phase's git fetcher.
    pub git_source_cache: &'a pnpm_git_fetcher::GitSourceCache,
    pub dir_clone_cache: Option<&'a crate::DirCloneCache<'a>>,
}

impl InstallContext<'_> {
    /// Whether this install writes package directories into the project
    /// trees rather than into a virtual store.
    #[must_use]
    pub fn is_hoisted(&self) -> bool {
        matches!(self.node_linker, NodeLinker::Hoisted)
    }
}

#[cfg(test)]
mod tests;
