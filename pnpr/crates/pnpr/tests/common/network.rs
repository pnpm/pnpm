//! The connect policy exception the resolver suites need: their upstreams
//! are mock servers listening on loopback.

use pnpr::IpNetwork;

pub fn loopback_networks() -> Vec<IpNetwork> {
    ["127.0.0.0/8", "::1"]
        .into_iter()
        .map(IpNetwork::parse)
        .map(|parsed| parsed.expect("loopback network parses"))
        .collect()
}
