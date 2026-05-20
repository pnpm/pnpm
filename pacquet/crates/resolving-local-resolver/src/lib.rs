//! Pacquet port of pnpm's
//! [`@pnpm/resolving.local-resolver`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/local-resolver/src/index.ts).
//!
//! Resolves `file:`, `link:`, `workspace:`, and bare filesystem
//! specifiers — the four shapes the install layer can satisfy from
//! the project tree rather than a registry or git host. The fetch
//! side for the directory case lives in `pacquet-directory-fetcher`;
//! this crate is resolution-only.
//!
//! Three public entry points mirror upstream's:
//!
//! - [`resolve_from_local_scheme`] — claims a wanted dep iff its bare
//!   specifier starts with `link:`, `workspace:`, or `file:`. The
//!   `path:` prefix is rejected with [`PathProtocolNotSupportedError`]
//!   to match upstream's
//!   [`PATH_IS_UNSUPPORTED_PROTOCOL`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/local-resolver/src/parseBareSpecifier.ts#L29-L34)
//!   error.
//! - [`resolve_from_local_path`] — claims a wanted dep purely by path
//!   shape (relative path, absolute path, drive letter, tarball
//!   filename). Bare-specifier dispatchers run this *after*
//!   [`resolve_from_local_scheme`] so scheme prefixes don't slip
//!   through.
//! - [`resolve_latest_from_local`] — declines (returns
//!   `Ok(LatestInfo::default())`) for `link:` / `file:` /
//!   `workspace:` specs so they don't accidentally route to a
//!   named-registry alias named `link` / `file` / `workspace`.

mod chain;
mod local_resolver;
mod parse_bare_specifier;

pub use chain::{LocalPathResolver, LocalResolver, LocalSchemeResolver};
pub use local_resolver::{
    LocalCurrentPkg, LocalResolveResult, LocalResolverContext, LocalResolverOptions,
    LocalResolverUpdate, LocalSpecError, ResolveLocalError, resolve_from_local_path,
    resolve_from_local_scheme, resolve_latest_from_local,
};
pub use parse_bare_specifier::{PathProtocolNotSupportedError, WantedLocalDependency};
