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
mod fast_path;
mod package_name;
mod policy;
mod publish;
mod search;
mod server;
mod streaming;
mod upstream;

pub use auth::{AuthState, TokenStore, UserStore, identify};
pub use config::{
    AuthConfig, Config, ConfigSource, DEFAULT_CONFIG_YAML, HtpasswdConfig, LogConfig, LogFormat,
    LogLevel, MaxUsers, PackageAccess, TokensConfig, UplinkConfig,
};
pub use error::{RegistryError, Result};
pub use policy::{AccessList, AccessToken, Identity, PackagePolicies, PackagePolicy};
pub use server::{router, router_with_auth, serve, serve_listener};
