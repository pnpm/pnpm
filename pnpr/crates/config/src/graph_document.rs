//! The registry graph as one document: the `registries`, `defaultRegistry`,
//! and `routes.public` keys of pnpr.yaml. The config file declares it, or,
//! with `registriesManagedBy: api`, the metadata database stores it.

pub use edits::{HostedStorage, PackageLists, RegistryGraphChanges, RuleLists};

pub(crate) use credentials::is_env_name;

use super::{
    DefaultRegistryFile, Deserialize, EnvVar, IndexMap, IpNetwork, PublicRoute, PublicRouteFile,
    RegistryError, RegistryFile, RoutePolicy, RoutingConfig, Serialize, SystemEnv,
    null_default::deserialize_null_default, resolve_file_registries,
};

mod credentials;
mod edits;

/// The registry graph in the shape pnpr.yaml declares it.
///
/// `deserialize(serialize(document))` yields an equal document, so a stored
/// document and an exported one round-trip.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegistryGraphDocument {
    #[serde(
        default,
        deserialize_with = "deserialize_null_default",
        skip_serializing_if = "IndexMap::is_empty"
    )]
    pub(crate) registries: IndexMap<String, RegistryFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) default_registry: Option<DefaultRegistryFile>,
    #[serde(
        default,
        deserialize_with = "deserialize_null_default",
        skip_serializing_if = "GraphRoutes::is_empty"
    )]
    pub(crate) routes: GraphRoutes,
}

/// The `routes:` keys that belong to the graph. `allowedPrivateNetworks` stays
/// in the config file.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct GraphRoutes {
    #[serde(
        default,
        deserialize_with = "deserialize_null_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub(crate) public: Vec<PublicRouteFile>,
}

impl GraphRoutes {
    fn is_empty(&self) -> bool {
        self.public.is_empty()
    }
}

/// Where a document came from, which decides how its upstream credentials
/// resolve.
#[derive(Debug, Clone, Copy)]
pub enum DocumentOrigin<'env> {
    /// The config file. Its `${VAR}` placeholders were substituted when the
    /// file was read.
    File,
    /// The metadata database. Upstream credentials may only reference the
    /// variables listed in `registryEnv`, and resolve when the routing is
    /// built. See [`RegistryGraphDocument::check_stored`].
    Stored { registry_env: &'env [String] },
}

/// How [`RoutingConfig::from_document`] builds the routing.
#[derive(Debug, Clone)]
pub struct RoutingOptions<'env> {
    pub origin: DocumentOrigin<'env>,
    /// `routes.allowedPrivateNetworks` from the config file.
    pub allowed_private_networks: Vec<IpNetwork>,
    /// Whether upstream serving configs and credentials are resolved. A tier
    /// with the registry surface disabled validates the graph without them.
    pub resolve_upstreams: bool,
}

impl RegistryGraphDocument {
    /// Parse a document written as YAML, as `pnpr registries import` reads
    /// one. An empty file is the empty graph.
    pub fn from_yaml(raw: &str) -> Result<Self, RegistryError> {
        serde_saphyr::from_str::<Option<Self>>(raw)
            .map(Option::unwrap_or_default)
            .map_err(|err| RegistryError::InvalidConfig { reason: err.to_string() })
    }

    /// The document as YAML, in the shape [`Self::from_yaml`] reads.
    pub fn to_yaml(&self) -> Result<String, RegistryError> {
        serde_saphyr::to_string(self)
            .map_err(|err| RegistryError::Internal { reason: err.to_string() })
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.registries.is_empty()
            && self.default_registry.is_none()
            && self.routes.is_empty()
    }

    pub(crate) fn from_file_parts(
        registries: IndexMap<String, RegistryFile>,
        default_registry: Option<DefaultRegistryFile>,
        public: Vec<PublicRouteFile>,
    ) -> Self {
        Self { registries, default_registry, routes: GraphRoutes { public } }
    }
}

impl RoutingConfig {
    /// Build and validate the routing a document declares. Every rule the
    /// config file's `registries:` block is held to applies, plus the
    /// stored-credential rules for a [`DocumentOrigin::Stored`] document.
    pub fn from_document(
        document: &RegistryGraphDocument,
        options: &RoutingOptions<'_>,
    ) -> Result<Self, RegistryError> {
        Self::from_document_with_env::<SystemEnv>(document, options)
    }

    /// [`Self::from_document`] reading environment variables through `Sys`.
    pub fn from_document_with_env<Sys: EnvVar>(
        document: &RegistryGraphDocument,
        options: &RoutingOptions<'_>,
    ) -> Result<Self, RegistryError> {
        Self::build_from_document::<Sys>(document.clone(), options)
    }

    pub(crate) fn build_from_document<Sys: EnvVar>(
        document: RegistryGraphDocument,
        options: &RoutingOptions<'_>,
    ) -> Result<Self, RegistryError> {
        let document = match options.origin {
            DocumentOrigin::File => document,
            DocumentOrigin::Stored { registry_env } => {
                document.check_stored(registry_env)?;
                if options.resolve_upstreams {
                    document.with_credentials_resolved::<Sys>()?
                } else {
                    document
                }
            }
        };
        let RegistryGraphDocument {
            registries,
            default_registry,
            routes,
        } = document;
        let resolved = resolve_file_registries::<Sys>(
            registries,
            default_registry,
            options.resolve_upstreams,
        )?;
        Ok(Self {
            upstreams: resolved.upstreams,
            route_policy: RoutePolicy {
                public: routes.public
                    .into_iter()
                    .map(PublicRoute::from)
                    .collect(),
                allowed_private_networks: options.allowed_private_networks.clone(),
            },
            registries: resolved.registries,
            hosted: resolved.hosted,
            tar_registries: resolved.tar_registries,
        })
    }
}

impl From<PublicRouteFile> for PublicRoute {
    fn from(route: PublicRouteFile) -> Self {
        Self { registry: route.registry, package: route.package }
    }
}
