use super::{AuthHeaders, PackageTag, PackageVersion, ThrottledClient};

/// [`PackageVersion::fetch_from_registry`] must attach the
/// registry-keyed `Authorization` header on every tag GET, just
/// like [`crate::Package::fetch_from_registry`].
#[tokio::test]
async fn fetch_from_registry_attaches_authorization_header() {
    let mut server = mockito::Server::new_async().await;
    let body = r#"{
        "name": "acme",
        "version": "1.0.0",
        "dist": {
            "integrity": "sha512-AAAA",
            "shasum": "0000000000000000000000000000000000000000",
            "tarball": "https://registry.test/acme-1.0.0.tgz"
        }
    }"#;
    let mock = server
        .mock("GET", "/acme/latest")
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

    let pkg_version = PackageVersion::fetch_from_registry(
        "acme",
        PackageTag::Latest,
        &client,
        &registry,
        &auth_headers,
    )
    .await
    .expect("server should accept the request once the bearer header is attached");
    assert_eq!(pkg_version.name, "acme");
    mock.assert_async().await;
}

/// The abbreviated registry response (`application/vnd.npm.install-v1+json`)
/// carries `optionalDependencies` and `peerDependenciesMeta` for any
/// package that publishes them. Both must round-trip through
/// [`PackageVersion`] so the resolver's `extract_children` reads the
/// optional-dep edges and `extract_peer_dependencies` reads the
/// per-peer `optional` flag. Dropping either field silently treats
/// optional peers as required (auto-installed via
/// `autoInstallPeers`) and skips `optionalDependencies` entirely.
#[test]
fn deserializes_optional_dependencies_and_peer_dependencies_meta() {
    let body = r#"{
        "name": "unstorage",
        "version": "1.17.5",
        "dist": {
            "integrity": "sha512-AAAA",
            "shasum": "0000000000000000000000000000000000000000",
            "tarball": "https://registry.test/unstorage-1.17.5.tgz"
        },
        "peerDependencies": {
            "@vercel/kv": "^1 || ^2 || ^3",
            "ioredis": "^5.4.2"
        },
        "peerDependenciesMeta": {
            "@vercel/kv": { "optional": true },
            "ioredis": { "optional": true }
        },
        "optionalDependencies": {
            "sharp": "^0.34.0"
        }
    }"#;

    let pkg: PackageVersion =
        serde_json::from_str(body).expect("deserialize PackageVersion fixture");

    let optional = pkg.optional_dependencies.as_ref().expect("optionalDependencies present");
    assert_eq!(optional.get("sharp").map(String::as_str), Some("^0.34.0"));

    let peer_meta = pkg.peer_dependencies_meta.as_ref().expect("peerDependenciesMeta present");
    assert_eq!(peer_meta["@vercel/kv"].optional, Some(true));
    assert_eq!(peer_meta["ioredis"].optional, Some(true));

    // The JSON shape `serde_json::to_value(pkg)` produces feeds
    // `extract_children` / `extract_peer_dependencies` downstream;
    // both consume the camelCase keys verbatim.
    let value = serde_json::to_value(&pkg).expect("serialize PackageVersion");
    assert!(value.get("optionalDependencies").is_some_and(serde_json::Value::is_object));
    assert!(value.get("peerDependenciesMeta").is_some_and(serde_json::Value::is_object));
}
