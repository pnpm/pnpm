//! Resolves `yarn@runtime:<spec>` dependencies for Yarn 6 and above.
//!
//! Yarn Classic and Yarn Berry are npm packages, but Yarn 6
//! ([`yarnpkg/zpm`](https://github.com/yarnpkg/zpm)) is a native binary
//! published only as GitHub release archives. Version selection therefore
//! reads the release list instead of a packument, and each asset's
//! integrity comes from the digest the release API reports for it.
//!
//! The resolver answers the same `runtime:` protocol the managed-runtime
//! resolvers use, because that is how pnpm expresses "a platform archive
//! rather than a package"; Yarn is a package manager, not a runtime.

mod read_yarn_releases;
mod yarn_resolver;

pub use read_yarn_releases::{ReadYarnReleasesError, YarnRelease};
pub use yarn_resolver::{YarnResolver, YarnResolverError, resolve_yarn_version};
