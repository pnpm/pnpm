use super::{Diagnostic, Display, Error};

/// Errors specific to `pnpm unpublish`. Codes and messages match the
/// TypeScript CLI; the registry-communication errors are shared with
/// [`super::DeprecateError`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum UnpublishError {
    #[display("Package name is required")]
    #[diagnostic(code(ERR_PNPM_UNPUBLISH_REQUIRED))]
    PackageRequired,

    #[display(
        "Run pnpm unpublish --force to remove all published versions of {package_name} ({versions_list}) from the registry.\nThis is a protection mechanism to prevent accidental unpublish of packages with many versions.\nIf you want to unpublish a specific version, run pnpm unpublish {package_name}@<version>"
    )]
    #[diagnostic(code(ERR_PNPM_UNPUBLISH_CONFIRM))]
    ConfirmRequired {
        #[error(not(source))]
        package_name: String,
        #[error(not(source))]
        versions_list: String,
    },

    #[display(
        "This package cannot be completely unpublished. Deprecate it instead or contact npm support."
    )]
    #[diagnostic(code(ERR_PNPM_UNPUBLISH_FORBIDDEN))]
    CompletelyForbidden,
}
