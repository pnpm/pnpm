//! The `--workspace` flag `pnpm add` and `pnpm update` share: both link
//! the named dependencies to their workspace copies, so both reject the
//! same invocations.

use derive_more::{Display, Error};
use miette::Diagnostic;
use std::path::Path;

/// The invocations `--workspace` rejects, checked before any resolution
/// happens on every dispatch path — plain, selected, and global (whose
/// global directory is never a workspace).
#[derive(Debug, Display, Error, Diagnostic)]
pub(crate) enum WorkspaceOptionError {
    #[display("Cannot use --latest with --workspace simultaneously")]
    #[diagnostic(code(ERR_PNPM_BAD_OPTIONS))]
    LatestWithWorkspace,

    #[display("--workspace can only be used inside a workspace")]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_OPTION_OUTSIDE_WORKSPACE))]
    OutsideWorkspace,
}

/// The workspace root a `--workspace` run reads its link targets from.
/// `Ok(None)` means the flag was not passed.
pub(crate) fn workspace_link_root(
    requested: bool,
    workspace_root: Option<&Path>,
) -> miette::Result<Option<&Path>> {
    if !requested {
        return Ok(None);
    }
    workspace_root.ok_or_else(|| WorkspaceOptionError::OutsideWorkspace.into()).map(Some)
}
