use std::net::{IpAddr, SocketAddr};

use pnpm_network::{AddressGuard, GuardedDnsResolver, native_dns_resolver};
use reqwest::dns::{Name, Resolve};

use crate::resolve_ref::GitRunError;

/// `git -c` settings that pin an `http(s)` remote to the addresses `guard`
/// admits, so `git` connects to the addresses checked here instead of
/// resolving the name again, which could now answer with an internal address
/// (DNS rebinding). Redirects are refused, since `git` would resolve a
/// redirect target itself. Pinning needs git 2.37 or later
/// (`http.curloptResolve`); an older git ignores the setting.
///
/// Only `http(s)` remotes can be pinned. ssh, `git://`, and the other
/// transports resolve the name inside `git` or `ssh`, so for them the fetch
/// allowlist checks the remote's origin but not the address it resolves to.
/// An IP-literal host needs no pin: the allowlist check applies the connect
/// policy to it.
pub(crate) async fn pinned_git_config(
    repo: &str,
    guard: AddressGuard,
) -> Result<Vec<String>, GitRunError> {
    let resolver = GuardedDnsResolver::new(native_dns_resolver(), guard);
    pin_with(repo, &resolver).await
}

pub(crate) async fn pin_with(
    repo: &str,
    resolver: &dyn Resolve,
) -> Result<Vec<String>, GitRunError> {
    let Ok(url) = reqwest::Url::parse(repo.strip_prefix("git+").unwrap_or(repo)) else {
        return Ok(Vec::new());
    };
    if !matches!(url.scheme(), "http" | "https") {
        return Ok(Vec::new());
    }
    let (Some(host), Some(port)) = (url.domain(), url.port_or_known_default()) else {
        return Ok(Vec::new());
    };
    let name: Name = host
        .parse()
        .map_err(|_| GitRunError { message: format!("{host} is not a valid host name") })?;
    let addresses: Vec<SocketAddr> = resolver
        .resolve(name)
        .await
        .map_err(|error| GitRunError { message: error.to_string() })?
        .collect();
    let addresses = addresses
        .iter()
        .map(|address| match address.ip() {
            IpAddr::V4(ip) => ip.to_string(),
            IpAddr::V6(ip) => format!("[{ip}]"),
        })
        .collect::<Vec<_>>()
        .join(",");
    Ok(vec![
        format!("http.curloptResolve={host}:{port}:{addresses}"),
        "http.followRedirects=false".to_string(),
    ])
}

#[cfg(test)]
mod tests;
