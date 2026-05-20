//! Pacquet port of pnpm's
//! [`@pnpm/engine.runtime.deno-resolver`](https://github.com/pnpm/pnpm/blob/1627943d2a/engine/runtime/deno-resolver/src/index.ts).
//!
//! Resolves `deno@runtime:<spec>` dependencies. Two pieces:
//!
//! 1. **Version selection** delegates to the npm resolver — Deno
//!    publishes a `deno` package on the registry whose `manifest.version`
//!    field tracks every GitHub release. Using the registry avoids the
//!    paginated GitHub releases API and keeps `minimumReleaseAge`
//!    enforcement uniform with the rest of the install.
//! 2. **Asset enumeration** then talks to the GitHub Releases API for
//!    that tag, downloads each artifact's per-file `.sha256sum`, and
//!    emits one [`PlatformAssetResolution`](pacquet_lockfile::PlatformAssetResolution)
//!    per `(os, cpu)` triple.
//!
//! Architecture differs slightly from upstream: pacquet's resolver
//! trait owns an [`Arc<dyn Resolver>`](pacquet_resolving_resolver_base::Resolver)
//! for the npm side rather than taking a function reference, so the
//! same instance can plug into the default-resolver chain both
//! directly and as the version-selection dependency of this resolver.

mod deno_resolver;
mod read_deno_assets;

pub use deno_resolver::{DenoResolver, DenoResolverError};
pub use read_deno_assets::ReadDenoAssetsError;
