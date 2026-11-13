//! The registry graph the database stores: reading a stored
//! revision, building its routing, and the checks a new revision must pass
//! before it is written.

use super::super::{oidc::validate_workloads, oidc_groups::validate_group_grants};
use pnpm_env_replace::EnvVar;
use pnpr_config::{Config, GraphSource, RegistryGraphDocument, RoutingConfig, RoutingOptions};
use pnpr_error::RegistryError;
use pnpr_route::RouteContext;
use pnpr_storage::{
    Storage,
    database::{HostedNamespace, StoredRegistryGraph},
};
use std::net::IpAddr;
use url::{Host, Url};

/// Whether building the routing reads the upstream credentials the document
/// references.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Credentials {
    /// Resolve them from this process's environment, as a serving replica must.
    Resolve,
    /// Check that each reference is allowed, and stand in for its value.
    /// Offline commands that send no upstream request use this, so they run
    /// without the variables set; the routing is never used for a request.
    Skip,
}

/// An environment in which every variable has a stand-in value.
struct StandInCredentials;

impl EnvVar for StandInCredentials {
    fn var(_name: &str) -> Option<String> {
        Some("unread".to_string())
    }
}

/// The version string of a stored generation, as `ETag` and `version` carry
/// it. `"0"` is the empty graph before the first revision.
pub(crate) fn graph_version(generation: Option<u64>) -> String {
    generation.unwrap_or(0).to_string()
}

/// The document `stored` holds; no stored revision is the empty graph.
pub(crate) fn stored_document(
    stored: Option<&StoredRegistryGraph>,
) -> Result<RegistryGraphDocument, RegistryError> {
    stored.map_or_else(
        || Ok(RegistryGraphDocument::default()),
        |stored| parse_document(&stored.document),
    )
}

/// Parse the JSON text of a stored revision.
pub(crate) fn parse_document(text: &str) -> Result<RegistryGraphDocument, RegistryError> {
    serde_json::from_str(text)
        .map_err(|err| RegistryError::Internal {
            reason: format!("the stored registry graph is unreadable: {err}"),
        })
}

/// Build the routing a stored `document` declares on this process.
pub(crate) fn stored_routing(
    config: &Config,
    document: &RegistryGraphDocument,
    credentials: Credentials,
) -> Result<RoutingConfig, RegistryError> {
    let options = RoutingOptions {
        registry_env: &config.registry_env,
        allowed_private_networks: config.routing.route_policy
            .allowed_private_networks
            .clone(),
        resolve_upstreams: config.features.registry.enabled,
    };
    match credentials {
        Credentials::Resolve => RoutingConfig::from_document(document, &options),
        Credentials::Skip => {
            RoutingConfig::from_document_with_env::<StandInCredentials>(document, &options)
        }
    }
}

/// The config file's references into the graph: the OIDC workloads and group
/// grants must name hosted registries and teams that exist.
pub(crate) fn check_file_references(
    config: &Config,
    routing: &RoutingConfig,
) -> Result<(), RegistryError> {
    validate_workloads(config, routing)?;
    validate_group_grants(config, routing)
}

/// A revision that passed [`check_revision`].
pub(crate) struct CheckedRevision {
    pub(crate) routing: RoutingConfig,
    pub(crate) hosted: HostedChange,
}

/// The hosted namespaces of a revision, as
/// [`pnpr_storage::database::NewRegistryGraph`] takes them.
pub(crate) struct HostedChange {
    /// The namespaces the revision stops serving. The write checks again, in
    /// its transaction, that they hold no package.
    pub(crate) vacated: Vec<HostedNamespace>,
    pub(crate) bound: Vec<HostedNamespace>,
}

