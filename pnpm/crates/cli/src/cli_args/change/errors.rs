use super::{Diagnostic, Display, Error};

/// Errors of `pnpm change`. Codes and messages match the TypeScript CLI.
#[derive(Debug, Display, Error, Diagnostic)]
pub(super) enum ChangeError {
    #[display("pnpm change is only supported in a workspace")]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_ONLY))]
    WorkspaceOnly,

    #[display("No releasable packages found in this workspace")]
    #[diagnostic(code(ERR_PNPM_VERSIONING_NO_PACKAGES))]
    NoPackages,

    #[display("{pkg_name} is not a releasable package of this workspace")]
    #[diagnostic(code(ERR_PNPM_VERSIONING_UNKNOWN_PACKAGE))]
    UnknownPackage { pkg_name: String },

    #[display(
        "{reference} matches multiple workspace projects: {}. Reference the project by directory instead.",
        dirs.join(", ")
    )]
    #[diagnostic(code(ERR_PNPM_VERSIONING_AMBIGUOUS_PACKAGE))]
    AmbiguousPackage {
        reference: String,
        dirs: Vec<String>,
    },

    #[display("Invalid bump type: {bump}. Expected one of none, patch, minor, major")]
    #[diagnostic(code(ERR_PNPM_VERSIONING_INVALID_BUMP))]
    InvalidBump { bump: String },
}
