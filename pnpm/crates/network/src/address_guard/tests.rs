use super::{AddressGuard, GuardedDnsResolver, is_public_address};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use std::{net::SocketAddr, sync::Arc};

struct FixedResolver(Vec<SocketAddr>);

impl Resolve for FixedResolver {
    fn resolve(&self, _name: Name) -> Resolving {
        let addresses = self.0.clone();
        Box::pin(async move { Ok(Box::new(addresses.into_iter()) as Addrs) })
    }
}

fn public_only() -> AddressGuard {
    Arc::new(|_, address| is_public_address(address))
}

#[test]
fn refuses_nonpublic_and_transition_addresses() {
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
        "::ffff:169.254.169.254",
        "2001:db8::1",
        "2002:7f00:1::",
        "64:ff9b::7f00:1",
    ] {
        assert!(!is_public_address(address.parse().unwrap()), "accepted {address}");
    }
    for address in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
        assert!(is_public_address(address.parse().unwrap()), "rejected {address}");
    }
}

#[tokio::test]
async fn refuses_a_name_when_any_address_is_rejected() {
    let resolver = GuardedDnsResolver::new(
        Arc::new(FixedResolver(vec![
            "8.8.8.8:0".parse().unwrap(),
            "169.254.169.254:0".parse().unwrap(),
        ])),
        public_only(),
    );
    let error = resolver
        .resolve("registry.example".parse().unwrap())
        .await
        .err()
        .expect("a name resolving to the metadata address is refused");
    assert_eq!(
        error.to_string(),
        "registry.example resolves to 169.254.169.254, which this client is not allowed to connect to",
    );
}

#[tokio::test]
async fn refuses_a_name_with_no_address() {
    let resolver = GuardedDnsResolver::new(Arc::new(FixedResolver(Vec::new())), public_only());
    assert!(
        resolver
            .resolve("registry.example".parse().unwrap())
            .await
            .is_err(),
    );
}

#[tokio::test]
async fn passes_the_host_to_the_guard_and_returns_accepted_addresses() {
    let guard: AddressGuard =
        Arc::new(|host, address| host == "pnpr.internal" || is_public_address(address));
    let loopback: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let resolver = GuardedDnsResolver::new(Arc::new(FixedResolver(vec![loopback])), guard);
    let addresses: Vec<_> = resolver
        .resolve("pnpr.internal".parse().unwrap())
        .await
        .unwrap()
        .collect();
    assert_eq!(addresses, vec![loopback]);
    assert!(
        resolver
            .resolve("other.internal".parse().unwrap())
            .await
            .is_err(),
    );
}
