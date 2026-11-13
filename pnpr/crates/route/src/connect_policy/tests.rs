use std::{net::SocketAddr, path::PathBuf};

use pnpr_config::{Config, IpNetwork, PublicRoute};
use pnpr_fixtures::MockConfigPresets;

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

#[test]
fn the_own_host_at_another_origin_is_not_fetchable() {
    for host in ["127.0.0.1", "localhost"] {
        let other = format!("http://{host}:9000/");
        let context = RouteContext::from_config(&config(
            &format!("http://{host}:4873"),
            &[other.as_str()],
            &[],
        ));
        assert!(context.allows_registry(&other), "{other} is on the allowlist");
        assert!(!context.allows_fetch(&format!("{other}foo")), "fetched {other}");
        assert!(!context.allows_route_host(&other), "{other} as a public route");
        assert!(context.allows_fetch(&format!("http://{host}:4873/~local/foo")), "{host}");
    }
}

#[test]
fn stored_destinations_never_reach_the_own_host_at_another_origin() {
    for host in ["127.0.0.1", "localhost", "104.16.0.5"] {
        let own = format!("http://{host}:4873");
        let config = config(&own, &[], &["127.0.0.0/8"]);
        let stored = RouteContext::from_config(&config);
        for url in [format!("{own}/pkg"), format!("http://{host}:9000/pkg")] {
            assert!(!stored.allows_stored_upstream_url(&url), "stored upstream {url}");
        }
        assert!(stored.allows_route_host(&format!("{own}/")), "{own} as a public route");
        assert!(!stored.allows_route_host(&format!("http://{host}:9000/")), "{host}:9000");
        assert!(stored.allows_stored_upstream_url("http://127.0.0.2:4874/pkg"), "{host}");
    }
}

#[test]
fn a_public_ip_public_url_is_reached_only_at_its_origin() {
    let own: std::net::IpAddr = "104.16.0.5".parse().unwrap();
    let stored = RouteContext::from_config(&config("http://104.16.0.5:4873", &[], &[]));
    assert!(!stored.allows_address("alias.example", own), "another name for the own address");
    assert!(stored.allows_address("104.16.0.5", own), "the hosted-package fetch");
    assert!(stored.allows_fetch("http://104.16.0.5:4873/~local/foo"));
    assert!(!stored.allows_stored_upstream_address(own));
    assert!(stored.allows_stored_upstream_address("104.16.0.6".parse().unwrap()));
}

#[test]
fn an_ipv4_mapped_spelling_of_the_own_address_is_the_own_host() {
    let mapped: std::net::IpAddr = "::ffff:104.16.0.5".parse().unwrap();
    let plain: std::net::IpAddr = "104.16.0.5".parse().unwrap();
    let spellings = [
        ("http://104.16.0.5:4873", "http://[::ffff:104.16.0.5]:9000/", mapped),
        ("http://[::ffff:104.16.0.5]:4873", "http://104.16.0.5:9000/", plain),
    ];
    for (own, other_port, address) in spellings {
        let stored = RouteContext::from_config(&config(own, &[], &[]));
        assert!(stored.is_own_host(other_port), "{own}: {other_port}");
        assert!(!stored.allows_stored_upstream_url(other_port), "{own}: {other_port}");
        assert!(!stored.allows_stored_upstream_address(address), "{own}: {address}");
        assert!(!stored.allows_route_host(other_port), "{own}: route {other_port}");
        assert!(!stored.allows_address("alias.example", address), "{own}: alias of {address}");
    }
}

#[test]
fn a_terminal_dot_does_not_make_another_host() {
    let spellings = [
        ("http://registry.corp.example:4873", "http://registry.corp.example.:9000/"),
        ("http://registry.corp.example.:4873", "http://Registry.Corp.Example:9000/"),
    ];
    for (own, other_port) in spellings {
        let stored = RouteContext::from_config(&config(own, &[], &[]));
        assert!(stored.is_own_host(other_port), "{own}: {other_port}");
        assert!(!stored.allows_stored_upstream_url(other_port), "{own}: {other_port}");
        assert!(!stored.allows_route_host(other_port), "{own}: route {other_port}");
        let file = RouteContext::from_config(&config(own, &[], &[]));
        for name in ["registry.corp.example", "registry.corp.example."] {
            assert!(file.allows_address(name, "10.1.2.3".parse().unwrap()), "{own}: {name}");
        }
    }
}
