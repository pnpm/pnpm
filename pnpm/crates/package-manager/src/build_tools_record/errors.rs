use derive_more::{Display, Error};
use miette::Diagnostic;

/// Why the build tools of git-hosted packages could not be recorded.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum RecordBuildToolsError {
    #[diagnostic(transparent)]
    ReadGitPackage(#[error(source)] pnpm_git_fetcher::GitFetcherError),

    #[diagnostic(transparent)]
    ReadArchive(#[error(source)] pnpm_tarball::TarballError),

    #[display("Failed to read the manifest of {package} from the store: {source}")]
    #[diagnostic(code(ERR_PNPM_BUILD_TOOLS_READ_MANIFEST))]
    ReadArchiveFile {
        package: String,
        #[error(source)]
        source: std::io::Error,
    },

    #[display("Failed to read the manifest of {package}: {source}")]
    #[diagnostic(code(ERR_PNPM_BUILD_TOOLS_READ_MANIFEST))]
    ReadManifest {
        package: String,
        #[error(source)]
        source: serde_json::Error,
    },

    #[display("Failed to resolve {tool}, which builds a git-hosted dependency: {reason}")]
    #[diagnostic(code(ERR_PNPM_BUILD_TOOL_RESOLUTION))]
    Resolve { tool: String, reason: String },

    #[display(
        "No version of {tool} matches, so the git-hosted dependency that asks for it cannot be built"
    )]
    #[diagnostic(code(ERR_PNPM_BUILD_TOOL_RESOLUTION))]
    NoMatch { tool: String },
}
