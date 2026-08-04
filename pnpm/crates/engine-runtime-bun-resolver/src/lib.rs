//! Resolves `bun@runtime:<spec>` dependencies. Same shape as the
//! [`pacquet_engine_runtime_deno_resolver`](https://docs.rs/pacquet-engine-runtime-deno-resolver)
//! crate: version selection delegates to the npm resolver, and asset
//! enumeration walks the GitHub Release `SHASUMS256.txt`. Bun's
//! asset names are simpler — one zip per `(platform, arch)` with an
//! optional `-musl` suffix — so the SHASUMS file alone has every
//! integrity needed without per-asset SHA256 sidecar requests.

mod bun_resolver;
mod read_bun_assets;

pub use bun_resolver::{BunResolver, BunResolverError};
pub use read_bun_assets::ReadBunAssetsError;
