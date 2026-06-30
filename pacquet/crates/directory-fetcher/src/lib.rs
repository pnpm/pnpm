//! The fetcher used for `LockfileResolution::Directory` snapshots,
//! i.e. injected workspace packages (`file:./local-pkg` in the
//! manifest, `dependenciesMeta[*].injected = true`).
//!
//! See [`DirectoryFetcher`] for the public entry point.

mod error;
mod fetcher;
mod walker;

pub use error::DirectoryFetcherError;
pub use fetcher::{DirectoryFetchOutput, DirectoryFetcher};
