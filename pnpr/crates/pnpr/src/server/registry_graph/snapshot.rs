use super::super::{
    AppInner, Upstream, compute_upstream_cache_namespace, ecosystem::upstream_fetch_guard,
};
use indexmap::IndexMap;
use pnpm_network::{AddressGuard, RedirectGuard};
use pnpr_config::{Config, Management, RoutingConfig, UpstreamConfig};
use pnpr_route::RouteContext;
use reqwest::header::HeaderMap;
use std::{
    ops::Deref,
    sync::{Arc, OnceLock, PoisonError, RwLock, atomic::Ordering},
    time::Duration,
};

/// The route context upstream fetch guards check redirects against. Shared
/// by every snapshot, so an upstream client that a rebuild reuses checks
/// against the current graph.
pub(crate) type UpstreamGuardContext = Arc<RwLock<Arc<RouteContext>>>;

/// The registry graph and everything the server derives from it. A request
/// reads one snapshot from start to finish (see [`super::with_pinned_routing`]), so a
/// graph change never mixes two graphs within one request.
pub(crate) struct RoutingSnapshot {
    routing: RoutingConfig,
    /// One client per upstream registry, keyed like
    /// [`RoutingConfig::upstreams`]. Empty when the registry surface is off,
    /// since only the registry routes consult them.
    pub(crate) upstream_clients: IndexMap<String, Upstream>,
    /// The disposable cache namespace of each upstream, keyed like
    /// [`Self::upstream_clients`]. A pure function of the graph and the cache secret
    /// (see [`compute_upstream_cache_namespace`]), precomputed so the
    /// per-request path doesn't re-sort and re-hash an upstream's headers.
    pub(crate) upstream_cache_namespaces: IndexMap<String, String>,
    pub(crate) route_context: Arc<RouteContext>,
    /// The resolver engine for this graph, built on the first resolve.
    pub(crate) resolver: OnceLock<crate::resolver::Resolver>,
    /// The stored graph revision this snapshot was built from. `None` for a
    /// graph the configuration file declares.
    pub(crate) generation: Option<u64>,
    /// How many snapshots this server served before this one. Set when the
    /// snapshot starts serving; caches that outlive a snapshot key by it.
    pub(crate) epoch: u64,
    client_keys: IndexMap<String, UpstreamClientKey>,
}

impl Deref for RoutingSnapshot {
    type Target = RoutingConfig;

    fn deref(&self) -> &RoutingConfig {
        &self.routing
    }
}

/// The upstream settings an [`Upstream`] client is built from. A rebuild
/// reuses the previous client, with its circuit-breaker state, while these
/// are unchanged.
#[derive(PartialEq)]
struct UpstreamClientKey {
    url: String,
    headers: HeaderMap,
    packument_ttl: Option<Duration>,
    cache: bool,
    timeout: Duration,
    max_packument_size: usize,
    max_fails: u32,
    fail_timeout: Duration,
}

impl UpstreamClientKey {
    fn of(upstream: &UpstreamConfig) -> Self {
        Self {
            url: upstream.url.clone(),
            headers: upstream.headers.clone(),
            packument_ttl: upstream.packument_ttl,
            cache: upstream.cache,
            timeout: upstream.requests.timeout,
            max_packument_size: upstream.requests.max_packument_size,
            max_fails: upstream.requests.max_fails,
            fail_timeout: upstream.requests.fail_timeout,
        }
    }
}

/// Build the snapshot of `routing`, reusing each upstream client of
/// `previous` whose settings are unchanged.
pub(crate) fn build_snapshot(
    config: &Config,
    routing: RoutingConfig,
    previous: Option<&RoutingSnapshot>,
    guard_context: &UpstreamGuardContext,
) -> RoutingSnapshot {
    let client_keys: IndexMap<String, UpstreamClientKey> = routing.upstreams
        .iter()
        .map(|(name, upstream)| (name.clone(), UpstreamClientKey::of(upstream)))
        .collect();
    let upstream_clients = if config.features.registry.enabled {
        let sources =
            ClientSources { previous, guard_context, managed_by: config.registries_managed_by };
        upstream_clients(&routing, &client_keys, &sources)
    } else {
        IndexMap::new()
    };
    let upstream_cache_namespaces = routing.upstreams
        .keys()
        .map(|name| {
            let namespace =
                compute_upstream_cache_namespace(&routing, &config.resolution_cache_secret, name);
            (name.clone(), namespace)
        })
        .collect();
    let route_context = RouteContext::from_routing(&routing, &config.http.public_url);
    let route_context = Arc::new(match config.registries_managed_by {
        Management::Api => route_context.with_registries_in_database(),
        Management::Config => route_context,
    });
    RoutingSnapshot {
        routing,
        upstream_clients,
        upstream_cache_namespaces,
        route_context,
        resolver: OnceLock::new(),
        generation: None,
        epoch: 0,
        client_keys,
    }
}

