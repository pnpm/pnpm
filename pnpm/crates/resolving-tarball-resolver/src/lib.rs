//! Claims any wanted dependency whose bare specifier starts with
//! `http://` or `https://` and resolves it to a tarball URL. Two
//! pieces of behavior to call out:
//!
//! - **URL normalization.** The bare specifier is round-tripped
//!   through `url::Url` so a redundant default port
//!   (`registry.npmjs.org:443`) is dropped before it reaches the
//!   lockfile.
//! - **Immutable-redirect follow.** A HEAD request runs against the
//!   normalized URL; if the response carries `cache-control:
//!   immutable`, the *post-redirect* URL is stored in the
//!   resolution. Mutable hosts (e.g. `github.com/.../tarball/master`)
//!   keep the original URL so subsequent installs revalidate the
//!   moving target.
//!
//! Because pacquet builds the lockfile *before* the install/fetch
//! pass, for a remote (non-registry) tarball *direct* dependency the
//! resolver must learn name/version/integrity here rather than
//! deferring it to a later fetch step. When a [`TarballFetchContext`]
//! is supplied, the resolver downloads the tarball, computes its
//! sha512 integrity, extracts it to the store, and reads its bundled
//! manifest, warming the shared mem cache so the install pass reuses
//! the extraction without re-downloading.

mod tarball_resolver;

pub use tarball_resolver::{TarballFetchContext, TarballResolver};
