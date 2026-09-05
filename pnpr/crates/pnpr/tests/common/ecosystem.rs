//! Shared fixtures for the Cargo and Python surface tests: a stock npm
//! graph plus one hosted and one upstream registry of the ecosystem under
//! test, all fronted by the `main` router, and small response helpers.

use axum::body::{Body, to_bytes};
use pnpr::{
    AccessList, Config, Ecosystem, HostedConfig, PackagePattern, PackageRules, Registries,
    Registry, Teams, UpstreamConfig,
};
use reqwest::header::HeaderMap;
use sha2::{Digest, Sha256};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::{Path, PathBuf},
    time::Duration,
};

pub const PUBLIC_URL: &str = "http://pnpr.test";

/// The hosted registry of the ecosystem under test.
#[derive(Clone, Copy)]
pub struct HostedSource<'spec> {
    pub name: &'spec str,
    pub org: &'spec str,
    /// The registry-level default `access:`.
    pub access: &'spec str,
    /// The exact names the registry claims.
    pub packages: &'spec [&'spec str],
}

/// [`Config::proxy`] with `hosted` and an upstream registry named
/// `upstream.0` at `upstream.1`, both of `ecosystem`, added to the stock
/// `main` router beside the npm registries. `/<ecosystem>/...` is then the
/// default-target form and `/<ecosystem>/~<name>/...` the named form.
pub fn mixed_router_config(
    storage: PathBuf,
    ecosystem: Ecosystem,
    hosted: HostedSource<'_>,
    upstream: (&str, &str),
) -> Config {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let mut config = Config::proxy(listen, storage);
    config.public_url = PUBLIC_URL.to_string();
    config.packument_ttl = Duration::from_mins(1);
    config.hosted.insert(
        hosted.name.to_string(),
        HostedConfig {
            org: hosted.org.to_string(),
            rules: PackageRules::new(Vec::new(), Some(AccessList::from_tokens([hosted.access]))),
            teams: Teams::default(),
        },
    );
    config.upstreams.insert(
        upstream.0.to_string(),
        UpstreamConfig::with_defaults(upstream.1.to_string(), HeaderMap::new()),
    );
    let claimed = hosted
        .packages
        .iter()
        .map(|name| PackagePattern::parse(name).expect("package name is a valid pattern"))
        .collect();
    let mut graph: indexmap::IndexMap<String, Registry> = config
        .registries
        .names()
        .map(|name| (name.to_string(), config.registries.get(name).unwrap().clone()))
        .collect();
    graph.insert(hosted.name.to_string(), Registry::Hosted { patterns: claimed });
    graph.insert(upstream.0.to_string(), Registry::Upstream { patterns: vec![] });
    graph.insert(
        "main".to_string(),
        Registry::Router {
            sources: ["local", "npmjs", hosted.name, upstream.0].map(str::to_string).to_vec(),
        },
    );
    let registries = Registries::new(graph, Some("main".to_string()))
        .with_ecosystem(hosted.name, ecosystem)
        .with_ecosystem(upstream.0, ecosystem);
    registries.validate().expect("mixed graph is valid");
    config.registries = registries;
    config
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub async fn body_bytes(body: Body) -> Vec<u8> {
    to_bytes(body, usize::MAX).await.expect("read body").to_vec()
}

/// The first file named `filename` under `root`, at any depth.
pub fn find_file(root: &Path, filename: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(root).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_file(&path, filename) {
                return Some(found);
            }
        } else if entry.file_name() == filename {
            return Some(path);
        }
    }
    None
}

pub async fn assert_cache_tracks_metadata(ecosystem: Ecosystem) {
    use axum::http::{Request, StatusCode};
    use pnpr::{AuthState, router_with_auth};
    use serde_json::json;
    use tempfile::TempDir;
    use tower::ServiceExt;

    let mut upstream = mockito::Server::new_async().await;
    let (name, filename, index_path, download_path) = match ecosystem {
        Ecosystem::Cargo => (
            "serde",
            "serde-1.0.0.crate",
            "/se/rd/serde",
            "/cargo/api/v1/crates/serde/1.0.0/download",
        ),
        Ecosystem::Pypi => (
            "requests",
            "requests-1.0.0-py3-none-any.whl",
            "/requests/",
            "/pypi/files/requests/requests-1.0.0-py3-none-any.whl",
        ),
        Ecosystem::Npm => unreachable!(),
    };
    upstream
        .mock("GET", "/config.json")
        .with_body(
            json!({ "dl": format!("{}/artifact/{{crate}}/{{version}}", upstream.url()) })
                .to_string(),
        )
        .create_async()
        .await;
    let artifact_path = match ecosystem {
        Ecosystem::Cargo => format!("/artifact/{name}/1.0.0"),
        _ => "/artifact".to_string(),
    };
    let tmp = TempDir::new().unwrap();
    let mut config = mixed_router_config(
        tmp.path().to_path_buf(),
        ecosystem,
        HostedSource { name: "hosted", org: "hosted", access: "$all", packages: &["demo"] },
        ("upstream", &upstream.url()),
    );
    config.packument_ttl = Duration::ZERO;
    let app = router_with_auth(config, AuthState::in_memory());
    for bytes in [b"old artifact".as_slice(), b"new artifact".as_slice()] {
        let entry = match ecosystem {
            Ecosystem::Cargo => json!({ "name": name, "vers": "1.0.0", "deps": [], "cksum": sha256_hex(bytes), "features": {}, "yanked": false }).to_string(),
            _ => json!({ "meta": { "api-version": "1.0" }, "name": name, "files": [{ "filename": filename, "url": format!("{}/artifact", upstream.url()), "hashes": { "sha256": sha256_hex(bytes) } }] }).to_string(),
        };
        let index =
            upstream.mock("GET", index_path).with_body(entry).expect(2).create_async().await;
        let artifact = upstream
            .mock("GET", artifact_path.as_str())
            .with_body(bytes)
            .expect(1)
            .create_async()
            .await;
        for _ in 0..2 {
            let response = app
                .clone()
                .oneshot(Request::get(download_path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(body_bytes(response.into_body()).await, bytes);
        }
        index.assert_async().await;
        artifact.assert_async().await;
        index.remove_async().await;
        artifact.remove_async().await;
    }
    let empty = match ecosystem {
        Ecosystem::Cargo => String::new(),
        _ => json!({ "meta": { "api-version": "1.0" }, "name": name, "files": [] }).to_string(),
    };
    let index = upstream.mock("GET", index_path).with_body(empty).expect(1).create_async().await;
    let response =
        app.oneshot(Request::get(download_path).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    index.assert_async().await;
}
