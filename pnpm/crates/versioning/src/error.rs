use std::path::PathBuf;

use derive_more::{Display, Error};
use miette::Diagnostic;

/// Errors of the versioning engine. Codes and messages match the TypeScript
/// implementation's `PnpmError`s — they are part of the shared CLI contract.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum VersioningError {
    #[display("Change intent file {} has no YAML frontmatter", file_path.display())]
    #[diagnostic(code(ERR_PNPM_INVALID_CHANGE_INTENT))]
    NoFrontmatter { file_path: PathBuf },

    #[display("Change intent file {} has invalid YAML frontmatter: {message}", file_path.display())]
    #[diagnostic(code(ERR_PNPM_INVALID_CHANGE_INTENT))]
    InvalidFrontmatter { file_path: PathBuf, message: String },

    #[display(
        "Change intent file {} declares an invalid bump type for {pkg_name}: {bump_type}. Expected one of none, patch, minor, major",
        file_path.display()
    )]
    #[diagnostic(code(ERR_PNPM_INVALID_CHANGE_INTENT))]
    InvalidBumpType { file_path: PathBuf, pkg_name: String, bump_type: String },

    #[display("Expected {} to be a mapping of package@version keys to intent id lists", ledger_path.display())]
    #[diagnostic(code(ERR_PNPM_INVALID_VERSIONING_LEDGER))]
    InvalidLedger { ledger_path: PathBuf },

    #[display("Change intent file {} names {pkg_name}, which is not a package in this workspace", file_path.display())]
    #[diagnostic(code(ERR_PNPM_VERSIONING_UNKNOWN_PACKAGE))]
    UnknownPackage { file_path: PathBuf, pkg_name: String },

    #[display(
        "Change intent file {} requests a {bump_type} release of {pkg_name}, which cannot release (it is listed in versioning.ignore, has no version field, or has a non-semver version). Remove the entry or change it to \"none\".",
        file_path.display()
    )]
    #[diagnostic(code(ERR_PNPM_VERSIONING_UNRELEASABLE_PACKAGE))]
    UnreleasablePackage { file_path: PathBuf, pkg_name: String, bump_type: String },

    #[display(
        "Package {pkg_name} declares the internal dependency {alias} in {field} as \"{spec}\". Internal dependencies must use the workspace: protocol so that dependency ranges never need rewriting at release time."
    )]
    #[diagnostic(code(ERR_PNPM_VERSIONING_INTERNAL_RANGE))]
    InternalRange { pkg_name: String, alias: String, field: String, spec: String },

    #[display(
        "versioning.lanes assigns {pkg_name} to the \"{lane}\" lane, but \"main\" is the reserved default lane. Remove the entry instead."
    )]
    #[diagnostic(code(ERR_PNPM_VERSIONING_INVALID_LANE_NAME))]
    InvalidLaneName { pkg_name: String, lane: String },

    #[display(
        "The fixed group [{}] mixes packages on different lanes. A fixed group must move between lanes together.",
        group.join(", ")
    )]
    #[diagnostic(code(ERR_PNPM_VERSIONING_CONFLICTING_CONFIG))]
    ConflictingConfig { group: Vec<String> },

    #[display(
        "The release plan bumps {pkg_name} by {bump_type}, but versioning.maxBump caps releases from this branch at {max_bump}. Raised by {raised_by}."
    )]
    #[diagnostic(code(ERR_PNPM_VERSIONING_MAX_BUMP_EXCEEDED))]
    MaxBumpExceeded { pkg_name: String, bump_type: String, max_bump: String, raised_by: String },

    #[display(
        "versioning.changelog.storage \"{storage}\" is not implemented yet. Use \"repository\"."
    )]
    #[diagnostic(code(ERR_PNPM_VERSIONING_UNSUPPORTED_CHANGELOG_STORAGE))]
    UnsupportedChangelogStorage { storage: String },

    #[display("Failed to read {}: {source}", path.display())]
    #[diagnostic(code(pacquet_versioning::read_error))]
    Read { path: PathBuf, source: std::io::Error },

    #[display("Failed to write {}: {source}", path.display())]
    #[diagnostic(code(pacquet_versioning::write_error))]
    Write { path: PathBuf, source: std::io::Error },

    #[display("{_0}")]
    #[diagnostic(transparent)]
    Manifest(pacquet_package_manifest::PackageManifestError),
}
