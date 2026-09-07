use super::{PublicResolver, is_public_address, validate_destination};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use std::{net::SocketAddr, sync::Arc};

struct FixedResolver(Vec<SocketAddr>);
impl Resolve for FixedResolver {
    fn resolve(&self, _name: Name) -> Resolving {
        let addresses = self.0.clone();
        Box::pin(async move { Ok(Box::new(addresses.into_iter()) as Addrs) })
    }
}

#[test]
fn refuses_nonpublic_literals_and_transition_addresses() {
    for address in [
        "127.0.0.1",
        "10.0.0.1",
        "169.254.169.254",
        "172.16.0.1",
        "192.168.1.1",
        "100.64.0.1",
        "198.18.0.1",
        "192.0.2.1",
        "224.0.0.1",
        "0.1.2.3",
        "240.0.0.1",
        "::1",
        "fc00::1",
        "fe80::1",
        "::ffff:127.0.0.1",
        "2001:db8::1",
        "2002:7f00:1::",
        "64:ff9b::7f00:1",
    ] {
        assert!(!is_public_address(address.parse().unwrap()), "accepted {address}");
    }
    for url in ["https://127.0.0.1/jwks", "https://[::1]/token", "https://[::ffff:10.0.0.1]/jwks"] {
        assert!(validate_destination(url).is_err(), "accepted {url}");
    }
    for address in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
        assert!(is_public_address(address.parse().unwrap()), "rejected {address}");
    }
}

#[tokio::test]
async fn checks_the_addresses_returned_to_the_connector() {
    let resolver = PublicResolver(Arc::new(FixedResolver(vec![
        "8.8.8.8:0".parse().unwrap(),
        "127.0.0.1:0".parse().unwrap(),
    ])));
    assert!(resolver.resolve("issuer.example".parse().unwrap()).await.is_err());
    let resolver = PublicResolver(Arc::new(FixedResolver(vec!["8.8.8.8:0".parse().unwrap()])));
    let addresses: Vec<_> =
        resolver.resolve("issuer.example".parse().unwrap()).await.unwrap().collect();
    assert_eq!(addresses, vec!["8.8.8.8:0".parse::<SocketAddr>().unwrap()]);
}
