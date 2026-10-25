use super::{ConfigOverrides, LinkWorkspacePackages};
use crate::cli_args::lockfile_dir::LockfileDirWithGlobal;
use derive_more::{Display, Error};
use miette::Diagnostic;

/// A command-line setting that a `--global` command rejects. Mirrors
/// pnpm's `CONFIG_CONFLICT_*_WITH_GLOBAL` errors, which only fire for a
/// value given on the command line.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum GlobalConflict {
    #[display(r#"Configuration conflict. "hoist-pattern" may not be used with "global""#)]
    #[diagnostic(code(ERR_PNPM_CONFIG_CONFLICT_HOIST_PATTERN_WITH_GLOBAL))]
    HoistPattern,

    #[display(r#"Configuration conflict. "link-workspace-packages" may not be used with "global""#)]
    #[diagnostic(code(ERR_PNPM_CONFIG_CONFLICT_LINK_WORKSPACE_PACKAGES_WITH_GLOBAL))]
    LinkWorkspacePackages,

    #[display(
        r#"Configuration conflict. "shared-workspace-lockfile" may not be used with "global""#
    )]
    #[diagnostic(code(ERR_PNPM_CONFIG_CONFLICT_SHARED_WORKSPACE_LOCKFILE_WITH_GLOBAL))]
    SharedWorkspaceLockfile,

    #[diagnostic(transparent)]
    LockfileDir(#[error(source)] LockfileDirWithGlobal),

    #[display(r#"Configuration conflict. "virtual-store-dir" may not be used with "global""#)]
    #[diagnostic(code(ERR_PNPM_CONFIG_CONFLICT_VIRTUAL_STORE_DIR_WITH_GLOBAL))]
    VirtualStoreDir,
}

impl ConfigOverrides {
    /// Reject the settings a `--global` command cannot honour, checked in
    /// pnpm's order. The default hoist pattern, `*`, is accepted, and so
    /// is a disabled or empty value of the other settings.
    pub(crate) fn check_global_conflicts(&self) -> Result<(), GlobalConflict> {
        if self.hoist_pattern
            .as_deref()
            .is_some_and(|pattern| pattern != ["*"])
        {
            return Err(GlobalConflict::HoistPattern);
        }
        if self.link_workspace_packages.is_some_and(|value| value != LinkWorkspacePackages::Off) {
            return Err(GlobalConflict::LinkWorkspacePackages);
        }
        if self.shared_workspace_lockfile == Some(true) {
            return Err(GlobalConflict::SharedWorkspaceLockfile);
        }
        if self.unported
            .get("lockfile-dir")
            .is_some_and(|dir| !dir.is_empty())
        {
            return Err(GlobalConflict::LockfileDir(LockfileDirWithGlobal));
        }
        if self.virtual_store_dir
            .as_deref()
            .is_some_and(|dir| !dir.is_empty())
        {
            return Err(GlobalConflict::VirtualStoreDir);
        }
        Ok(())
    }
}
