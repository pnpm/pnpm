use derive_more::{Display, Error};
use pnpm_diagnostics::miette::{self, Diagnostic};

/// Error type of [`crate::prepare_package()`].
///
/// The error codes (`ERR_PNPM_GIT_DEP_PREPARE_NOT_ALLOWED`,
/// `ERR_PNPM_PREPARE_PACKAGE`, `ERR_PNPM_INVALID_PATH`) match pnpm's, so
/// `pnpm.io/errors/<code>` URL resolution keeps working.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum PreparePackageError {
    /// Package wants to run build scripts but is not in `allowBuilds`.
    #[display(
        "The git-hosted package \"{name}@{version}\" needs to execute build scripts but is not in the \"allowBuilds\" allowlist."
    )]
    #[diagnostic(
        code(ERR_PNPM_GIT_DEP_PREPARE_NOT_ALLOWED),
        help(
            "Add the package to \"allowBuilds\" in your project's pnpm-workspace.yaml to allow it to run scripts. For example:\nallowBuilds:\n  {dep_path}: true",
        )
    )]
    NotAllowed {
        name: String,
        version: String,
        /// The identity `allowBuilds` is checked against — the name
        /// plus the resolution id, not the manifest version. A
        /// name-only key cannot approve a git artifact, so quoting
        /// anything else here suggests an entry that never matches.
        ///
        /// Redacted: a resolution id can embed `user:pass@`
        /// credentials, which must not reach a terminal or a CI log.
        /// A specifier that carries them therefore needs its own copy
        /// of the key rather than this example verbatim.
        dep_path: String,
    },

    /// A lifecycle script invoked by `preparePackage` failed, stamped
    /// with the `ERR_PNPM_PREPARE_PACKAGE` code.
    #[display("Failed to prepare package: {source}")]
    #[diagnostic(code(ERR_PNPM_PREPARE_PACKAGE))]
    LifecycleFailed {
        #[error(source)]
        source: pnpm_executor::LifecycleScriptError,
    },

    /// `path` field on the resolution pointed outside the cloned dir
    /// or to a non-directory, rejected with the `ERR_PNPM_INVALID_PATH` code.
    #[display("Path {path:?} is not a valid sub-directory of the git checkout")]
    #[diagnostic(code(ERR_PNPM_INVALID_PATH))]
    InvalidPath { path: String },

    #[diagnostic(transparent)]
    ReadManifest(#[error(source)] pnpm_package_manifest::PackageManifestError),

    /// The dependency pins the package manager that prepares it, and pnpm
    /// could not put that package manager on the build's `PATH`.
    #[display("Cannot provide {package_manager} to prepare the git-hosted package: {source}")]
    #[diagnostic(code(ERR_PNPM_GIT_DEP_PACKAGE_MANAGER_UNAVAILABLE))]
    PackageManagerUnavailable {
        package_manager: String,
        #[error(source)]
        source: std::io::Error,
    },

    #[display("I/O error during preparePackage: {_0}")]
    #[diagnostic(code(ERR_PNPM_GIT_FETCHER_PREPARE_PACKAGE_IO))]
    Io(#[error(source)] std::io::Error),
}

/// Error type of the git fetcher itself.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum GitFetcherError {
    /// `git` executable not found on `PATH`. Pacquet, like pnpm, does
    /// not bundle git — the user must install it themselves.
    #[display("`git` executable not found on PATH. Install git to fetch git-hosted packages.")]
    #[diagnostic(code(ERR_PNPM_GIT_FETCHER_GIT_NOT_FOUND))]
    GitNotFound,

    /// `git` exited non-zero on `clone` / `fetch` / `checkout` /
    /// `rev-parse`. `operation` is the subcommand, `stderr` is captured
    /// from the child so the failure surfaces in the install log.
    #[display("`git {operation}` failed ({status}): {stderr}")]
    #[diagnostic(code(ERR_PNPM_GIT_FETCHER_GIT_EXEC_FAILED))]
    GitExec { operation: &'static str, stderr: String, status: std::process::ExitStatus },

    /// The clone (or shallow fetch) of a git dependency failed. Carries
    /// the package the resolution belongs to, which [`Self::GitExec`]
    /// alone cannot name — a bare `git clone` failure leaves the user to
    /// work out which of their dependencies it came from.
    #[display("Failed to fetch {package:?} from the git repository {repo:?}: {stderr}")]
    #[diagnostic(code(ERR_PNPM_GIT_FETCH_FAILED))]
    Fetch { package: String, repo: String, stderr: String },

    /// [`Self::Fetch`] for a repository the lockfile pins to an SSH
    /// remote. Split into its own variant purely so the derived
    /// `Diagnostic` can carry the remediation `help`, which does not
    /// apply to a transport that needs no key.
    ///
    /// A lockfile written before pnpm v11.21 could record an SSH URL for
    /// a dependency whose specifier never asked for SSH, and resolution
    /// is skipped while that lockfile stays up to date — so the entry
    /// survives the upgrade that fixed it and the install keeps failing
    /// wherever no SSH key is configured.
    #[display("Failed to fetch {package:?} from the git repository {repo:?}: {stderr}")]
    #[diagnostic(
        code(ERR_PNPM_GIT_FETCH_FAILED),
        help(
            r#"The lockfile records an SSH remote for this dependency, so fetching it needs an SSH key for {host}.

If its specifier does not ask for SSH (for example "github:owner/repo"), the lockfile entry was written before pnpm v11.21 and can be re-recorded over HTTPS:

    pnpm update {package}

"pnpm install --force" and "pnpm install --resolution-only" do not re-resolve git dependencies, so neither clears it."#,
        )
    )]
    FetchOverSsh { package: String, repo: String, host: String, stderr: String },

    /// `git rev-parse HEAD` did not return the pinned commit, rejected
    /// with the `ERR_PNPM_GIT_CHECKOUT_FAILED` code.
    #[display("received commit {received} does not match expected value {expected}")]
    #[diagnostic(code(ERR_PNPM_GIT_CHECKOUT_FAILED))]
    CheckoutMismatch { expected: String, received: String },

    /// `resolution.commit` is not a 40-character hexadecimal SHA. A
    /// commit value beginning with `-` would otherwise be parsed by
    /// `git fetch` / `git checkout` as an option (e.g. `--upload-pack`),
    /// allowing a malicious lockfile to execute arbitrary commands on
    /// SSH or local-file transports.
    #[display(
        "Invalid git commit hash {commit:?} for repository {repo:?}. Expected a 40-character hexadecimal SHA."
    )]
    #[diagnostic(code(ERR_PNPM_INVALID_GIT_COMMIT))]
    InvalidCommit { commit: String, repo: String },

    /// `resolution.repo` begins with `-`. Same class as
    /// [`Self::InvalidCommit`]: git parses such a value as an option
    /// rather than a repository, so `--upload-pack=<cmd>` reaches the
    /// transport and runs `<cmd>`. The `--` end-of-options marker is
    /// passed as well; this rejects the value outright rather than rely
    /// on every subcommand honoring it.
    #[display("Invalid git repository {repo:?}. A repository must not begin with '-'.")]
    #[diagnostic(code(INVALID_GIT_REPOSITORY))]
    InvalidRepo { repo: String },

    #[display("I/O error during git fetch: {_0}")]
    #[diagnostic(code(ERR_PNPM_GIT_FETCHER_IO))]
    Io(#[error(source)] std::io::Error),

    #[diagnostic(transparent)]
    ReadManifest(#[error(source)] pnpm_package_manifest::PackageManifestError),

    #[diagnostic(transparent)]
    Prepare(#[error(source)] PreparePackageError),

    #[diagnostic(transparent)]
    Packlist(#[error(source)] pnpm_fs_packlist::PacklistError),

    #[diagnostic(transparent)]
    AddFilesFromDir(#[error(source)] pnpm_store_dir::AddFilesFromDirError),
}
