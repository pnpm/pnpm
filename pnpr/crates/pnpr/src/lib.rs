//! pnpm-compatible npm registry server.
//!
//! Implements a tiny verdaccio-shaped proxy: a [`router`] exposes a
//! packument endpoint and a tarball endpoint that fetch from
//! a configurable upstream npm registry and cache the responses on
//! disk.
//!
//! See <https://github.com/pnpm/pnpm> for the parent project.

mod auth;
mod config;
mod error;
mod journal;
mod package_name;
mod policy;
mod publish;
mod registry;
mod resolver;
mod route;
mod s3;
mod search;
mod server;
mod storage;
mod streaming;
mod upstream;

pub use auth::{
    AuthState, TokenBackend, TokenRecord, TokenStore, UpsertOutcome, UserBackend, UserStore,
    identify,
};
pub use config::{
    AccessSpec, AuthConfig, BackendConfig, Config, ConfigSource, DEFAULT_CONFIG_YAML,
    FeatureOverrides, HostedConfig, HostedStoreConfig, HtpasswdConfig, LibsqlSettings, LogConfig,
    LogFormat, LogLevel, MaxUsers, OsvConfig, PackageAccess, PublicRoute, RegistryFeature,
    ResolverFeature, RoutePolicy, SqlBackendSettings, TokensConfig, UpstreamConfig,
    default_cache_dir,
};
pub use error::{RegistryError, Result};
pub use journal::recover_publish_journal;
pub use policy::{AccessList, AccessToken, Identity, PackageRule, PackageRules};
pub use registry::{
    ConcreteKind, PackagePattern, Registries, Registry, RegistryConfigError, Resolved,
};
pub use server::{
    router, router_with_auth, serve, serve_listener, try_router, try_router_with_auth,
};