/// Validate `next` as the revision that replaces `previous`. An invalid
/// document is a `400`; a change the stored state refuses is a `409`.
pub(crate) async fn check_revision(
    config: &Config,
    storage: &Storage,
    previous: &RegistryGraphDocument,
    next: &RegistryGraphDocument,
    credentials: Credentials,
) -> Result<CheckedRevision, RegistryError> {
    let routing = stored_routing(config, next, credentials).map_err(invalid_document)?;
    pnpr_route::validate_public_routes(&routing.route_policy)
        .map_err(|reason| RegistryError::BadRequest { reason })?;
    check_file_references(config, &routing).map_err(refused_change)?;
    check_upstream_hosts(config, &routing, previous, next)?;
    check_public_route_hosts(config, &routing, previous, next)?;
    let vacated = check_hosted_storage(storage, previous, next).await?;
    Ok(CheckedRevision { routing, hosted: HostedChange { vacated, bound: bound_namespaces(next) } })
}

/// Every hosted namespace `document` serves.
pub(crate) fn bound_namespaces(document: &RegistryGraphDocument) -> Vec<HostedNamespace> {
    document
        .registry_names()
        .filter_map(|name| document.hosted_storage(name))
        .map(|hosted| HostedNamespace { namespace: hosted.namespace, ecosystem: hosted.ecosystem })
        .collect()
}

/// The refusal of a write that would vacate `occupied`.
pub(crate) fn occupied_namespace_error(occupied: &HostedNamespace) -> RegistryError {
    RegistryError::AdminConflict {
        reason: format!(
            "the {} namespace {:?} holds packages, so the hosted registry serving it cannot be \
             removed, renamed, or moved to another namespace or ecosystem",
            occupied.ecosystem, occupied.namespace,
        ),
    }
}

/// An upstream URL the admin API writes must not name this server's own
/// `public_url` host at any origin, nor loopback, private, or link-local addresses that
/// `routes.allowedPrivateNetworks` does not allow.
/// Hostnames are checked again on every connection, when they resolve.
fn check_upstream_hosts(
    config: &Config,
    routing: &RoutingConfig,
    previous: &RegistryGraphDocument,
    next: &RegistryGraphDocument,
) -> Result<(), RegistryError> {
    let context = RouteContext::from_routing(routing, &config.http.public_url);
    for name in next.registry_names() {
        let Some(url) = next.upstream_url(name) else { continue };
        if previous.upstream_url(name) == Some(url) {
            continue;
        }
        let parsed = Url::parse(url)
            .ok()
            .filter(|parsed| matches!(parsed.scheme(), "http" | "https") && parsed.has_host())
            .ok_or_else(|| RegistryError::BadRequest {
                reason: format!("upstream registry {name:?} has `url` {url:?}, not an http(s) URL"),
            })?;
        if context.is_own_host(url) {
            return Err(RegistryError::BadRequest {
                reason: format!(
                    "upstream registry {name:?} points at {url:?}, on this server's own host",
                ),
            });
        }
        if !upstream_host_allowed(&context, &parsed) {
            return Err(RegistryError::BadRequest {
                reason: format!(
                    "upstream registry {name:?} points at {url:?}, a loopback, private, or \
                     link-local address that `routes.allowedPrivateNetworks` does not allow",
                ),
            });
        }
    }
    Ok(())
}

/// A `routes.public` registry the admin API writes must not name an address
/// the resolver refuses to fetch from: see
/// [`RouteContext::allows_route_host`].
fn check_public_route_hosts(
    config: &Config,
    routing: &RoutingConfig,
    previous: &RegistryGraphDocument,
    next: &RegistryGraphDocument,
) -> Result<(), RegistryError> {
    let context = RouteContext::from_routing(routing, &config.http.public_url);
    let unchanged = previous.public_route_registries().collect::<Vec<_>>();
    for registry in next.public_route_registries() {
        if unchanged.contains(&registry) {
            continue;
        }
        if !context.allows_route_host(registry) {
            return Err(RegistryError::BadRequest {
                reason: format!(
                    "routes.public registry {registry:?} is a loopback, private, or link-local \
                     address that `routes.allowedPrivateNetworks` does not allow, or this \
                     server's own host at another origin than its public URL",
                ),
            });
        }
    }
    Ok(())
}

