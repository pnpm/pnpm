//! The `--workspace` flag shared by `pnpm add` and `pnpm update`.

use derive_more::{Display, Error};
use miette::Diagnostic;
use std::path::Path;

#[derive(Debug, Display, Error, Diagnostic)]
pub(crate) enum WorkspaceOptionError {
    #[display("Cannot use --latest with --workspace simultaneously")]
    #[diagnostic(code(ERR_PNPM_BAD_OPTIONS))]
    LatestWithWorkspace,

    #[display("--workspace can only be used inside a workspace")]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_OPTION_OUTSIDE_WORKSPACE))]
    OutsideWorkspace,
}

/// The workspace root `--workspace` links from; `Ok(None)` when the flag
/// was not passed.
pub(crate) fn workspace_link_root(
    requested: bool,
    workspace_root: Option<&Path>,
) -> miette::Result<Option<&Path>> {
    if !requested {
        return Ok(None);
    }
    workspace_root.ok_or_else(|| WorkspaceOptionError::OutsideWorkspace.into()).map(Some)
}
