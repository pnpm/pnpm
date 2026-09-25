use std::{net::SocketAddr, path::PathBuf};

use pnpr_config::{Config, IpNetwork, PublicRoute};

use crate::RouteContext;

fn config(public_url: &str, public_routes: &[&str], allowed_private_networks: &[&str]) -> Config {
    let mut config = Config::proxy(
        "127.0.0.1:7677".parse::<SocketAddr>().unwrap(),
        PathBuf::from("/tmp/pnpr-route"),
    );
    config.http.public_url = public_url.to_string();
    config.routing.route_policy.public.extend(
        public_routes
            .iter()
            .map(|registry| PublicRoute { registry: Some((*registry).to_string()), package: None }),
    );
    config.routing.route_policy.allowed_private_networks = allowed_private_networks
        .iter()
        .map(|network| IpNetwork::parse(network).unwrap())
        .collect();
    config
}

/// <https://github.com/pnpm/pnpm/issues/12705>
#[test]
fn allows_address_admits_public_addresses_the_own_host_and_allowed_networks() {
    let context =
        RouteContext::from_config(&config("http://pnpr.internal:4873", &[], &["10.20.0.0/16"]));
    let registry = "registry.npmjs.org";

    assert!(context.allows_address(registry, "104.16.0.1".parse().unwrap()));
    assert!(context.allows_address(registry, "10.20.3.4".parse().unwrap()));
    assert!(context.allows_address("pnpr.internal", "127.0.0.1".parse().unwrap()));
    assert!(context.allows_address("PNPR.internal", "127.0.0.1".parse().unwrap()));
    for address in ["169.254.169.254", "127.0.0.1", "10.0.0.1", "192.168.1.1", "::1", "fd00::1"] {
        assert!(!context.allows_address(registry, address.parse().unwrap()), "admitted {address}");
    }
}

/// <https://github.com/pnpm/pnpm/issues/12705>
#[test]
fn allows_fetch_checks_an_ip_literal_host_against_the_connect_policy() {
    let routes = [
        "http://10.0.0.5:4873/",
        "http://169.254.169.254/",
        "http://[::1]:4873/",
        "http://[::ffff:169.254.169.254]/",
    ];
    let context = RouteContext::from_config(&config("http://127.0.0.1:4873", &routes, &[]));
    for route in routes {
        assert!(context.allows_registry(route), "{route} is on the allowlist");
        assert!(!context.allows_fetch(route), "fetched {route}");
    }
    assert!(context.allows_fetch("http://127.0.0.1:4873/~local/foo"));
    assert!(context.allows_fetch("https://registry.npmjs.org/foo"));
    assert!(!context.allows_fetch("https://evil.example/foo"));

    let context = RouteContext::from_config(&config(
        "http://127.0.0.1:4873",
        &routes,
        &["10.0.0.0/8", "::1"],
    ));
    assert!(context.allows_fetch("http://10.0.0.5:4873/foo"));
    assert!(context.allows_fetch("http://[::1]:4873/foo"));
    assert!(!context.allows_fetch("http://169.254.169.254/latest/meta-data/"));
}
