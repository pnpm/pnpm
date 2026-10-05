use super::AuthHeaders;
use crate::AddressGuard;
use std::sync::Arc;

/// Server-side override for upstream auth selection.
///
/// A plain [`AuthHeaders`] answers "what `Authorization` header does the
/// client's `.npmrc` attach to this URL?" — the right question for the
/// pnpm CLI, which fetches as the user. A server (pnpr) that resolves on
/// behalf of many callers must instead answer "what credential does *this
/// deployment's route policy* attach to this fetch, for this caller?" and
/// record which private route was touched so the result can be cached
/// without leaking one caller's private resolution to another.
///
/// When a hook is attached via [`AuthHeaders::with_route_hook`], every
/// [`AuthHeaders::for_url`] / [`AuthHeaders::for_url_with_package`] lookup
/// is delegated to it: the client-forwarded credentials carried by the
/// [`AuthHeaders`] are ignored, and the hook alone decides the header
/// (returning `None` for an anonymous/public fetch) and records the
/// route. A `None` hook (the CLI case) leaves lookup behavior unchanged.
pub trait UpstreamRouteHook: Send + Sync {
    /// Decide the `Authorization` header value for a fetch to `url` for
    /// package `package` (`None` for non-package fetches), and record the
    /// route the decision selected. `None` means fetch anonymously.
    fn authorize(&self, url: &str, package: Option<&str>) -> Option<String>;

    /// Whether this deployment may fetch `url` at all.
    ///
    /// A server resolves on behalf of callers who describe their own
    /// registries, so a URL it was told about is not automatically one it may
    /// reach. Answered at the fetch itself rather than when the request is
    /// read, so a registry a caller merely configures — a scope it never
    /// resolves a package from — costs nothing, while one it does resolve
    /// from is refused before the request leaves the process.
    ///
    /// Defaults to `true` for hooks with no such policy (the CLI fetches as
    /// the user, who may reach whatever they configured).
    fn allows_fetch(&self, _url: &str) -> bool {
        true
    }

    /// The addresses a connection made on this deployment's behalf may
    /// reach, for connections a caller opens outside the HTTP client, such
    /// as a `git` subprocess. `None` for hooks with no such policy.
    fn connect_guard(&self) -> Option<AddressGuard> {
        None
    }

    /// Classify the metadata cache scope for a fetch to `url` for package
    /// `package` (`None` for non-package fetches). Unlike [`Self::authorize`]
    /// this is a read-only query — it must **not** record into the resolve's
    /// footprint — so the resolver can pick the on-disk mirror namespace and
    /// in-memory/fetch-lock keys without double-counting a route.
    ///
    /// Defaults to [`MetadataCacheScope::Public`] for hooks that don't
    /// partition metadata by route.
    fn metadata_scope(&self, _url: &str, _package: Option<&str>) -> MetadataCacheScope {
        MetadataCacheScope::Public
    }
}

/// The cache namespace a metadata fetch for one `(registry, package)` route
/// belongs to, decided once per fetch from the route policy. A server (pnpr)
/// that resolves on behalf of many callers must keep one caller's private
/// metadata out of the global mirror every other caller reads; this enum is
/// how the route decision reaches the npm resolver's mirror path, in-memory
/// cache key, and fetch-lock key.
///
/// The pnpm CLI has no route hook, so every fetch is [`Self::Public`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataCacheScope {
    /// Public route: the shared, global metadata mirror.
    Public,
    /// Private route keyed by a private access descriptor. `descriptor_id`
    /// is a filesystem-safe, server-secret-keyed digest that namespaces the
    /// on-disk mirror, in-memory cache, and fetch lock, so one caller's
    /// private metadata never satisfies a fetch for a caller who does not
    /// reproduce the same descriptor.
    Private { descriptor_id: String },
}

impl AuthHeaders {
    /// Attach a server-side [`UpstreamRouteHook`] that takes over auth
    /// selection. The returned [`AuthHeaders`] keeps its
    /// client-forwarded credentials (so [`Self::to_by_scope`] still
    /// reflects them) but no longer consults them on lookup — the hook
    /// decides. Used by pnpr to resolve as the deployment's route policy
    /// rather than as the calling client.
    #[must_use]
    pub fn with_route_hook(mut self, hook: Arc<dyn UpstreamRouteHook>) -> Self {
        self.route_hook = Some(hook);
        self
    }

    /// Whether the fetch to `url` is permitted, per an attached
    /// [`UpstreamRouteHook`]. Always true without one — see
    /// [`UpstreamRouteHook::allows_fetch`].
    #[must_use]
    pub fn allows_fetch(&self, url: &str) -> bool {
        self.route_hook
            .as_ref()
            .is_none_or(|hook| hook.allows_fetch(url))
    }

    /// See [`UpstreamRouteHook::connect_guard`].
    #[must_use]
    pub fn connect_guard(&self) -> Option<AddressGuard> {
        self.route_hook.as_ref().and_then(|hook| hook.connect_guard())
    }

    /// Record the route for a metadata/tarball fetch that is about to be
    /// served from an in-memory or on-disk cache *without* an HTTP
    /// request, so a server [`UpstreamRouteHook`]'s footprint still
    /// reflects every private route the resolve depended on. The route is
    /// classified exactly as the real fetch would have (same `url`, same
    /// `pkg_name`); the credential the hook selects is discarded because
    /// no request is sent.
    ///
    /// No-op when no route hook is installed (the CLI case): a fetch that
    /// never happens needs no `Authorization` header, and the CLI keeps no
    /// footprint. Idempotent for the hook — recording the same route more
    /// than once collapses to one footprint entry.
    pub fn record_route(&self, url: &str, pkg_name: Option<&str>) {
        if let Some(hook) = &self.route_hook {
            hook.authorize(url, pkg_name);
        }
    }

    /// The metadata cache scope a fetch to `url` for `pkg_name` belongs to.
    /// A server route hook owns the decision; without one (the CLI case)
    /// every fetch is [`MetadataCacheScope::Public`], leaving the global
    /// mirror unchanged. Read-only — never records into a footprint.
    #[must_use]
    pub fn metadata_scope(&self, url: &str, pkg_name: Option<&str>) -> MetadataCacheScope {
        match &self.route_hook {
            Some(hook) => hook.metadata_scope(url, pkg_name),
            None => MetadataCacheScope::Public,
        }
    }
}
