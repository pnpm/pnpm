//! The Cargo registry surface: sparse index, downloads, `cargo publish`,
//! yank, and proxying an upstream sparse index and its downloads.

use axum::{
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode, header},
};
use pnpr::{
    AccessList, AuthState, Config, Ecosystem, HostedConfig, PackagePattern, PackageRules,
    Registries, Registry, Teams, UpstreamConfig, router_with_auth,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::Write as _,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::{Path, PathBuf},
    time::Duration,
};
use tempfile::TempDir;
use tower::ServiceExt;

const PUBLIC_URL: &str = "http://pnpr.test";

/// A registry graph with a hosted Cargo registry (`crates`, claiming `demo`
/// and `inflector`) and a Cargo upstream (`cratesio`, everything else) at
/// `upstream_url`, both added to the stock `main` router beside the npm
/// registries, so `/cargo/...` is the default-target form and
/// `/cargo/~crates/...` the named form.
fn cargo_config(storage: PathBuf, upstream_url: &str, hosted_access: &str) -> Config {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let mut config = Config::proxy(listen, storage);
    config.public_url = PUBLIC_URL.to_string();
    config.packument_ttl = Duration::from_mins(1);
    config.hosted.insert(
        "crates".to_string(),
        HostedConfig {
            org: "crates".to_string(),
            rules: PackageRules::new(Vec::new(), Some(AccessList::from_tokens([hosted_access]))),
            teams: Teams::default(),
        },
    );
    config.upstreams.insert(
        "cratesio".to_string(),
        UpstreamConfig::with_defaults(upstream_url.to_string(), HeaderMap::new()),
    );
    let claimed = ["demo", "inflector"]
        .into_iter()
        .map(|name| PackagePattern::parse(name).expect("crate name is a valid pattern"))
        .collect();
    // Keep the stock npm graph (`local`, `npmjs`, `main`) and add the Cargo one.
    let mut graph: indexmap::IndexMap<String, Registry> = config
        .registries
        .names()
        .map(|name| (name.to_string(), config.registries.get(name).unwrap().clone()))
        .collect();
    graph.insert("crates".to_string(), Registry::Hosted { patterns: claimed });
    graph.insert("cratesio".to_string(), Registry::Upstream { patterns: vec![] });
    // One router fronts every ecosystem: `/cargo/...` requests only see the
    // Cargo sources, `/webpack`-style npm requests only the npm ones.
    graph.insert(
        "main".to_string(),
        Registry::Router {
            sources: ["local", "npmjs", "crates", "cratesio"].map(str::to_string).to_vec(),
        },
    );
    let registries = Registries::new(graph, Some("main".to_string()))
        .with_ecosystem("crates", Ecosystem::Cargo)
        .with_ecosystem("cratesio", Ecosystem::Cargo);
    registries.validate().expect("cargo graph is valid");
    config.registries = registries;
    config
}

fn crate_archive(name: &str, version: &str) -> Vec<u8> {
    let root = format!("{name}-{version}");
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut builder = tar::Builder::new(encoder);
    for (path, contents) in [
        ("Cargo.toml", format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\n")),
        ("src/lib.rs", "pub fn demo() {}\n".to_string()),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, format!("{root}/{path}"), contents.as_bytes()).unwrap();
    }
    builder.into_inner().unwrap().finish().unwrap()
}

fn publish_body(metadata: &Value, archive: &[u8]) -> Vec<u8> {
    let metadata = serde_json::to_vec(metadata).unwrap();
    let mut body = Vec::new();
    body.write_all(&(metadata.len() as u32).to_le_bytes()).unwrap();
    body.write_all(&metadata).unwrap();
    body.write_all(&(archive.len() as u32).to_le_bytes()).unwrap();
    body.write_all(archive).unwrap();
    body
}

