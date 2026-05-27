//! Fetchers for git-hosted dependencies, plus the `preparePackage`
//! port both fetchers delegate to.
//!
//! Two fetcher entry points, sharing `prepare_package` / `packlist` /
//! the CAS-import helpers:
//!
//! - [`GitFetcher`] handles `LockfileResolution::Git` — shells out to
//!   the system `git` binary. Ports pnpm's
//!   [`fetching/git-fetcher`](https://github.com/pnpm/pnpm/blob/94240bc046/fetching/git-fetcher/src/index.ts).
//! - [`GitHostedTarballFetcher`] handles
//!   `TarballResolution { gitHosted: true }` — picks up a tarball the
//!   pacquet HTTP path has already downloaded into the CAS, materializes
//!   it into a temp dir, and runs the same `prepare_package` + packlist
//!   passes. Ports pnpm's
//!   [`fetching/tarball-fetcher/src/gitHostedTarballFetcher.ts`](https://github.com/pnpm/pnpm/blob/94240bc046/fetching/tarball-fetcher/src/gitHostedTarballFetcher.ts).
//!
//! `prepare_package` lives in this crate rather than a sibling because
//! both fetchers above are its only consumers.

mod cas_io;
mod error;
mod fetcher;
mod packlist;
mod preferred_pm;
mod prepare_package;
mod tarball_fetcher;

pub use error::{GitFetcherError, PacklistError, PreparePackageError};
pub use fetcher::{GitFetchOutput, GitFetcher};
pub use packlist::packlist;
pub use preferred_pm::{PreferredPm, detect_preferred_pm};
pub use prepare_package::{PreparePackageOptions, PreparedPackage, prepare_package};
pub use tarball_fetcher::GitHostedTarballFetcher;
