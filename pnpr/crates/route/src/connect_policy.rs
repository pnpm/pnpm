use std::net::IpAddr;

use pnpm_network::is_public_address;
use pnpr_config::{Config, IpNetwork};
use url::{Host, Url};

/// The addresses pnpr may open a resolver connection to: public addresses,
/// the networks the operator allowed with `resolver.allowedPrivateNetworks`,
/// and whatever this server's own `public_url` host names, which the resolver
/// fetches hosted packages from.
#[derive(Debug, Clone)]
pub(super) struct ConnectPolicy {
    own_host: Option<String>,
    allowed_private_networks: Vec<IpNetwork>,
}

impl ConnectPolicy {
    pub(super) fn from_config(config: &Config) -> Self {
        Self {
            own_host: Url::parse(&config.http.public_url)
                .ok()
                .and_then(|url| url.host_str().map(str::to_owned)),
            allowed_private_networks: config.features.resolver
                .allowed_private_networks
                .clone(),
        }
    }

    pub(super) fn allows_address(&self, host: &str, address: IpAddr) -> bool {
        is_public_address(address)
            || self.own_host
                .as_deref()
                .is_some_and(|own_host| own_host.eq_ignore_ascii_case(host))
            || self.allowed_private_networks
                .iter()
                .any(|network| network.contains(address))
    }

    /// `false` when `url`'s host is an IP literal [`Self::allows_address`]
    /// refuses. A literal never reaches a DNS resolver, so this is where it
    /// is checked; a hostname's addresses are checked when it resolves.
    pub(super) fn allows_literal_host(&self, url: &str) -> bool {
        let Ok(url) = Url::parse(url) else { return true };
        let address = match url.host() {
            Some(Host::Ipv4(address)) => IpAddr::V4(address),
            Some(Host::Ipv6(address)) => IpAddr::V6(address),
            Some(Host::Domain(_)) | None => return true,
        };
        self.allows_address(url.host_str().unwrap_or_default(), address)
    }
}

#[cfg(test)]
mod tests;
