//! The connect policy of registries the admin API wrote.

use std::net::IpAddr;

use super::{RouteContext, nerf_prefix};

impl RouteContext {
    /// This context for registries the admin API wrote: the resolver
    /// fetches from this server's own host only at the `public_url` origin,
    /// whichever route, upstream, or redirect leads there.
    #[must_use]
    pub fn with_registries_in_database(mut self) -> Self {
        self.connect_policy = self.connect_policy.with_exact_own_origin();
        self
    }

    /// Whether the resolver may fetch from some URL the `routes.public`
    /// `registry` matches: see [`Self::allows_fetch`], without the allowlist.
    /// A route matches without its scheme, so a fetch over either scheme
    /// counts. A `registry` that is not a registry URL matches nothing.
    #[must_use]
    pub fn allows_route_host(&self, registry: &str) -> bool {
        let Some(prefix) = nerf_prefix(registry) else { return true };
        ["http", "https"]
            .iter()
            .any(|scheme| self.connect_policy.allows_fetched_host(&format!("{scheme}:{prefix}")))
    }

    /// Whether `url` is on this server's own `public_url` host, at any
    /// origin.
    #[must_use]
    pub fn is_own_host(&self, url: &str) -> bool {
        self.connect_policy.is_own_host(url)
    }

    /// Whether a client built from an upstream URL that the admin API wrote
    /// may connect to `address`: [`Self::allows_address`] without the
    /// exemption for this server's own host, since that URL is not this
    /// server's to vouch for, and never `public_url`'s literal address.
    #[must_use]
    pub fn allows_stored_upstream_address(&self, address: IpAddr) -> bool {
        self.connect_policy.allows_stored_address(address)
    }

    /// Whether a client built from an upstream URL that the admin API wrote
    /// may fetch `url`: not on this server's own `public_url` host at any
    /// origin, since its own origin loops back to it and another port is
    /// another service on the same machine, and not an IP literal
    /// [`Self::allows_stored_upstream_address`] refuses.
    #[must_use]
    pub fn allows_stored_upstream_url(&self, url: &str) -> bool {
        !self.connect_policy.is_own_host(url) && self.connect_policy.allows_stored_literal_host(url)
    }
}