fn upstream_host_allowed(context: &RouteContext, url: &Url) -> bool {
    let address = match url.host() {
        Some(Host::Ipv4(address)) => IpAddr::V4(address),
        Some(Host::Ipv6(address)) => IpAddr::V6(address),
        Some(Host::Domain(domain)) => {
            let domain = pnpr_route::normalized_host_name(domain);
            return domain != "localhost" && !domain.ends_with(".localhost");
        }
        None => return false,
    };
    context.allows_stored_upstream_address(address)
}

/// A hosted registry that holds packages keeps its name, namespace, and
/// ecosystem, so its packages stay reachable where clients expect them.
/// Returns the namespaces `next` stops serving.
async fn check_hosted_storage(
    storage: &Storage,
    previous: &RegistryGraphDocument,
    next: &RegistryGraphDocument,
) -> Result<Vec<HostedNamespace>, RegistryError> {
    let mut vacated = Vec::new();
    for name in previous.registry_names() {
        let Some(before) = previous.hosted_storage(name) else { continue };
        if next.hosted_storage(name).as_ref() == Some(&before) {
            continue;
        }
        vacated.push(HostedNamespace {
            namespace: before.namespace.clone(),
            ecosystem: before.ecosystem,
        });
        if storage.hosted_namespace_has_packages(&before.namespace, before.ecosystem).await? {
            return Err(RegistryError::AdminConflict {
                reason: format!(
                    "hosted registry {name:?} holds packages, so it cannot be removed, renamed, or \
                     moved to another namespace or ecosystem",
                ),
            });
        }
    }
    Ok(vacated)
}

fn invalid_document(error: RegistryError) -> RegistryError {
    match error {
        RegistryError::InvalidConfig { reason } => RegistryError::BadRequest { reason },
        other => other,
    }
}

fn refused_change(error: RegistryError) -> RegistryError {
    match error {
        RegistryError::InvalidConfig { reason } => RegistryError::AdminConflict { reason },
        other => other,
    }
}

/// `config` with its routing replaced by the stored graph when the database
/// manages the registries, for commands that run without a server.
pub(crate) async fn with_stored_routing(
    config: &Config,
    credentials: Credentials,
    references: FileReferences,
) -> Result<Config, RegistryError> {
    let mut config = config.clone();
    if config.graph_source == GraphSource::Database {
        config.routing = load_stored_routing(&config, credentials, references)
            .await?
            .0;
    }
    Ok(config)
}

/// The routing of the stored graph and its generation.
pub(crate) async fn load_stored_routing(
    config: &Config,
    credentials: Credentials,
    references: FileReferences,
) -> Result<(RoutingConfig, Option<u64>), RegistryError> {
    let storage = Storage::from_config(&config.storage)?;
    let stored = storage.read_registry_graph().await?;
    let document = stored_document(stored.as_ref())?;
    let routing = loaded_routing(config, &document, credentials, references)?;
    Ok((routing, stored.map(|stored| stored.generation)))
}

/// What loading the stored graph does with a config-file reference (an OIDC
/// workload or group grant) that the graph does not satisfy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileReferences {
    /// Log it and serve the graph: writes refuse such a graph, so only a
    /// config change can cause it, and a replica should keep serving.
    Warn,
    /// Fail, as a check of the config does.
    Refuse,
}

/// The routing of the stored `document`, with `references` deciding what an
/// unsatisfied config-file reference does.
pub(super) fn loaded_routing(
    config: &Config,
    document: &RegistryGraphDocument,
    credentials: Credentials,
    references: FileReferences,
) -> Result<RoutingConfig, RegistryError> {
    let routing = stored_routing(config, document, credentials)?;
    if let Err(err) = check_file_references(config, &routing) {
        match references {
            FileReferences::Refuse => return Err(err),
            FileReferences::Warn => {
                tracing::warn!(
                    error = %err,
                    "the stored registry graph lacks what the config file names",
                );
            }
        }
    }
    Ok(routing)
}
