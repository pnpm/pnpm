//! pnpm-compatible npm registry server.
//!
//! Implements a tiny verdaccio-shaped proxy: a [`router`] exposes a
//! packument endpoint and a tarball endpoint that fetch from
//! a configurable upstream npm registry and cache the responses on
//! disk.
//!
//! See <https://github.com/pnpm/pnpm> for the parent project.

mod auth;
mod cache;
mod config;
mod error;
mod package_name;
mod policy;
mod publish;
mod server;
mod streaming;
mod upstream;

pub use config::Config;
pub use error::{RegistryError, Result};
pub use policy::{AccessRule, PackagePolicies, PackagePolicy};
pub use server::{router, serve};
