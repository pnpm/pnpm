use derive_more::{Display, Error};
use pacquet_diagnostics::miette::{self, Diagnostic};
use std::path::PathBuf;

/// Error type of [`crate::DirectoryFetcher`].
///
/// Covers the failure modes of a directory fetch: directory-walk I/O,
/// manifest parse / read, and the `include_only_package_files` packlist
/// pass (which pacquet delegates to `pacquet_git_fetcher::packlist`).
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum DirectoryFetcherError {
    /// Failed to stat or read an entry under the source directory.
    /// Broken symlinks are *not* surfaced here — they degrade to a
    /// skipped entry inside the walker.
    #[display("I/O error while walking directory {dir:?}: {source}")]
    #[diagnostic(code(pacquet_directory_fetcher::io))]
    Io {
        dir: String,
        #[error(source)]
        source: std::io::Error,
    },

    #[display(
        "path {} resolves outside source directory {}",
        path.display(),
        directory.display()
    )]
    #[diagnostic(code(pacquet_directory_fetcher::path_escape))]
    PathOutsideDirectory { path: PathBuf, directory: PathBuf },

    #[diagnostic(transparent)]
    Packlist(#[error(source)] pacquet_git_fetcher::PacklistError),

    #[diagnostic(transparent)]
    ReadManifest(#[error(source)] pacquet_package_manifest::PackageManifestError),
}