/// What a snapshot's upstream clients are built from besides the graph.
struct ClientSources<'a> {
    /// The snapshot whose unchanged clients are reused.
    previous: Option<&'a RoutingSnapshot>,
    guard_context: &'a UpstreamGuardContext,
    /// Who wrote the upstream URLs, which decides how the clients are guarded.
    managed_by: Management,
}

fn upstream_clients(
    routing: &RoutingConfig,
    client_keys: &IndexMap<String, UpstreamClientKey>,
    sources: &ClientSources<'_>,
) -> IndexMap<String, Upstream> {
    let ClientSources { previous, guard_context, managed_by } = *sources;
    routing.upstreams
        .iter()
        .map(|(name, upstream)| {
            let reused = previous.and_then(|previous| {
                let unchanged = previous.client_keys.get(name) == client_keys.get(name);
                previous.upstream_clients
                    .get(name)
                    .filter(|_| unchanged)
                    .cloned()
            });
            let client = reused.unwrap_or_else(|| match managed_by {
                Management::Config => Upstream::new(name, upstream)
                    .with_fetch_guard(upstream_fetch_guard(guard_context, upstream)),
                Management::Api => stored_upstream(name, upstream, guard_context),
            });
            (name.clone(), client)
        })
        .collect()
}

/// The client of an upstream the database stores. An API-written URL must not
/// reach loopback, private, or link-local addresses outside
/// `routes.allowedPrivateNetworks`, whether it names them by IP literal or by
/// a hostname that resolves to them, including this server's own host, nor
/// this server's own `public_url` origin.
fn stored_upstream(
    name: &str,
    upstream: &UpstreamConfig,
    guard_context: &UpstreamGuardContext,
) -> Upstream {
    let fetch_guard = upstream_fetch_guard(guard_context, upstream);
    let literal_context = Arc::clone(guard_context);
    let guard: RedirectGuard = Arc::new(move |url| {
        fetch_guard(url)
            && current_context(&literal_context).allows_stored_upstream_url(url.as_str())
    });
    let connect_context = Arc::clone(guard_context);
    let connect_guard: AddressGuard = Arc::new(move |_host, address| {
        current_context(&connect_context).allows_stored_upstream_address(address)
    });
    Upstream::new(name, upstream).with_fetch_and_connect_guards(guard, connect_guard)
}

fn current_context(guard_context: &UpstreamGuardContext) -> Arc<RouteContext> {
    Arc::clone(&guard_context.read().unwrap_or_else(PoisonError::into_inner))
}

impl AppInner {
    /// The current snapshot, for code outside a request's pin.
    pub(crate) fn current_routing(&self) -> Arc<RoutingSnapshot> {
        Arc::clone(&self.routing.read().unwrap_or_else(PoisonError::into_inner))
    }

    /// Serve `snapshot` from now on. Requests that already pinned the previous
    /// snapshot finish on it.
    pub(crate) fn replace_routing(&self, mut snapshot: RoutingSnapshot) -> Arc<RoutingSnapshot> {
        snapshot.epoch = self.routing_epochs.fetch_add(1, Ordering::Relaxed) + 1;
        let snapshot = Arc::new(snapshot);
        *self.upstream_guard_context.write().unwrap_or_else(PoisonError::into_inner) =
            Arc::clone(&snapshot.route_context);
        *self.routing.write().unwrap_or_else(PoisonError::into_inner) = Arc::clone(&snapshot);
        if let Some(resolver) = self.resolver_shared.get() {
            resolver.forget_results();
        }
        snapshot
    }
}

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod tests;
