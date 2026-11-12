use std::net::IpAddr;

use pnpm_network::is_public_address;
use pnpr_config::{IpNetwork, RoutingConfig};
use url::{Host, Url};

/// The addresses pnpr may open a resolver connection to: public addresses,
/// the networks the operator allowed with `routes.allowedPrivateNetworks`,
/// and whatever this server's own `public_url` host names, which the resolver
/// fetches hosted packages from.
#[derive(Debug, Clone)]
pub(super) struct ConnectPolicy {
    own_host: Option<String>,
    /// The origin of `public_url`.
    own_origin: Option<url::Origin>,
    /// The origins of upstreams the admin API wrote. On the own host, only
    /// `own_origin` is exempt for them, so an upstream on another port of
    /// that host is not.
    stored_origins: Vec<url::Origin>,
    allowed_private_networks: Vec<IpNetwork>,
}

impl ConnectPolicy {
    pub(super) fn from_routing(routing: &RoutingConfig, public_url: &str) -> Self {
        let own_url = Url::parse(public_url).ok();
        Self {
            own_host: own_url
                .as_ref()
                .and_then(|url| url.host_str().map(str::to_owned)),
            own_origin: own_url.map(|url| url.origin()),
            stored_origins: Vec::new(),
            allowed_private_networks: routing.route_policy
                .allowed_private_networks
                .clone(),
        }
    }

    pub(super) fn allows_address(&self, host: &str, address: IpAddr) -> bool {
        self.allows_listed_address(address)
            || self.own_host
                .as_deref()
                .is_some_and(|own_host| own_host.eq_ignore_ascii_case(host))
    }

    /// [`Self::allows_address`] without the exemption for this server's own
    /// host: a public address, or one in `routes.allowedPrivateNetworks`.
    pub(super) fn allows_listed_address(&self, address: IpAddr) -> bool {
        is_public_address(address)
            || self.allowed_private_networks
                .iter()
                .any(|network| network.contains(address))
    }

    /// `false` when `url`'s host is an IP literal [`Self::allows_address`]
    /// refuses. A literal never reaches a DNS resolver, so this is where it
    /// is checked; a hostname's addresses are checked when it resolves.
    pub(super) fn allows_literal_host(&self, url: &str) -> bool {
        literal_address(url).is_none_or(|(host, address)| self.allows_address(&host, address))
    }

    /// The host checks a resolver fetch of `url` passes: see
    /// [`Self::allows_literal_host`] and [`Self::allows_own_host_origin`].
    pub(super) fn allows_fetched_host(&self, url: &str) -> bool {
        self.allows_literal_host(url) && self.allows_own_host_origin(url)
    }

    /// This policy with the upstreams of `routing` as ones the admin API wrote.
    pub(super) fn with_stored_upstreams(mut self, routing: &RoutingConfig) -> Self {
        self.stored_origins = routing.upstreams
            .values()
            .filter_map(|upstream| Url::parse(&upstream.url).ok())
            .map(|url| url.origin())
            .collect();
        self
    }

    /// `false` when `url` is at the origin of an upstream the admin API wrote,
    /// on this server's own host but not at `public_url`'s origin, and only
    /// the exemption for the own host would admit it. That exemption covers
    /// the hosted packages the resolver fetches from this server. A
    /// hostname's addresses are checked when it resolves, with the exemption,
    /// so such a URL on an own hostname is refused here.
    pub(super) fn allows_own_host_origin(&self, url: &str) -> bool {
        let Ok(parsed) = Url::parse(url) else { return true };
        let origin = parsed.origin();
        let is_own_host = self.own_host
            .as_deref()
            .zip(parsed.host_str())
            .is_some_and(|(own_host, host)| own_host.eq_ignore_ascii_case(host));
        if !is_own_host
            || self.own_origin.as_ref() == Some(&origin)
            || !self.stored_origins.contains(&origin)
        {
            return true;
        }
        literal_address(url).is_some_and(|(_, address)| self.allows_listed_address(address))
    }

    /// [`Self::allows_literal_host`] checked with
    /// [`Self::allows_listed_address`].
    pub(super) fn allows_listed_literal_host(&self, url: &str) -> bool {
        literal_address(url).is_none_or(|(_, address)| self.allows_listed_address(address))
    }
}

/// The host and address of `url` when its host is an IP literal.
fn literal_address(url: &str) -> Option<(String, IpAddr)> {
    let url = Url::parse(url).ok()?;
    let address = match url.host()? {
        Host::Ipv4(address) => IpAddr::V4(address),
        Host::Ipv6(address) => IpAddr::V6(address),
        Host::Domain(_) => return None,
    };
    Some((
        url.host_str()
            .unwrap_or_default()
            .to_string(),
        address,
    ))
}

#[cfg(test)]
mod tests;
