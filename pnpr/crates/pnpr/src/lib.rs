//! pnpm-compatible npm registry server.
//!
//! Implements a tiny verdaccio-shaped proxy: a [`router`] exposes a
//! packument endpoint and a tarball endpoint that fetch from
//! a configurable upstream npm registry and cache the responses on
//! disk.
//!
//! See <https://github.com/pnpm/pnpm> for the parent project.

// The streamed resolve spawns the engine's whole resolve-and-fetch future
// graph; proving it `Send` walks deeper than rustc's default limit.
#![recursion_limit = "256"]

pub mod oci_maintenance;

pub use pnpr_auth::{
    AuthState, TokenBackend, TokenRecord, TokenStore, UpsertOutcome, UserBackend, UserStore,
    identify,
};
pub use pnpr_config::{
    AccessSpec, ArtifactsFeature, AuthConfig, BackendConfig, Config, ConfigSource, CorsConfig,
    DEFAULT_CONFIG_YAML, FeatureOverrides, HostedConfig, HostedStoreConfig, HtpasswdConfig,
    LibsqlSettings, LogConfig, LogFormat, LogLevel, MaxUsers, OsvConfig, PackageAccess,
    PublicRoute, RegistryFeature, ResolverFeature, RoutePolicy, S3Settings, SqlBackendSettings,
    Teams, TokensConfig, UpstreamConfig, default_cache_dir,
};
pub use pnpr_error::{RegistryError, Result};
pub use pnpr_policy::{AccessList, AccessToken, Identity, PackageRule, PackageRules};
pub use pnpr_registry::{
    ConcreteKind, Ecosystem, PackagePattern, Registries, Registry, RegistryConfigError, Resolved,
};
pub use server::{
    recover_publish_journal, router, router_with_auth, serve, serve_listener, try_router,
    try_router_with_auth,
};

mod resolver;
mod server;
