//! Claims any wanted dependency whose bare specifier starts with
//! `http://` or `https://` and resolves it to a tarball URL.
//! Behavior to call out:
//!
//! - **URL normalization.** The bare specifier is round-tripped
//!   through `url::Url` so a redundant default port
//!   (`registry.npmjs.org:443`) is dropped before it reaches the
//!   lockfile.
//! - **Immutable redirects.** A successful immutable response pins the final
//!   URL in the lockfile. Mutable responses keep the requested URL.
//!
//! Because pacquet builds the lockfile *before* the install/fetch
//! pass, for a remote (non-registry) tarball *direct* dependency the
//! resolver must learn name/version/integrity here rather than
//! deferring it to a later fetch step. When a [`TarballFetchContext`]
//! is supplied, the resolver downloads the tarball, computes its
//! sha512 integrity, extracts it to the store, and reads its bundled
//! manifest, warming the shared mem cache so the install pass reuses
//! the extraction without re-downloading.

#[cfg(target_family = "wasm")]
extern crate pnpm_http as reqwest;

pub use tarball_resolver::{
    PriorTarballEntry,
    TarballFetchContext,
    TarballResolver,
    remote_tarball_id,
};

mod http_cache;
mod tarball_resolver;
