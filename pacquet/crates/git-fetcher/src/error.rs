use derive_more::{Display, Error};
use pacquet_diagnostics::miette::{self, Diagnostic};

/// Error type of [`crate::prepare_package()`].
///
/// The error codes (`GIT_DEP_PREPARE_NOT_ALLOWED`,
/// `ERR_PNPM_PREPARE_PACKAGE`, `INVALID_PATH`) match pnpm's, so
/// `pnpm.io/errors/<code>` URL resolution keeps working.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum PreparePackageError {
    /// Package wants to run build scripts but is not in `allowBuilds`.
    #[display(
        "The git-hosted package \"{name}@{version}\" needs to execute build scripts but is not in the \"allowBuilds\" allowlist."
    )]
    #[diagnostic(
        code(GIT_DEP_PREPARE_NOT_ALLOWED),
        help(
            "Add the package to \"allowBuilds\" in your project's pnpm-workspace.yaml to allow it to run scripts. For example:\nallowBuilds:\n  {name}: true",
        )
    )]
    NotAllowed { name: String, version: String },

    /// A lifecycle script invoked by `preparePackage` failed, stamped
    /// with the `ERR_PNPM_PREPARE_PACKAGE` code.
    #[display("Failed to prepare package: {source}")]
    #[diagnostic(code(ERR_PNPM_PREPARE_PACKAGE))]
    LifecycleFailed {
        #[error(source)]
        source: pacquet_executor::LifecycleScriptError,
    },

    /// `path` field on the resolution pointed outside the cloned dir
    /// or to a non-directory, rejected with the `INVALID_PATH` code.
    #[display("Path {path:?} is not a valid sub-directory of the git checkout")]
    #[diagnostic(code(INVALID_PATH))]
    InvalidPath { path: String },

    #[diagnostic(transparent)]
    ReadManifest(#[error(source)] pacquet_package_manifest::PackageManifestError),

    #[display("I/O error during preparePackage: {_0}")]
    #[diagnostic(code(pacquet_git_fetcher::prepare_package::io))]
    Io(#[error(source)] std::io::Error),
}

/// Error type of the git fetcher itself.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum GitFetcherError {
    /// `git` executable not found on `PATH`. Pacquet, like pnpm, does
    /// not bundle git — the user must install it themselves.
    #[display("`git` executable not found on PATH. Install git to fetch git-hosted packages.")]
    #[diagnostic(code(pacquet_git_fetcher::git_not_found))]
    GitNotFound,

    /// `git` exited non-zero on `clone` / `fetch` / `checkout` /
    /// `rev-parse`. `operation` is the subcommand, `stderr` is captured
    /// from the child so the failure surfaces in the install log.
    #[display("`git {operation}` failed ({status}): {stderr}")]
    #[diagnostic(code(pacquet_git_fetcher::git_exec_failed))]
    GitExec { operation: &'static str, stderr: String, status: std::process::ExitStatus },

    /// `git rev-parse HEAD` did not return the pinned commit, rejected
    /// with the `GIT_CHECKOUT_FAILED` code.
    #[display("received commit {received} does not match expected value {expected}")]
    #[diagnostic(code(GIT_CHECKOUT_FAILED))]
    CheckoutMismatch { expected: String, received: String },

    /// `resolution.commit` is not a 40-character hexadecimal SHA. A
    /// commit value beginning with `-` would otherwise be parsed by
    /// `git fetch` / `git checkout` as an option (e.g. `--upload-pack`),
    /// allowing a malicious lockfile to execute arbitrary commands on
    /// SSH or local-file transports.
    #[display(
        "Invalid git commit hash {commit:?} for repository {repo:?}. Expected a 40-character hexadecimal SHA."
    )]
    #[diagnostic(code(INVALID_GIT_COMMIT))]
    InvalidCommit { commit: String, repo: String },

    #[display("I/O error during git fetch: {_0}")]
    #[diagnostic(code(pacquet_git_fetcher::io))]
    Io(#[error(source)] std::io::Error),

    #[diagnostic(transparent)]
    Prepare(#[error(source)] PreparePackageError),

    #[diagnostic(transparent)]
    Packlist(#[error(source)] pacquet_fs_packlist::PacklistError),

    #[diagnostic(transparent)]
    AddFilesFromDir(#[error(source)] pacquet_store_dir::AddFilesFromDirError),
}
