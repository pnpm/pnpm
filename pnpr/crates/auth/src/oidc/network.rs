use pnpm_network::{GuardedDnsResolver, is_public_address, native_dns_resolver};
use pnpr_error::{RegistryError, Result};
use std::{net::IpAddr, sync::Arc};
use url::{Host, Url};

pub(super) fn public_resolver() -> Arc<GuardedDnsResolver> {
    Arc::new(GuardedDnsResolver::new(
        native_dns_resolver(),
        Arc::new(|_, address| is_public_address(address)),
    ))
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

fn blocked() -> RegistryError {
    RegistryError::InvalidConfig {
        reason: "OIDC endpoints must resolve exclusively to public IP addresses".to_string(),
    }
}

#[cfg(test)]
mod tests;
