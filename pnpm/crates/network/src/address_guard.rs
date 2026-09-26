use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::Arc,
};

/// Decides whether a connection to `address`, resolved for `host`, may be
/// opened. See [`GuardedDnsResolver`].
pub type AddressGuard = Arc<dyn Fn(&str, IpAddr) -> bool + Send + Sync>;

/// A DNS resolver that refuses a name when the guard rejects any address it
/// resolves to.
///
/// The connector dials only the addresses a resolver returns, so checking
/// them here is what a connection is made to, not a lookup that a second one
/// could answer differently (DNS rebinding). Refusing the whole name rather
/// than dropping the rejected addresses keeps a name that mixes public and
/// internal addresses from reaching the internal ones on a retry. A URL whose
/// host is an IP literal never reaches a resolver, so callers that guard
/// connections must check literal hosts themselves.
pub struct GuardedDnsResolver {
    inner: Arc<dyn Resolve>,
    guard: AddressGuard,
}

impl GuardedDnsResolver {
    #[must_use]
    pub fn new(inner: Arc<dyn Resolve>, guard: AddressGuard) -> Self {
        Self { inner, guard }
    }
}

impl Resolve for GuardedDnsResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        let lookup = self.inner.resolve(name);
        let guard = Arc::clone(&self.guard);
        Box::pin(async move {
            let addresses: Vec<_> = lookup.await?.collect();
            if addresses.is_empty() {
                return Err(Box::new(BlockedAddress { host, address: None })
                    as Box<dyn std::error::Error + Send + Sync>);
            }
            if let Some(blocked) = addresses
                .iter()
                .find(|address| !guard(&host, address.ip()))
            {
                return Err(Box::new(BlockedAddress { host, address: Some(blocked.ip()) })
                    as Box<dyn std::error::Error + Send + Sync>);
            }
            Ok(Box::new(addresses.into_iter()) as Addrs)
        })
    }
}

#[derive(Debug)]
pub(crate) struct BlockedAddress {
    host: String,
    address: Option<IpAddr>,
}

impl std::fmt::Display for BlockedAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.address {
            Some(address) => write!(
                f,
                "{} resolves to {address}, which this client is not allowed to connect to",
                self.host,
            ),
            None => write!(f, "{} resolves to no address", self.host),
        }
    }
}

impl std::error::Error for BlockedAddress {}

/// Whether `address` is a globally routable unicast address: not loopback,
/// private, link-local (which holds the `169.254.169.254` cloud metadata
/// endpoint), shared, documentation, benchmarking, multicast, or reserved,
/// and not an IPv6 form that embeds or tunnels to such an IPv4 address.
#[must_use]
pub fn is_public_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_public_ipv4(address),
        IpAddr::V6(address) => is_public_ipv6(address),
    }
}

fn is_public_ipv4(address: Ipv4Addr) -> bool {
    let [first, second, third, _] = address.octets();
    !address.is_private()
        && !address.is_loopback()
        && !address.is_link_local()
        && !address.is_documentation()
        && first != 0
        && first < 224
        && !(first == 100 && (64..=127).contains(&second))
        && !(first == 192 && second == 0 && third == 0)
        && !(first == 192 && second == 88 && third == 99)
        && !(first == 198 && (18..=19).contains(&second))
}

fn is_public_ipv6(address: Ipv6Addr) -> bool {
    let Some(mapped) = address.to_ipv4_mapped() else {
        let segments = address.segments();
        return (segments[0] & 0xe000) == 0x2000
            && !(segments[0] == 0x2001 && (segments[1] < 0x200 || segments[1] == 0xdb8))
            && segments[0] != 0x2002
            && !(segments[0] == 0x3fff && segments[1] < 0x1000);
    };
    is_public_ipv4(mapped)
}

#[cfg(test)]
mod tests;
