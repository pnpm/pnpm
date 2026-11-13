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
    /// The address of `public_url` when its host is an IP literal.
    own_address: Option<IpAddr>,
    /// Whether the exemption for the own host covers only `own_origin`, as
    /// it does when the admin API writes the registries, so a destination
    /// on another port of that host is not exempt.
    exact_own_origin: bool,
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
            own_address: literal_address(public_url).map(|(_, address)| address),
            exact_own_origin: false,
            allowed_private_networks: routing.route_policy
                .allowed_private_networks
                .clone(),
        }
    }

    /// With the exemption limited to `public_url`'s origin, the own literal
    /// address is admitted only for the own host, whose fetches
    /// [`Self::allows_own_host_origin`] limits to that origin, so another
    /// name for it cannot reach another port.
    pub(super) fn allows_address(&self, host: &str, address: IpAddr) -> bool {
        let is_own_host = self.is_own_host_name(host);
        if self.exact_own_origin && self.own_address == Some(address) {
            return is_own_host;
        }
        self.allows_listed_address(address) || is_own_host
    }

    fn is_own_host_name(&self, host: &str) -> bool {
        self.own_host
            .as_deref()
            .is_some_and(|own_host| own_host.eq_ignore_ascii_case(host))
    }

    /// Whether `url` is on this server's own `public_url` host, at any origin.
    pub(super) fn is_own_host(&self, url: &str) -> bool {
        Url::parse(url)
            .ok()
            .and_then(|url| {
                url.host_str()
                    .map(|host| self.is_own_host_name(host))
            })
            .unwrap_or(false)
    }

    /// An address a client built from a URL the admin API wrote may connect
    /// to: [`Self::allows_listed_address`], and not `public_url`'s literal
    /// address.
    pub(super) fn allows_stored_address(&self, address: IpAddr) -> bool {
        self.own_address != Some(address) && self.allows_listed_address(address)
    }

    /// `false` when `url`'s host is an IP literal
    /// [`Self::allows_stored_address`] refuses.
    pub(super) fn allows_stored_literal_host(&self, url: &str) -> bool {
        literal_address(url).is_none_or(|(_, address)| self.allows_stored_address(address))
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

    /// This policy with the own-host exemption limited to `public_url`'s
    /// origin.
    pub(super) fn with_exact_own_origin(mut self) -> Self {
        self.exact_own_origin = true;
        self
    }

    /// `false` when the exemption is limited to `public_url`'s origin and
    /// `url` is on this server's own host at another origin, whatever that
    /// host is: a hostname, or a private or public IP literal. The exemption
    /// covers the hosted packages the resolver fetches from this server;
    /// another port of the host is another service on the same machine.
    pub(super) fn allows_own_host_origin(&self, url: &str) -> bool {
        !self.exact_own_origin || !self.is_own_host(url) || self.is_own_origin(url)
    }

    /// Whether `url` is at `public_url`'s origin.
    pub(super) fn is_own_origin(&self, url: &str) -> bool {
        Url::parse(url).is_ok_and(|url| self.own_origin.as_ref() == Some(&url.origin()))
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
