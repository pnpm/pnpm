use derive_more::{Display, Error};
use pacquet_diagnostics::miette::{self, Diagnostic};

/// Error type of [`crate::prepare_package()`].
///
/// Mirrors the upstream error codes thrown by
/// [`exec/prepare-package`](https://github.com/pnpm/pnpm/blob/94240bc046/exec/prepare-package/src/index.ts).
/// Codes that match upstream byte-for-byte (`GIT_DEP_PREPARE_NOT_ALLOWED`,
/// `ERR_PNPM_PREPARE_PACKAGE`, `INVALID_PATH`) keep `pnpm.io/errors/<code>`
/// URL resolution working.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum PreparePackageError {
    /// Package wants to run build scripts but is not in `allowBuilds`.
    /// Mirrors [`exec/prepare-package/src/index.ts:37-46`](https://github.com/pnpm/pnpm/blob/94240bc046/exec/prepare-package/src/index.ts#L37-L46).
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

    /// A lifecycle script invoked by `preparePackage` failed. Mirrors
    /// upstream's `ERR_PNPM_PREPARE_PACKAGE` stamp at
    /// [`exec/prepare-package/src/index.ts:71-77`](https://github.com/pnpm/pnpm/blob/94240bc046/exec/prepare-package/src/index.ts#L71-L77).
    #[display("Failed to prepare package: {source}")]
    #[diagnostic(code(ERR_PNPM_PREPARE_PACKAGE))]
    LifecycleFailed {
        #[error(source)]
        source: pacquet_executor::LifecycleScriptError,
    },

    /// `path` field on the resolution pointed outside the cloned dir
    /// or to a non-directory. Mirrors `safeJoinPath`'s `INVALID_PATH`
    /// at [`exec/prepare-package/src/index.ts:92-103`](https://github.com/pnpm/pnpm/blob/94240bc046/exec/prepare-package/src/index.ts#L92-L103).
    #[display("Path {path:?} is not a valid sub-directory of the git checkout")]
    #[diagnostic(code(INVALID_PATH))]
    InvalidPath { path: String },

    #[diagnostic(transparent)]
    ReadManifest(#[error(source)] pacquet_package_manifest::PackageManifestError),

    #[display("I/O error during preparePackage: {_0}")]
    #[diagnostic(code(pacquet_git_fetcher::prepare_package::io))]
    Io(#[error(source)] std::io::Error),
}

/// Error type of [`crate::packlist()`]. Surfaces the subset of npm-packlist
/// failures the MVP scope can produce.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum PacklistError {
    #[display("I/O error while computing packlist for {pkg_dir}: {source}")]
    #[diagnostic(code(pacquet_git_fetcher::packlist::io))]
    Io {
        pkg_dir: String,
        #[error(source)]
        source: std::io::Error,
    },
}

/// Error type of the git fetcher itself. Reserved for the next patch in
/// this PR — defined now so `lib.rs` can re-export the full error surface
/// without churn when the fetcher module lands.
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

    /// `git rev-parse HEAD` did not return the pinned commit. Mirrors
    /// upstream's `GIT_CHECKOUT_FAILED` at
    /// [`fetching/git-fetcher/src/index.ts:39-41`](https://github.com/pnpm/pnpm/blob/94240bc046/fetching/git-fetcher/src/index.ts#L39-L41).
    #[display("received commit {received} does not match expected value {expected}")]
    #[diagnostic(code(GIT_CHECKOUT_FAILED))]
    CheckoutMismatch { expected: String, received: String },

    #[display("I/O error during git fetch: {_0}")]
    #[diagnostic(code(pacquet_git_fetcher::io))]
    Io(#[error(source)] std::io::Error),

    #[diagnostic(transparent)]
    Prepare(#[error(source)] PreparePackageError),

    #[diagnostic(transparent)]
    Packlist(#[error(source)] PacklistError),

    #[diagnostic(transparent)]
    AddFilesFromDir(#[error(source)] pacquet_store_dir::AddFilesFromDirError),
}
