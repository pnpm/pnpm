//! Fetchers for git-hosted dependencies, plus the `preparePackage`
//! port both fetchers delegate to.
//!
//! Two fetcher entry points, sharing `prepare_package` / `packlist` /
//! the CAS-import helpers:
//!
//! - [`GitFetcher`] handles `LockfileResolution::Git` — shells out to
//!   the system `git` binary.
//! - [`GitHostedTarballFetcher`] handles
//!   `TarballResolution { gitHosted: true }` — picks up a tarball the
//!   pacquet HTTP path has already downloaded into the CAS, materializes
//!   it into a temp dir, and runs the same `prepare_package` + packlist
//!   passes.
//!
//! `prepare_package` lives in this crate rather than a sibling because
//! both fetchers above are its only consumers.

mod cas_io;
mod error;
mod fetcher;
mod preferred_pm;
mod prepare_package;
mod tarball_fetcher;

pub use error::{GitFetcherError, PreparePackageError};
pub use fetcher::{
    CheckoutOptions, GitFetchOutput, GitFetcher, GitManifestQuery, checkout_commit,
    read_git_manifest,
};
pub use pacquet_fs_packlist::{PacklistError, packlist};
pub use preferred_pm::{PreferredPm, detect_preferred_pm};
pub use prepare_package::{PreparePackageOptions, PreparedPackage, prepare_package};
pub use tarball_fetcher::GitHostedTarballFetcher;
