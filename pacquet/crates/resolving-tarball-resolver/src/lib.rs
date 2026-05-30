//! Pacquet port of pnpm's
//! [`@pnpm/resolving.tarball-resolver`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/tarball-resolver/src/index.ts).
//!
//! Claims any wanted dependency whose bare specifier starts with
//! `http://` or `https://` and resolves it to a tarball URL. Two
//! pieces of upstream behavior the port preserves:
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
//! Unlike pnpm — which defers the tarball download (and thus the
//! manifest read + integrity computation) to the package-requester
//! after resolution — pacquet builds the lockfile *before* the
//! install/fetch pass, so for a remote (non-registry) tarball *direct*
//! dependency the resolver must learn name/version/integrity here.
//! When a [`TarballFetchContext`] is supplied, the resolver downloads
//! the tarball, computes its sha512 integrity, extracts it to the
//! store, and reads its bundled manifest, warming the shared mem cache
//! so the install pass reuses the extraction without re-downloading.
//! See pnpm's
//! [`packageRequester.ts`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/installing/package-requester/src/packageRequester.ts).

mod tarball_resolver;

pub use tarball_resolver::{TarballFetchContext, TarballResolver};
