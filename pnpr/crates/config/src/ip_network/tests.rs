use super::IpNetwork;
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
            .contains(address("8.8.8.8"))
    );
}

#[test]
fn rejects_what_is_not_a_network() {
    for raw in ["", "localhost", "10.0.0.0/33", "::/129", "10.0.0.0/", "10.0.0.0/-1", "10.0.0/8"] {
        assert!(IpNetwork::parse(raw).is_err(), "accepted {raw:?}");
    }
}
