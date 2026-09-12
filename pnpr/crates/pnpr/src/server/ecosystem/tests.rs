use super::upstream_fetch_guard;
use pnpr_config::{Config, PublicRoute, UpstreamConfig};
use reqwest::header::HeaderMap;
use std::{net::SocketAddr, path::PathBuf};
use url::Url;

#[test]
fn official_upstreams_allow_only_their_own_download_host() {
    let mut config =
        Config::proxy(SocketAddr::from(([127, 0, 0, 1], 4873)), PathBuf::from("unused"));
    config.route_policy.public.push(PublicRoute {
        registry: Some("https://approved.test/files/".to_string()),
        package: None,
    });
    for (base, allowed, denied) in [
        (
            "https://index.crates.io/",
            "https://static.crates.io/crates/demo/demo-1.0.0.crate",
            "https://files.pythonhosted.org/wheel.whl",
        ),
        (
            "https://pypi.org/simple",
            "https://files.pythonhosted.org/wheel.whl",
            "https://static.crates.io/crates/demo/demo-1.0.0.crate",
        ),
    ] {
        let upstream = UpstreamConfig::with_defaults(base.to_string(), HeaderMap::new());
        let guard = upstream_fetch_guard(&config, &upstream);
        assert!(guard(&Url::parse(allowed).unwrap()));
        assert!(guard(&Url::parse("https://approved.test/files/archive").unwrap()));
        for rejected in [
            denied,
            "http://169.254.169.254/metadata",
            "http://127.0.0.1/private",
            "https://attacker.test/archive",
            "https://approved.test/private",
            "https://token@approved.test/files/archive",
        ] {
            assert!(!guard(&Url::parse(rejected).unwrap()), "{rejected}");
        }
    }
}
