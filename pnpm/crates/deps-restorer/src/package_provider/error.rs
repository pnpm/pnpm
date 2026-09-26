use derive_more::{Display, Error};
use miette::Diagnostic;
use std::{io, path::PathBuf};

/// Error type of [`crate::materialize_through_package_provider`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum PackageProviderError {
    #[display(
        "The package provider cannot install {dep_path}: git dependencies that need to be built (prepare) are not supported yet"
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_PROVIDER_UNSUPPORTED))]
    GitPrepareUnsupported {
        #[error(not(source))]
        dep_path: String,
    },

    #[display("The package provider does not support the resolution of {dep_path} ({kind})")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_PROVIDER_UNSUPPORTED))]
    UnsupportedResolution {
        #[error(not(source))]
        dep_path: String,
        kind: &'static str,
    },

    #[display(
        "The package provider needs the patch file of {dep_path}, but only its hash is known"
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_PROVIDER_UNSUPPORTED))]
    PatchWithoutFile {
        #[error(not(source))]
        dep_path: String,
    },

    #[display(
        "The package provider cannot install {dep_path}, which depends on a different version of itself"
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_PROVIDER_UNSUPPORTED))]
    SelfDependency {
        #[error(not(source))]
        dep_path: String,
    },

    #[display(
        "The package provider cannot install {dep_path}: no matching entry in the lockfile packages section"
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_PROVIDER_UNSUPPORTED))]
    MissingPackageMetadata {
        #[error(not(source))]
        dep_path: String,
    },

    #[display("Cannot run the package provider at \"{provider}\": {source}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_PROVIDER_FAILED))]
    Spawn {
        provider: String,
        #[error(source)]
        source: io::Error,
    },

    #[display("The package provider at \"{provider}\" exited with code {code}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_PROVIDER_FAILED))]
    NonZeroExit {
        #[error(not(source))]
        provider: String,
        code: String,
    },

    #[display("The package provider at \"{provider}\" did not return valid JSON")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_PROVIDER_RESULT_INVALID))]
    InvalidJson {
        #[error(not(source))]
        provider: String,
    },

    #[display(
        "The package provider at \"{provider}\" returned an unsupported response (protocol {protocol})"
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_PROVIDER_RESULT_INVALID))]
    UnsupportedResponse {
        #[error(not(source))]
        provider: String,
        protocol: String,
    },

    #[display("The package provider skipped {dep_path}, which is not an optional dependency")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_PROVIDER_RESULT_INVALID))]
    SkippedNonOptional {
        #[error(not(source))]
        dep_path: String,
    },

    #[display("The package provider returned no path for {dep_path}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_PROVIDER_RESULT_INVALID))]
    MissingPath {
        #[error(not(source))]
        dep_path: String,
    },

    #[display("The package provider returned a relative path for {dep_path}: {dir}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_PROVIDER_RESULT_INVALID))]
    RelativePath {
        #[error(not(source))]
        dep_path: String,
        dir: String,
    },

    #[display("Failed to read the patch file of {dep_path} at {}: {source}", path.display())]
    #[diagnostic(code(pnpm_package_manager::package_provider_read_patch_file))]
    ReadPatchFile {
        dep_path: String,
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed to serialize the package provider request: {_0}")]
    #[diagnostic(code(pnpm_package_manager::package_provider_serialize_request))]
    SerializeRequest(#[error(source)] serde_json::Error),
}
