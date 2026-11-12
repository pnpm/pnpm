//! The connect policy of the upstream clients built from URLs the admin API
//! wrote.

use std::net::IpAddr;

use pnpr_config::RoutingConfig;

use super::RouteContext;

impl RouteContext {
    /// This context with the upstreams of `routing` as ones the admin API
    /// wrote: the resolver fetches from them at the own host only at the
    /// `public_url` origin.
    #[must_use]
    pub fn with_stored_upstreams(mut self, routing: &RoutingConfig) -> Self {
        self.connect_policy = self.connect_policy.with_stored_upstreams(routing);
        self
    }

    /// Whether a client built from an upstream URL that the admin API wrote
    /// may connect to `address`: [`Self::allows_address`] without the
    /// exemption for this server's own host, since that URL is not this
    /// server's to vouch for.
    #[must_use]
    pub fn allows_stored_upstream_address(&self, address: IpAddr) -> bool {
        self.connect_policy.allows_listed_address(address)
    }

    /// `false` when `url`'s host is an IP literal
    /// [`Self::allows_stored_upstream_address`] refuses.
    #[must_use]
    pub fn allows_stored_upstream_literal_host(&self, url: &str) -> bool {
        self.connect_policy.allows_listed_literal_host(url)
    }
}
