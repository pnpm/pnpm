//! The connect policy of the upstream clients built from URLs the admin API
//! wrote.

use std::net::IpAddr;

use super::RouteContext;

impl RouteContext {
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
