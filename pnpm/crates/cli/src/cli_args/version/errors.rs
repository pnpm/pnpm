use super::{Diagnostic, Display, Error};

/// Errors of `pnpm version`. Codes and messages match the TypeScript CLI.
#[derive(Debug, Display, Error, Diagnostic)]
pub(super) enum VersionError {
    #[display(
        "A version argument is required. Must be a valid semver version (e.g. 1.2.3) or one of: major, minor, patch, premajor, preminor, prepatch, prerelease, from-git"
    )]
    #[diagnostic(code(ERR_PNPM_INVALID_VERSION_BUMP))]
    MissingBump,

    #[display(
        "Invalid version argument: {raw}. Must be a valid semver version (e.g. 1.2.3) or one of: major, minor, patch, premajor, preminor, prepatch, prerelease, from-git"
    )]
    #[diagnostic(code(ERR_PNPM_INVALID_VERSION_BUMP))]
    InvalidBump { raw: String },

    #[display(
        "Could not determine a valid version from Git in {dir:?} using tag prefix {tag_version_prefix:?}: {reason}"
    )]
    #[diagnostic(code(ERR_PNPM_INVALID_VERSION_FROM_GIT))]
    InvalidVersionFromGit {
        dir: String,
        tag_version_prefix: String,
        reason: String,
    },

    #[display("Invalid version in {dir}: {version}")]
    #[diagnostic(code(ERR_PNPM_INVALID_VERSION))]
    InvalidVersion { dir: String, version: String },

    #[display("Version was not changed: {version}")]
    #[diagnostic(code(ERR_PNPM_VERSION_NOT_CHANGED))]
    VersionNotChanged { version: String },

    #[display("No packages to version")]
    #[diagnostic(code(ERR_PNPM_NO_PACKAGES_TO_VERSION))]
    NoPackagesToVersion,

    #[display("Cannot stage manifest outside of git cwd: {path}")]
    #[diagnostic(code(ERR_PNPM_INVALID_MANIFEST_PATH))]
    InvalidManifestPath { path: String },

    #[display("git {args} failed: {stderr}")]
    #[diagnostic(code(ERR_PNPM_GIT_COMMAND_FAILED))]
    GitCommandFailed { args: String, stderr: String },

    #[display(
        r#"The bare "pnpm version -r" form consumes change intents and is only supported in a workspace"#
    )]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_ONLY))]
    ReleaseOutsideWorkspace,

    #[display("Working tree is not clean. Commit or stash your changes.")]
    #[diagnostic(code(ERR_PNPM_UNCLEAN_WORKING_TREE))]
    UncleanWorkingTree,
}
