use std::collections::HashMap;

use node_semver::Version;
use pretty_assertions::assert_eq;

use super::{AuthHeaders, Package, PackageVersion, ThrottledClient};
use crate::package_distribution::PackageDistribution;

#[test]
pub fn package_version_should_include_peers() {
    let mut dependencies = HashMap::<String, String>::new();
    dependencies.insert("fastify".to_string(), "1.0.0".to_string());
    let mut peer_dependencies = HashMap::<String, String>::new();
    peer_dependencies.insert("fast-querystring".to_string(), "1.0.0".to_string());
    let version = PackageVersion {
        name: "".to_string(),
        version: Version::parse("1.0.0").unwrap(),
        dist: PackageDistribution::default(),
        dependencies: Some(dependencies),
        dev_dependencies: None,
        peer_dependencies: Some(peer_dependencies),
    };

    let dependencies = |peer| version.dependencies(peer).collect::<HashMap<_, _>>();
    assert!(dependencies(false).contains_key("fastify"));
    assert!(!dependencies(false).contains_key("fast-querystring"));
    assert!(dependencies(true).contains_key("fastify"));
    assert!(dependencies(true).contains_key("fast-querystring"));
    assert!(!dependencies(true).contains_key("hello-world"));
}

#[test]
pub fn serialized_according_to_params() {
    let version = PackageVersion {
        name: "".to_string(),
        version: Version { major: 3, minor: 2, patch: 1, build: vec![], pre_release: vec![] },
        dist: PackageDistribution::default(),
        dependencies: None,
        dev_dependencies: None,
        peer_dependencies: None,
    };

    assert_eq!(version.serialize(true), "3.2.1");
    assert_eq!(version.serialize(false), "^3.2.1");
}

/// [`Package::fetch_from_registry`] must attach the registry-keyed
/// `Authorization` header on every metadata GET, even for the
/// abbreviated install-v1 endpoint. `mockito::Matcher::Exact`
/// rejects the request unless the header arrives verbatim, so a
/// missing or wrong header would 501 the request and propagate as
/// a deserialization error.
#[tokio::test]
async fn fetch_from_registry_attaches_authorization_header() {
    let mut server = mockito::Server::new_async().await;
    let body = r#"{"name":"acme","dist-tags":{"latest":"1.0.0"},"versions":{}}"#;
    let mock = server
        .mock("GET", "/acme")
        .match_header("authorization", "Bearer top-secret")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .expect(1)
        .create_async()
        .await;

    let registry = format!("{}/", server.url());
    let client = ThrottledClient::default();
    let auth_headers = AuthHeaders::from_creds_map(
        [(pacquet_network::nerf_dart(&registry), "Bearer top-secret".to_owned())],
        None,
    );

    let pkg = Package::fetch_from_registry("acme", &client, &registry, &auth_headers)
        .await
        .expect("server should accept the request once the bearer header is attached");
    assert_eq!(pkg.name, "acme");
    mock.assert_async().await;
}

fn package_with_versions(name: &str, versions: &[&str], latest: &str) -> Package {
    let versions_map = versions
        .iter()
        .map(|v| {
            (
                v.to_string(),
                PackageVersion {
                    name: name.to_string(),
                    version: Version::parse(v).unwrap(),
                    dist: PackageDistribution::default(),
                    dependencies: None,
                    dev_dependencies: None,
                    peer_dependencies: None,
                },
            )
        })
        .collect();
    let mut dist_tags = HashMap::new();
    dist_tags.insert("latest".to_string(), latest.to_string());
    Package { name: name.to_string(), dist_tags, versions: versions_map, mutex: Default::default() }
}

/// `Package` equality is by `name` only; the mutex and versions
/// HashMap (whose iteration order is non-deterministic) are
/// excluded. Two packages with the same name compare equal even
/// when their `versions` maps differ — this lets call sites
/// dedupe in-flight metadata fetches against the package name.
#[test]
fn package_equality_compares_by_name_only() {
    let lhs = package_with_versions("acme", &["1.0.0"], "1.0.0");
    let rhs = package_with_versions("acme", &["2.0.0"], "2.0.0");
    assert_eq!(lhs, rhs);

    let other = package_with_versions("widget", &["1.0.0"], "1.0.0");
    assert!(lhs != other);
}

/// `latest()` looks up the version named under the `latest`
/// dist-tag and returns the corresponding `PackageVersion`.
#[test]
fn latest_returns_version_pointed_to_by_dist_tag() {
    let pkg = package_with_versions("acme", &["1.0.0", "2.0.0", "3.0.0"], "2.0.0");
    let latest = pkg.latest();
    assert_eq!(latest.version.to_string(), "2.0.0");
}

/// `pinned_version` picks the highest version inside the given
/// range, mirroring `node-semver`'s `maxSatisfying`.
#[test]
fn pinned_version_picks_highest_matching() {
    let pkg = package_with_versions("acme", &["1.0.0", "1.2.0", "1.5.3", "2.0.0"], "2.0.0");
    let picked =
        pkg.pinned_version("^1.0.0").expect("at least one 1.x version satisfies the range");
    assert_eq!(picked.version.to_string(), "1.5.3");
}

/// `pinned_version` returns `None` when no version satisfies the
/// range, rather than panicking or falling back to `latest`.
#[test]
fn pinned_version_returns_none_when_no_match() {
    let pkg = package_with_versions("acme", &["1.0.0", "1.2.0"], "1.2.0");
    assert!(pkg.pinned_version("^2.0.0").is_none());
}
