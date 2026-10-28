//! The fetcher used for `LockfileResolution::Directory` snapshots,
//! i.e. injected workspace packages (`file:./local-pkg` in the
//! manifest, `dependenciesMeta[*].injected = true`).
//!
//! See [`DirectoryFetcher`] for the public entry point.

pub use copy_files::CopyFiles;
pub use error::DirectoryFetcherError;
pub use fetcher::{DirectoryFetchOutput, DirectoryFetcher};

mod copy_files;
mod error;
mod fetcher;
mod walker;
