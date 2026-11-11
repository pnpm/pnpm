use super::{IpNetwork, IpNetworkSet};
use std::net::IpAddr;

fn address(raw: &str) -> IpAddr {
    raw.parse().unwrap()
}

#[test]
fn a_network_contains_the_addresses_under_its_prefix() {
    let network = IpNetwork::parse("10.1.0.0/16").unwrap();
    assert!(network.contains(address("10.1.255.7")));
    assert!(network.contains(address("::ffff:10.1.0.1")));
    assert!(!network.contains(address("10.2.0.1")));
    assert!(!network.contains(address("::1")));

    let network = IpNetwork::parse("fd00::/8").unwrap();
    assert!(network.contains(address("fd12:3456::1")));
    assert!(!network.contains(address("fe80::1")));
}

#[test]
fn a_bare_address_is_a_single_host_network() {
    let network = IpNetwork::parse("127.0.0.1").unwrap();
    assert!(network.contains(address("127.0.0.1")));
    assert!(!network.contains(address("127.0.0.2")));
    assert!(
        IpNetwork::parse("0.0.0.0/0")
            .unwrap()
            .contains(address("8.8.8.8")),
    );
}

#[test]
fn rejects_what_is_not_a_network() {
    for raw in ["", "localhost", "10.0.0.0/33", "::/129", "10.0.0.0/", "10.0.0.0/-1", "10.0.0/8"] {
        assert!(IpNetwork::parse(raw).is_err(), "accepted {raw:?}");
    }
}

fn contains(networks: &[super::IpNetwork], ip: &str) -> bool {
    networks
        .iter()
        .any(|net| net.contains(address(ip)))
}

#[test]
fn parse_trusted_proxies_expands_loopback_and_cidrs() {
    let list = ["loopback", "10.0.0.0/8", "192.168.1.1"];
    let networks = super::parse_trusted_proxies(list).unwrap();
    assert_eq!(networks.len(), 4);
    assert!(contains(&networks, "127.0.0.1"));
    assert!(contains(&networks, "::1"));
    assert!(contains(&networks, "10.5.0.1"));
    assert!(contains(&networks, "192.168.1.1"));
    assert!(!contains(&networks, "192.168.1.2"));
}

#[test]
fn parse_trusted_proxies_rejects_invalid_entries() {
    assert!(super::parse_trusted_proxies(["not-an-ip"]).is_err());
    assert!(super::parse_trusted_proxies(["10.0.0.0/33"]).is_err());
}

#[test]
fn a_network_set_covers_only_the_networks_inside_its_union() {
    let network = |raw: &str| IpNetwork::parse(raw).unwrap();
    let office = IpNetworkSet::new([network("10.1.0.0/16")]);
    assert!(office.covers(&network("10.1.0.0/16")));
    assert!(office.covers(&network("10.1.2.0/24")));
    assert!(office.covers(&network("10.1.2.3")));
    assert!(!office.covers(&network("10.0.0.0/8")));
    assert!(!office.covers(&network("10.2.0.0/24")));
    assert!(!office.covers(&network("fd00::/64")));

    let halves = IpNetworkSet::new([network("10.0.0.128/25"), network("10.0.0.0/25")]);
    assert!(halves.covers(&network("10.0.0.0/24")));
    assert!(!halves.covers(&network("10.0.0.0/23")));
    let gap = IpNetworkSet::new([network("10.0.0.0/26"), network("10.0.0.128/25")]);
    assert!(!gap.covers(&network("10.0.0.0/24")));

    let everything = IpNetworkSet::new([network("0.0.0.0/0"), network("::/0")]);
    assert!(everything.covers(&network("192.168.0.0/16")));
    assert!(everything.covers(&network("fd00::/8")));
    assert!(!IpNetworkSet::new([network("0.0.0.0/0")]).covers(&network("fd00::/8")));
    assert!(!IpNetworkSet::default().covers(&network("10.0.0.1")));
}
