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
//! The resolver doesn't compute or stamp `integrity` — that work
//! happens later in the package-requester after the tarball is
//! downloaded. See pnpm's
//! [`packageRequester.ts`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/installing/package-requester/src/packageRequester.ts).

mod tarball_resolver;

pub use tarball_resolver::TarballResolver;
