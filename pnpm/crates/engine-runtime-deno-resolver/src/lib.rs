//! Resolves `deno@runtime:<spec>` dependencies. Two pieces:
//!
//! 1. **Version selection** delegates to the npm resolver — Deno
//!    publishes a `deno` package on the registry whose `manifest.version`
//!    field tracks every GitHub release. Using the registry avoids the
//!    paginated GitHub releases API and keeps `minimumReleaseAge`
//!    enforcement uniform with the rest of the install.
//! 2. **Asset enumeration** then talks to the GitHub Releases API for
//!    that tag, downloads each artifact's per-file `.sha256sum`, and
//!    emits one [`PlatformAssetResolution`](pnpm_lockfile::PlatformAssetResolution)
//!    per `(os, cpu)` triple.

#[cfg(target_family = "wasm")]
extern crate pnpm_http as reqwest;

pub use deno_resolver::{DenoResolver, DenoResolverError};
pub use read_deno_assets::ReadDenoAssetsError;

mod deno_resolver;
mod read_deno_assets;
