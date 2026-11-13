//! The connect policy of registries the admin API wrote.

use std::net::IpAddr;

use super::RouteContext;

impl RouteContext {
    /// This context for registries the admin API wrote: the resolver
    /// fetches from this server's own host only at the `public_url` origin,
    /// whichever route, upstream, or redirect leads there.
    #[must_use]
    pub fn with_registries_in_database(mut self) -> Self {
        self.connect_policy = self.connect_policy.with_exact_own_origin();
        self
    }

    /// Whether the resolver may fetch from the host of `url`, a
    /// `routes.public` registry: see [`Self::allows_fetch`], without the
    /// allowlist.
    #[must_use]
    pub fn allows_route_host(&self, url: &str) -> bool {
        self.connect_policy.allows_fetched_host(url)
    }

    /// Whether `url` is at this server's own `public_url` origin.
    #[must_use]
    pub fn is_own_origin(&self, url: &str) -> bool {
        self.connect_policy.is_own_origin(url)
    }

    /// Whether a client built from an upstream URL that the admin API wrote
    /// may connect to `address`: [`Self::allows_address`] without the
    /// exemption for this server's own host, since that URL is not this
    /// server's to vouch for.
    #[must_use]
    pub fn allows_stored_upstream_address(&self, address: IpAddr) -> bool {
        self.connect_policy.allows_listed_address(address)
    }

    /// Whether a client built from an upstream URL that the admin API wrote
    /// may fetch `url`: not this server's own `public_url` origin, which
    /// would loop back to it, and not an IP literal
    /// [`Self::allows_stored_upstream_address`] refuses.
    #[must_use]
    pub fn allows_stored_upstream_url(&self, url: &str) -> bool {
        !self.connect_policy.is_own_origin(url)
            && self.connect_policy.allows_listed_literal_host(url)
    }
}
