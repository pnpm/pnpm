//! pnpm-compatible npm registry server.
//!
//! Implements a tiny verdaccio-shaped proxy: a [`router`] exposes a
//! packument endpoint and a tarball endpoint that fetch from
//! a configurable upstream npm registry and cache the responses on
//! disk.
//!
//! See <https://github.com/pnpm/pnpm> for the parent project.

mod resolver;
mod server;

pub use pnpr_auth::{
    AuthState, TokenBackend, TokenRecord, TokenStore, UpsertOutcome, UserBackend, UserStore,
    identify,
};
pub use pnpr_config::{
    AccessSpec, ArtifactsFeature, AuthConfig, BackendConfig, Config, ConfigSource,
    DEFAULT_CONFIG_YAML, FeatureOverrides, HostedConfig, HostedStoreConfig, HtpasswdConfig,
    LibsqlSettings, LogConfig, LogFormat, LogLevel, MaxUsers, OsvConfig, PackageAccess,
    PublicRoute, RegistryFeature, ResolverFeature, RoutePolicy, S3Settings, SqlBackendSettings,
    Teams, TokensConfig, UpstreamConfig, default_cache_dir,
};
pub use pnpr_error::{RegistryError, Result};
pub use pnpr_policy::{AccessList, AccessToken, Identity, PackageRule, PackageRules};
pub use pnpr_registry::{
    ConcreteKind, PackagePattern, Registries, Registry, RegistryConfigError, Resolved,
};
pub use pnpr_storage::journal::recover_publish_journal;
pub use server::{
    router, router_with_auth, serve, serve_listener, try_router, try_router_with_auth,
};
