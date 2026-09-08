use pnpr_error::{RegistryError, Result};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::Arc,
};
use url::{Host, Url};

pub(super) struct PublicResolver(pub Arc<dyn Resolve>);

impl Resolve for PublicResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let future = self.0.resolve(name);
        Box::pin(async move {
            let addresses: Vec<_> = future.await?.collect();
            if addresses.is_empty()
                || addresses.iter().any(|address| !is_public_address(address.ip()))
            {
                return Err(Box::new(blocked()) as Box<dyn std::error::Error + Send + Sync>);
            }
            Ok(Box::new(addresses.into_iter()) as Addrs)
        })
    }
}

pub(super) fn validate_destination(raw: &str) -> Result<()> {
    let url = Url::parse(raw).map_err(|_| blocked())?;
    #[cfg(test)]
    if url.scheme() == "http" && url.host_str() == Some("127.0.0.1") {
        return Ok(());
    }
    let address = match url.host() {
        Some(Host::Ipv4(address)) => IpAddr::V4(address),
        Some(Host::Ipv6(address)) => IpAddr::V6(address),
        Some(Host::Domain(_)) => return Ok(()),
        None => return Err(blocked()),
    };
    if is_public_address(address) { Ok(()) } else { Err(blocked()) }
}

fn is_public_address(address: IpAddr) -> bool {
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

fn blocked() -> RegistryError {
    RegistryError::InvalidConfig {
        reason: "OIDC endpoints must resolve exclusively to public IP addresses".to_string(),
    }
}

#[cfg(test)]
mod tests;
