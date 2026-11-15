use std::net::IpAddr;

use pnpm_network::is_public_address;
use pnpr_config::{IpNetwork, RoutingConfig};
use url::{Host, Url};

/// The addresses pnpr may open a resolver connection to: public addresses,
/// the networks the operator allowed with `routes.allowedPrivateNetworks`,
/// and this server's own `public_url` origin, which the resolver fetches
/// hosted packages from. Another port of the own host is another service on
/// the same machine, so it is not exempt.
#[derive(Debug, Clone)]
pub(super) struct ConnectPolicy {
    own: Option<OwnUrl>,
    allowed_private_networks: Vec<IpNetwork>,
}

/// `public_url`, compared by canonical host.
#[derive(Debug, Clone)]
struct OwnUrl {
    scheme: String,
    host: HostKey,
    port: Option<u16>,
}

/// A host as own-host comparisons see it: an IP literal by its canonical
/// address, so `::ffff:a.b.c.d` is `a.b.c.d`, and a name by
/// [`normalized_host_name`].
#[derive(Debug, Clone, PartialEq, Eq)]
enum HostKey {
    Address(IpAddr),
    Name(String),
}

impl HostKey {
    fn of_url(url: &Url) -> Option<Self> {
        Some(match url.host()? {
            Host::Ipv4(address) => Self::Address(IpAddr::V4(address)),
            Host::Ipv6(address) => Self::Address(IpAddr::V6(address).to_canonical()),
            Host::Domain(name) => Self::Name(normalized_host_name(name)),
        })
    }

    /// The key of a host as a resolver names it: a name, or an IP literal
    /// with or without brackets.
    fn of_host(host: &str) -> Self {
        let literal = host
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
            .unwrap_or(host);
        match literal.parse::<IpAddr>() {
            Ok(address) => Self::Address(address.to_canonical()),
            Err(_) => Self::Name(normalized_host_name(host)),
        }
    }
}

/// `host` lowercased, without one terminal dot, so `Registry.Example.` and
/// `registry.example` are one host.
#[must_use]
pub fn normalized_host_name(host: &str) -> String {
    host.strip_suffix('.')
        .unwrap_or(host)
        .to_ascii_lowercase()
}

impl ConnectPolicy {
    pub(super) fn from_routing(routing: &RoutingConfig, public_url: &str) -> Self {
        let own = Url::parse(public_url)
            .ok()
            .and_then(|url| {
                Some(OwnUrl {
                    host: HostKey::of_url(&url)?,
                    scheme: url.scheme().to_string(),
                    port: url.port_or_known_default(),
                })
            });
        Self {
            own,
            allowed_private_networks: routing.route_policy
                .allowed_private_networks
                .clone(),
        }
    }

    /// The own literal address is admitted only for the own host, whose
    /// fetches [`Self::allows_own_host_origin`] limits to `public_url`'s
    /// origin, so another name for it cannot reach another port.
    pub(super) fn allows_address(&self, host: &str, address: IpAddr) -> bool {
        let is_own_host = self.is_own_host_key(&HostKey::of_host(host));
        if self.is_own_address(address) {
            return is_own_host;
        }
        self.allows_listed_address(address) || is_own_host
    }

    fn is_own_host_key(&self, key: &HostKey) -> bool {
        self.own
            .as_ref()
            .is_some_and(|own| own.host == *key)
    }

    fn is_own_address(&self, address: IpAddr) -> bool {
        self.is_own_host_key(&HostKey::Address(address.to_canonical()))
    }

    /// Whether `url` is on this server's own `public_url` host, at any origin.
    pub(super) fn is_own_host(&self, url: &str) -> bool {
        Url::parse(url)
            .ok()
            .and_then(|url| HostKey::of_url(&url))
            .is_some_and(|key| self.is_own_host_key(&key))
    }

    /// Whether `url` is at `public_url`'s origin.
    pub(super) fn is_own_origin(&self, url: &str) -> bool {
        let (Some(own), Ok(url)) = (&self.own, Url::parse(url)) else { return false };
        url.scheme() == own.scheme
            && url.port_or_known_default() == own.port
            && HostKey::of_url(&url).as_ref() == Some(&own.host)
    }

    /// An address a client built from a URL the admin API wrote may connect
    /// to: [`Self::allows_listed_address`], and not `public_url`'s literal
    /// address.
    pub(super) fn allows_stored_address(&self, address: IpAddr) -> bool {
        !self.is_own_address(address) && self.allows_listed_address(address)
    }

    /// `false` when `url`'s host is an IP literal
    /// [`Self::allows_stored_address`] refuses.
    pub(super) fn allows_stored_literal_host(&self, url: &str) -> bool {
        literal_address(url).is_none_or(|(_, address)| self.allows_stored_address(address))
    }

    /// [`Self::allows_address`] without the exemption for this server's own
    /// host: a public address, or one in `routes.allowedPrivateNetworks`.
    pub(super) fn allows_listed_address(&self, address: IpAddr) -> bool {
        let address = address.to_canonical();
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

    /// `false` when `url` is on this server's own host at another origin than
    /// `public_url`'s, whatever that host is: a hostname, or a private or
    /// public IP literal.
    pub(super) fn allows_own_host_origin(&self, url: &str) -> bool {
        !self.is_own_host(url) || self.is_own_origin(url)
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
