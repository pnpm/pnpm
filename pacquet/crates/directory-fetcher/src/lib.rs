//! Port of pnpm's
//! [`fetching/directory-fetcher`](https://github.com/pnpm/pnpm/blob/85ceff2383/fetching/directory-fetcher/src/index.ts)
//! — the fetcher used for `LockfileResolution::Directory` snapshots,
//! i.e. injected workspace packages (`file:./local-pkg` in the
//! manifest, `dependenciesMeta[*].injected = true`).
//!
//! See [`DirectoryFetcher`] for the public entry point.

mod error;
mod fetcher;
mod walker;

pub use error::DirectoryFetcherError;
pub use fetcher::{DirectoryFetchOutput, DirectoryFetcher};