fn metadata(name: &str, version: &str) -> Value {
    json!({
        "name": name,
        "vers": version,
        "deps": [{
            "name": "serde",
            "version_req": "^1",
            "features": ["derive"],
            "optional": false,
            "default_features": true,
            "target": null,
            "kind": "normal",
            "registry": null,
            "explicit_name_in_toml": null,
        }],
        "features": {},
        "authors": ["someone"],
        "description": "A demo crate",
        "documentation": null,
        "homepage": null,
        "readme": null,
        "readme_file": null,
        "keywords": [],
        "categories": [],
        "license": "MIT",
        "license_file": null,
        "repository": null,
        "badges": {},
        "links": null,
        "rust_version": null,
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

async fn body_bytes(body: Body) -> Vec<u8> {
    to_bytes(body, usize::MAX).await.expect("read body").to_vec()
}

/// `cargo` sends a registry token as the bare header value, with no scheme.
fn publish_request(token: Option<&str>, body: Vec<u8>) -> Request<Body> {
    let mut request = Request::put("/cargo/api/v1/crates/new");
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, token);
    }
    request.body(Body::from(body)).unwrap()
}

fn find_file(root: &Path, filename: &str) -> Option<PathBuf> {
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

#[tokio::test]
async fn config_json_points_downloads_and_the_api_back_at_the_registry() {
    let tmp = TempDir::new().unwrap();
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), "http://upstream.invalid/", "$all"),
        AuthState::in_memory(),
    );

    let response = app
        .clone()
        .oneshot(Request::get("/cargo/index/config.json").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let config: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(
        config,
        json!({ "dl": "http://pnpr.test/cargo/api/v1/crates", "api": "http://pnpr.test/cargo" }),
    );

    // A registry addressed by name advertises its own endpoint.
    for registry in ["crates", "main"] {
        let response = app
            .clone()
            .oneshot(
                Request::get(format!("/cargo/~{registry}/index/config.json"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{registry}");
        assert_eq!(response.headers()[header::CACHE_CONTROL], "private, no-store");
        let config: Value =
            serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
        assert_eq!(config["api"], format!("http://pnpr.test/cargo/~{registry}"));
    }

    // The old npm-only address of a registry has no sparse index.
    let response = app
        .oneshot(Request::get("/~crates/index/config.json").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn publish_then_resolve_and_download_a_hosted_crate() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), "http://upstream.invalid/", "$all"),
        auth,
    );
    let archive = crate_archive("demo", "0.1.0");

    let response = app
        .clone()
        .oneshot(publish_request(Some(&token), publish_body(&metadata("demo", "0.1.0"), &archive)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(body["warnings"]["other"], json!([]));

    // The sparse-index file: one JSON line per version.
    let response = app
        .clone()
        .oneshot(Request::get("/cargo/index/de/mo/demo").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    // A public crate through the default target stays cacheable.
    assert!(response.headers().get(header::CACHE_CONTROL).is_none());
    let index = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    let lines: Vec<Value> = index.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["name"], "demo");
    assert_eq!(lines[0]["vers"], "0.1.0");
    assert_eq!(lines[0]["cksum"], sha256_hex(&archive));
    assert_eq!(lines[0]["yanked"], false);
    assert_eq!(lines[0]["deps"][0]["name"], "serde");
    assert_eq!(lines[0]["deps"][0]["req"], "^1");

    // The archive is served byte-for-byte from the download endpoint.
    let response = app
        .clone()
        .oneshot(
            Request::get("/cargo/api/v1/crates/demo/0.1.0/download").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, archive);

    // Storage layout: the hosted org namespace, keyed by the lowercase name.
    assert!(tmp.path().join("crates/demo/demo-0.1.0.crate").is_file());
    assert!(tmp.path().join("crates/demo/package.json").is_file());

    // The crate is reachable at its one sparse-index path only.
    for wrong in ["/cargo/index/3/d/demo", "/cargo/index/de/mo/Demo", "/cargo/index/DE/MO/demo"] {
        let response =
            app.clone().oneshot(Request::get(wrong).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{wrong}");
    }
    // The same crate through the named registry, caller-scoped.
    let response = app
        .clone()
        .oneshot(Request::get("/cargo/~crates/index/de/mo/demo").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "private, no-store");
    // npm-shaped paths mean nothing on the Cargo surface.
    let response = app
        .clone()
        .oneshot(Request::get("/cargo/demo").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = app
        .oneshot(
            Request::get("/cargo/api/v1/crates/demo/0.2.0/download").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn publish_requires_a_token_and_refuses_duplicates_and_bad_archives() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), "http://upstream.invalid/", "$all"),
        auth,
    );
    let archive = crate_archive("demo", "0.1.0");
    let body = publish_body(&metadata("demo", "0.1.0"), &archive);

    // Anonymous: 401 in the crates API's JSON error shape.
    let response = app.clone().oneshot(publish_request(None, body.clone())).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let errors: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert!(errors["errors"][0]["detail"].is_string(), "{errors}");

    let response = app.clone().oneshot(publish_request(Some(&token), body.clone())).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Re-publishing the same version is refused.
    let response = app.clone().oneshot(publish_request(Some(&token), body)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let errors: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert!(
        errors["errors"][0]["detail"].as_str().unwrap().contains("already uploaded"),
        "{errors}",
    );

    // The archive must hold the crate the metadata names.
    let mismatched = publish_body(&metadata("demo", "0.2.0"), &crate_archive("other", "0.2.0"));
    let response = app.clone().oneshot(publish_request(Some(&token), mismatched)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(!tmp.path().join("crates/demo/demo-0.2.0.crate").exists());

    // A name the hosted registry does not claim routes to the upstream, where
    // nothing can be published.
    let unclaimed = publish_body(&metadata("serde", "1.0.0"), &crate_archive("serde", "1.0.0"));
    let response = app.clone().oneshot(publish_request(Some(&token), unclaimed)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let errors: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert!(
        errors["errors"][0]["detail"].as_str().unwrap().contains("upstream registry"),
        "{errors}",
    );

    // A malformed body is a 400, not a 500.
    let response = app.oneshot(publish_request(Some(&token), vec![1, 2, 3])).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn yank_and_unyank_flip_the_index_entry() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), "http://upstream.invalid/", "$all"),
        auth,
    );
    let response = app
        .clone()
        .oneshot(publish_request(
            Some(&token),
            publish_body(&metadata("demo", "0.1.0"), &crate_archive("demo", "0.1.0")),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let yanked_flag = |app: axum::Router| async move {
        let response = app
            .oneshot(Request::get("/cargo/index/de/mo/demo").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let index = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
        let line: Value = serde_json::from_str(index.lines().next().unwrap()).unwrap();
        line["yanked"].as_bool().unwrap()
    };

    // Anonymous yank is refused.
    let response = app
        .clone()
        .oneshot(
            Request::delete("/cargo/api/v1/crates/demo/0.1.0/yank").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(!yanked_flag(app.clone()).await);

    let response = app
        .clone()
        .oneshot(
            Request::delete("/cargo/api/v1/crates/demo/0.1.0/yank")
                .header(header::AUTHORIZATION, &token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(yanked_flag(app.clone()).await);

    // A yanked version stays downloadable, as on crates.io.
    let response = app
        .clone()
        .oneshot(
            Request::get("/cargo/api/v1/crates/demo/0.1.0/download").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(
            Request::put("/cargo/api/v1/crates/demo/0.1.0/unyank")
                .header(header::AUTHORIZATION, &token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!yanked_flag(app.clone()).await);

    // An unknown version is a 404.
    let response = app
        .oneshot(
            Request::delete("/cargo/api/v1/crates/demo/9.9.9/yank")
                .header(header::AUTHORIZATION, &token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn crate_names_are_case_insensitive_in_the_index_path() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), "http://upstream.invalid/", "$all"),
        auth,
    );
    let archive = crate_archive("Inflector", "0.11.4");
    let response = app
        .clone()
        .oneshot(publish_request(
            Some(&token),
            publish_body(&metadata("Inflector", "0.11.4"), &archive),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // `cargo` requests the lowercase path; the entry keeps the published case.
    let response = app
        .clone()
        .oneshot(Request::get("/cargo/index/in/fl/inflector").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let index = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    let line: Value = serde_json::from_str(index.lines().next().unwrap()).unwrap();
    assert_eq!(line["name"], "Inflector");

    // Downloads use the name as the index spells it.
    let response = app
        .oneshot(
            Request::get("/cargo/api/v1/crates/Inflector/0.11.4/download")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, archive);
}

#[tokio::test]
async fn private_hosted_registry_advertises_auth_required_and_masks_anonymous_reads() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), "http://upstream.invalid/", "$authenticated"),
        auth,
    );
    let response = app
        .clone()
        .oneshot(Request::get("/cargo/index/config.json").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let config: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(config["auth-required"], true);

    let response = app
        .clone()
        .oneshot(publish_request(
            Some(&token),
            publish_body(&metadata("demo", "0.1.0"), &crate_archive("demo", "0.1.0")),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // The registry-level default masks the crate from anonymous callers.
    let response = app
        .clone()
        .oneshot(Request::get("/cargo/index/de/mo/demo").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = app
        .clone()
        .oneshot(
            Request::get("/cargo/api/v1/crates/demo/0.1.0/download").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // With the raw token `cargo` sends once `auth-required` is set, both serve.
    for path in ["/cargo/index/de/mo/demo", "/cargo/api/v1/crates/demo/0.1.0/download"] {
        let response = app
            .clone()
            .oneshot(
                Request::get(path)
                    .header(header::AUTHORIZATION, &token)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
    }
}

#[tokio::test]
async fn proxies_the_sparse_index_and_verified_downloads_through_an_upstream() {
    let mut upstream = mockito::Server::new_async().await;
    let archive = crate_archive("serde", "1.0.0");
    let config_mock = upstream
        .mock("GET", "/config.json")
        .with_body(
            json!({ "dl": format!("{}/dl/{{crate}}/{{version}}", upstream.url()), "api": upstream.url() })
                .to_string(),
        )
        .expect(1)
        .create_async()
        .await;
    let index_line = json!({
        "name": "serde",
        "vers": "1.0.0",
        "deps": [],
        "cksum": sha256_hex(&archive),
        "features": {},
        "yanked": false,
        "v": 1,
    });
    let index_mock = upstream
        .mock("GET", "/se/rd/serde")
        .with_body(format!("{index_line}\n"))
        .expect(1)
        .create_async()
        .await;
    let download_mock = upstream
        .mock("GET", "/dl/serde/1.0.0")
        .with_body(archive.clone())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), &upstream.url(), "$all"),
        AuthState::in_memory(),
    );

    // The index file is proxied verbatim.
    let response = app
        .clone()
        .oneshot(Request::get("/cargo/index/se/rd/serde").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let index = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert_eq!(index.trim(), index_line.to_string());

    // The download is bound to the index checksum and cached.
    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(
                Request::get("/cargo/api/v1/crates/serde/1.0.0/download")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_bytes(response.into_body()).await, archive);
    }
    config_mock.assert_async().await;
    index_mock.assert_async().await;
    download_mock.assert_async().await;
    let cache = tmp.path().join(".pnpr-cache");
    assert!(find_file(&cache, "serde-1.0.0.crate").is_some(), "download is cached");

    // An unknown crate is a definitive 404, and the cache holds nothing for it.
    let missing = upstream.mock("GET", "/no/pe/nope").with_status(404).create_async().await;
    let response = app
        .oneshot(Request::get("/cargo/index/no/pe/nope").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    missing.assert_async().await;
}

#[tokio::test]
async fn a_download_that_fails_the_index_checksum_is_never_cached() {
    let mut upstream = mockito::Server::new_async().await;
    let archive = crate_archive("serde", "1.0.0");
    upstream
        .mock("GET", "/config.json")
        .with_body(json!({ "dl": format!("{}/dl", upstream.url()) }).to_string())
        .create_async()
        .await;
    upstream
        .mock("GET", "/se/rd/serde")
        .with_body(format!(
            "{}\n",
            json!({
                "name": "serde",
                "vers": "1.0.0",
                "deps": [],
                "cksum": sha256_hex(b"something else"),
                "features": {},
                "yanked": false,
            }),
        ))
        .create_async()
        .await;
    // A `dl` template without markers gets `/{crate}/{version}/download` appended.
    upstream.mock("GET", "/dl/serde/1.0.0/download").with_body(archive).create_async().await;

    let tmp = TempDir::new().unwrap();
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), &upstream.url(), "$all"),
        AuthState::in_memory(),
    );
    let response = app
        .oneshot(
            Request::get("/cargo/api/v1/crates/serde/1.0.0/download").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();
    // Streaming has started by the time the mismatch is known; the client
    // verifies the bytes itself, and the cache is never populated.
    let _ = body_bytes(response.into_body()).await;
    assert!(find_file(&tmp.path().join(".pnpr-cache"), "serde-1.0.0.crate").is_none());
}
