use super::{Diagnostic, Display, Error, STAGE_SUBCOMMANDS};

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum StageError {
    #[display("Stage subcommand is required")]
    #[diagnostic(
        code(ERR_PNPM_STAGE_SUBCOMMAND_REQUIRED),
        help("Use one of: {STAGE_SUBCOMMANDS}")
    )]
    SubcommandRequired,

    #[display(r#"Unknown stage subcommand "{subcommand}""#)]
    #[diagnostic(
        code(ERR_PNPM_STAGE_UNKNOWN_SUBCOMMAND),
        help("Use one of: {STAGE_SUBCOMMANDS}")
    )]
    UnknownSubcommand {
        #[error(not(source))]
        subcommand: String,
    },

    #[display(r#"Missing required <stage-id> for "pnpm stage {subcommand}""#)]
    #[diagnostic(code(ERR_PNPM_STAGE_ID_REQUIRED))]
    StageIdRequired {
        #[error(not(source))]
        subcommand: &'static str,
    },

    #[display("stage-id must be a valid UUID")]
    #[diagnostic(code(ERR_PNPM_INVALID_STAGE_ID))]
    InvalidStageId,

    #[display("Invalid package spec: {spec}")]
    #[diagnostic(code(ERR_PNPM_INVALID_PACKAGE_SPEC))]
    InvalidPackageSpec {
        #[error(not(source))]
        spec: String,
    },

    #[display("Version specifiers are not supported for listing staged packages")]
    #[diagnostic(code(ERR_PNPM_STAGE_VERSION_SPECIFIER_UNSUPPORTED))]
    VersionSpecifierUnsupported,

    #[display("Failed to {operation}: {reason}")]
    #[diagnostic(code(ERR_PNPM_STAGE_REGISTRY_ERROR))]
    RequestFailed {
        #[error(not(source))]
        operation: String,
        #[error(not(source))]
        reason: String,
    },

    #[display("Could not read package.json from tarball")]
    #[diagnostic(code(ERR_PNPM_STAGE_TARBALL_MANIFEST_NOT_FOUND))]
    TarballManifestNotFound,

    #[display(
        "Cannot approve stages {first_stage_id} and {second_stage_id} together because both publish {package_name}@{version}"
    )]
    #[diagnostic(code(ERR_PNPM_STAGE_DUPLICATE_PACKAGE))]
    DuplicateStagePackage {
        #[error(not(source))]
        first_stage_id: String,
        #[error(not(source))]
        second_stage_id: String,
        #[error(not(source))]
        package_name: String,
        #[error(not(source))]
        version: String,
    },

    #[display(r#"Invalid package name "{name}"."#)]
    #[diagnostic(code(ERR_PNPM_INVALID_PACKAGE_NAME))]
    InvalidPackageName {
        #[error(not(source))]
        name: String,
    },

    #[display(r#"Invalid package version "{version}"."#)]
    #[diagnostic(code(ERR_PNPM_INVALID_PACKAGE_VERSION))]
    InvalidPackageVersion {
        #[error(not(source))]
        version: String,
    },

    #[display(r#"Invalid tarball filename "{filename}"."#)]
    #[diagnostic(code(ERR_PNPM_INVALID_TARBALL_FILENAME))]
    InvalidTarballFilename {
        #[error(not(source))]
        filename: String,
    },
}
