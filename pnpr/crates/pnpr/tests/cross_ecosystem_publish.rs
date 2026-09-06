//! Integration tests for `PUT /-/pnpr/v0/publish` — one publish transaction
//! carrying packages of more than one ecosystem. Static-mode (no upstream) to
//! keep the tests hermetic.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use pnpr::{
    AccessList, AuthState, Config, Ecosystem, HostedConfig, MaxUsers, PackagePattern, PackageRules,
    Registries, Registry, Teams, router_with_auth,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use ssri::{Algorithm, IntegrityOpts};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::PathBuf,
};
use tempfile::TempDir;
use tower::ServiceExt;

/// A static registry serving all three ecosystems: the stock `local` npm
/// registry, a hosted Cargo registry claiming `demo`, and a hosted Python
/// registry claiming `demo-pkg`, all behind the `main` router.
fn tri_ecosystem_config(storage: PathBuf) -> Config {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let mut config = Config::static_serve(listen, storage);
    config.public_url = "http://pnpr.test".to_string();
    config.auth.htpasswd.max_users = MaxUsers::Unlimited;
    for (name, org) in [("crates", "crates"), ("python", "python")] {
        config.hosted.insert(
            name.to_string(),
            HostedConfig {
                org: org.to_string(),
                rules: PackageRules::new(Vec::new(), Some(AccessList::from_tokens(["$all"]))),
                teams: Teams::default(),
            },
        );
    }
    let mut graph: indexmap::IndexMap<String, Registry> = config
        .registries
        .names()
        .map(|name| (name.to_string(), config.registries.get(name).unwrap().clone()))
        .collect();
    graph.insert(
        "crates".to_string(),
        Registry::Hosted { patterns: vec![PackagePattern::parse("demo").unwrap()] },
    );
    graph.insert(
        "python".to_string(),
        Registry::Hosted { patterns: vec![PackagePattern::parse("demo-pkg").unwrap()] },
    );
    graph.insert(
        "main".to_string(),
        Registry::Router { sources: ["local", "crates", "python"].map(str::to_string).to_vec() },
    );
    let registries = Registries::new(graph, Some("main".to_string()))
        .with_ecosystem("crates", Ecosystem::Cargo)
        .with_ecosystem("python", Ecosystem::Pypi);
    registries.validate().expect("the three-ecosystem graph is valid");
    config.registries = registries;
    config
}

async fn body_bytes(body: Body) -> Vec<u8> {
    to_bytes(body, usize::MAX).await.expect("read body").to_vec()
}

async fn body_json(body: Body) -> Value {
    serde_json::from_slice(&body_bytes(body).await).expect("body parses as JSON")
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn sri_sha512(bytes: &[u8]) -> String {
    let mut opts = IntegrityOpts::new().algorithm(Algorithm::Sha512);
    opts.input(bytes);
    opts.result().to_string()
}

fn publish_request(path: &str, body: &Value, token: Option<&str>) -> Request<Body> {
    let mut request = Request::put(path).header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    request.body(Body::from(serde_json::to_vec(body).unwrap())).unwrap()
}

/// An npm publish document, the same one the npm batch endpoint takes.
fn npm_entry(name: &str, version: &str, tarball: &[u8]) -> Value {
    let filename = format!("{name}-{version}.tgz");
    json!({
        "_id": name,
        "name": name,
        "dist-tags": { "latest": version },
        "versions": {
            version: {
                "name": name,
                "version": version,
                "dist": {
                    "tarball": format!("http://pnpr.test/{name}/-/{filename}"),
                    "integrity": sri_sha512(tarball),
                },
            },
        },
        "_attachments": {
            filename: {
                "content_type": "application/octet-stream",
                "data": BASE64.encode(tarball),
                "length": tarball.len(),
            },
        },
    })
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

fn cargo_entry(name: &str, version: &str, archive: &[u8]) -> Value {
    json!({
        "ecosystem": "cargo",
        "metadata": {
            "name": name,
            "vers": version,
            "deps": [],
            "features": {},
            "authors": ["someone"],
            "description": "A demo crate",
            "keywords": [],
            "categories": [],
            "license": "MIT",
            "badges": {},
        },
        "archive": BASE64.encode(archive),
    })
}

fn pypi_entry(name: &str, version: &str, filename: &str, content: &[u8]) -> Value {
    json!({
        "ecosystem": "pypi",
        "name": name,
        "version": version,
        "filetype": "bdist_wheel",
        "filename": filename,
        "requires_python": ">=3.9",
        "sha256_digest": sha256_hex(content),
        "content": BASE64.encode(content),
    })
}

async fn token_for(app: &axum::Router, username: &str) -> String {
    let path = format!("/-/user/org.couchdb.user:{username}");
    let body = json!({
        "_id": format!("org.couchdb.user:{username}"),
        "name": username,
        "password": "secret",
        "email": "foo@bar.net",
        "type": "user",
        "roles": [],
    });
    let response = app.clone().oneshot(publish_request(&path, &body, None)).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    body_json(response.into_body()).await["token"].as_str().unwrap().to_string()
}

const WHEEL: &str = "demo_pkg-1.0.0-py3-none-any.whl";

#[tokio::test]
async fn publishes_a_package_a_crate_and_a_wheel_in_one_transaction() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router_with_auth(tri_ecosystem_config(storage.clone()), AuthState::in_memory());
    let token = token_for(&app, "alice").await;
    let tarball = b"npm-tarball-bytes";
    let archive = crate_archive("demo", "0.1.0");
    let wheel = b"PK\x03\x04 pretend wheel";

    let body = json!({
        "packages": [
            npm_entry("mixed-pkg", "1.0.0", tarball),
            cargo_entry("demo", "0.1.0", &archive),
            pypi_entry("demo-pkg", "1.0.0", WHEEL, wheel),
        ],
    });
    let response = app
        .clone()
        .oneshot(publish_request("/-/pnpr/v0/publish", &body, Some(&token)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let packument = app
        .clone()
        .oneshot(Request::get("/npm/mixed-pkg").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(packument.status(), StatusCode::OK);
    assert_eq!(body_json(packument.into_body()).await["dist-tags"]["latest"], "1.0.0");
    let npm_tarball = app
        .clone()
        .oneshot(Request::get("/npm/mixed-pkg/-/mixed-pkg-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(npm_tarball.status(), StatusCode::OK);
    assert_eq!(body_bytes(npm_tarball.into_body()).await, tarball);

    let index = app
        .clone()
        .oneshot(Request::get("/cargo/index/de/mo/demo").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(index.status(), StatusCode::OK);
    let line = String::from_utf8(body_bytes(index.into_body()).await).unwrap();
    let entry: Value = serde_json::from_str(line.lines().next().unwrap()).unwrap();
    assert_eq!(entry["vers"], "0.1.0");
    assert_eq!(entry["cksum"], sha256_hex(&archive));
    let download = app
        .clone()
        .oneshot(
            Request::get("/cargo/api/v1/crates/demo/0.1.0/download").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(body_bytes(download.into_body()).await, archive);

    let page = app
        .clone()
        .oneshot(
            Request::get("/pypi/simple/demo-pkg/")
                .header("accept", "application/vnd.pypi.simple.v1+json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    assert_eq!(body_json(page.into_body()).await["files"][0]["filename"], WHEEL);
    let file = app
        .oneshot(Request::get(format!("/pypi/files/demo-pkg/{WHEEL}")).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(body_bytes(file.into_body()).await, wheel);

    // One transaction, and it left nothing behind.
    assert_eq!(journal_entries(&storage), Vec::<PathBuf>::new());
}

#[tokio::test]
async fn a_batch_with_one_bad_entry_publishes_none_of_it() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router_with_auth(tri_ecosystem_config(storage.clone()), AuthState::in_memory());
    let token = token_for(&app, "alice").await;
    let wheel = b"PK\x03\x04 pretend wheel";

    // The archive belongs to another version than the metadata claims, which
    // the Cargo surface refuses.
    let body = json!({
        "packages": [
            npm_entry("mixed-pkg", "1.0.0", b"npm-tarball-bytes"),
            cargo_entry("demo", "0.1.0", &crate_archive("demo", "9.9.9")),
            pypi_entry("demo-pkg", "1.0.0", WHEEL, wheel),
        ],
    });
    let response = app
        .clone()
        .oneshot(publish_request("/-/pnpr/v0/publish", &body, Some(&token)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    for path in ["/mixed-pkg", "/cargo/index/de/mo/demo", "/pypi/simple/demo-pkg/"] {
        let response =
            app.clone().oneshot(Request::get(path).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
    assert_eq!(staged_files(&storage), Vec::<PathBuf>::new());
    assert_eq!(journal_entries(&storage), Vec::<PathBuf>::new());
}

/// A blob whose immutable slot another writer already owns is the one thing a
/// batch cannot roll back: the bytes that won are someone's published release.
/// The package that lost is reported, and the rest of the batch stays
/// published. An npm packument is written even when a version drops out of
/// it, so this case is only visible in the blobs the transaction lost.
#[tokio::test]
async fn a_package_that_loses_its_blob_is_reported_and_the_rest_stays() {
    use object_store::{ObjectStoreExt, memory::InMemory, path::Path as ObjectPath};
    use pnpr::HostedStoreConfig;
    use std::sync::Arc;

    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let store = Arc::new(InMemory::new());
    // Another writer already published this tarball, with other bytes.
    store
        .put(
            &ObjectPath::from("mixed-pkg/mixed-pkg-1.0.0.tgz"),
            axum::body::Bytes::from_static(b"the winning tarball").into(),
        )
        .await
        .unwrap();
    let mut config = tri_ecosystem_config(storage.clone());
    config.hosted_store = HostedStoreConfig::ObjectStore {
        store: Arc::<InMemory>::clone(&store),
        prefix: String::new(),
    };
    let app = router_with_auth(config, AuthState::in_memory());
    let token = token_for(&app, "alice").await;

    let body = json!({
        "packages": [
            npm_entry("mixed-pkg", "1.0.0", b"the losing tarball"),
            cargo_entry("demo", "0.1.0", &crate_archive("demo", "0.1.0")),
        ],
    });
    let response = app
        .clone()
        .oneshot(publish_request("/-/pnpr/v0/publish", &body, Some(&token)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let reason = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert!(reason.contains("mixed-pkg"), "{reason}");

    let packument = app
        .clone()
        .oneshot(Request::get("/npm/mixed-pkg").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(packument.status(), StatusCode::OK);
    let packument = body_json(packument.into_body()).await;
    assert_eq!(packument["versions"], json!({}), "the version that lost is not advertised");
    let index = app
        .oneshot(Request::get("/cargo/index/de/mo/demo").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(index.status(), StatusCode::OK, "the crate beside it stays published");
    assert_eq!(
        store
            .get(&ObjectPath::from("mixed-pkg/mixed-pkg-1.0.0.tgz"))
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap(),
        "the winning tarball",
    );
}

/// The failure that matters most is the one after other packages have already
/// staged their blobs: an npm tarball that does not match its declared
/// integrity is caught while staging, and every blob staged before it has to
/// disappear with it.
#[tokio::test]
async fn a_failure_while_staging_takes_the_staged_blobs_with_it() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router_with_auth(tri_ecosystem_config(storage.clone()), AuthState::in_memory());
    let token = token_for(&app, "alice").await;
    let mut broken = npm_entry("mixed-pkg", "1.0.0", b"npm-tarball-bytes");
    broken["versions"]["1.0.0"]["dist"]["integrity"] = json!(sri_sha512(b"other bytes"));

    let body = json!({
        "packages": [
            cargo_entry("demo", "0.1.0", &crate_archive("demo", "0.1.0")),
            pypi_entry("demo-pkg", "1.0.0", WHEEL, b"PK\x03\x04 pretend wheel"),
            broken,
        ],
    });
    let response = app
        .clone()
        .oneshot(publish_request("/-/pnpr/v0/publish", &body, Some(&token)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let reason = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert!(reason.contains("EINTEGRITY"), "{reason}");

    for path in ["/mixed-pkg", "/cargo/index/de/mo/demo", "/pypi/simple/demo-pkg/"] {
        let response =
            app.clone().oneshot(Request::get(path).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
    assert_eq!(staged_files(&storage), Vec::<PathBuf>::new());
    assert_eq!(journal_entries(&storage), Vec::<PathBuf>::new());
}

/// A version that is already published stops the batch before anything is
/// staged, so the packages beside it are not published either.
#[tokio::test]
async fn a_duplicate_in_one_ecosystem_stops_the_whole_batch() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router_with_auth(tri_ecosystem_config(storage.clone()), AuthState::in_memory());
    let token = token_for(&app, "alice").await;
    let archive = crate_archive("demo", "0.1.0");

    let first = json!({ "packages": [cargo_entry("demo", "0.1.0", &archive)] });
    let response = app
        .clone()
        .oneshot(publish_request("/-/pnpr/v0/publish", &first, Some(&token)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let again = json!({
        "packages": [
            npm_entry("mixed-pkg", "1.0.0", b"npm-tarball-bytes"),
            cargo_entry("demo", "0.1.0", &archive),
        ],
    });
    let response = app
        .clone()
        .oneshot(publish_request("/-/pnpr/v0/publish", &again, Some(&token)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let packument =
        app.oneshot(Request::get("/npm/mixed-pkg").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(packument.status(), StatusCode::NOT_FOUND, "the npm package must not be published");
    assert_eq!(staged_files(&storage), Vec::<PathBuf>::new());
}

/// A body this endpoint cannot read is the client's mistake: `502` would tell
/// them a gateway is broken.
#[tokio::test]
async fn a_malformed_batch_is_a_bad_request() {
    let tmp = TempDir::new().unwrap();
    let app =
        router_with_auth(tri_ecosystem_config(tmp.path().to_path_buf()), AuthState::in_memory());
    let token = token_for(&app, "alice").await;

    for body in [
        json!({ "packages": "not an array" }),
        json!({ "packages": [] }),
        json!({ "packages": [{ "ecosystem": "cargo", "archive": "not base64!" }] }),
        json!({ "packages": [{ "ecosystem": "brew", "name": "demo" }] }),
    ] {
        let response = app
            .clone()
            .oneshot(publish_request("/-/pnpr/v0/publish", &body, Some(&token)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{body}");
    }
}

/// `ecosystem` routes the entry; it is not part of the document a reader gets.
#[tokio::test]
async fn a_spelled_out_npm_entry_does_not_leak_its_routing_field() {
    let tmp = TempDir::new().unwrap();
    let app =
        router_with_auth(tri_ecosystem_config(tmp.path().to_path_buf()), AuthState::in_memory());
    let token = token_for(&app, "alice").await;
    let mut entry = npm_entry("mixed-pkg", "1.0.0", b"npm-tarball-bytes");
    entry["ecosystem"] = json!("npm");

    let response = app
        .clone()
        .oneshot(publish_request(
            "/-/pnpr/v0/publish",
            &json!({ "packages": [entry] }),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let packument =
        app.oneshot(Request::get("/npm/mixed-pkg").body(Body::empty()).unwrap()).await.unwrap();
    let packument = body_json(packument.into_body()).await;
    assert_eq!(packument["versions"]["1.0.0"]["version"], "1.0.0");
    assert!(packument.get("ecosystem").is_none(), "{packument}");
}

/// A digest is hexadecimal, and an uploader may spell it in either case.
#[tokio::test]
async fn an_uppercase_digest_is_accepted() {
    let tmp = TempDir::new().unwrap();
    let app =
        router_with_auth(tri_ecosystem_config(tmp.path().to_path_buf()), AuthState::in_memory());
    let token = token_for(&app, "alice").await;
    let wheel = b"PK\x03\x04 pretend wheel";
    let mut entry = pypi_entry("demo-pkg", "1.0.0", WHEEL, wheel);
    entry["sha256_digest"] = json!(sha256_hex(wheel).to_uppercase());

    let response = app
        .oneshot(publish_request(
            "/-/pnpr/v0/publish",
            &json!({ "packages": [entry] }),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn an_anonymous_batch_publishes_nothing() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router_with_auth(tri_ecosystem_config(storage.clone()), AuthState::in_memory());

    let body = json!({ "packages": [npm_entry("mixed-pkg", "1.0.0", b"npm-tarball-bytes")] });
    let response =
        app.clone().oneshot(publish_request("/-/pnpr/v0/publish", &body, None)).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let packument =
        app.oneshot(Request::get("/npm/mixed-pkg").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(packument.status(), StatusCode::NOT_FOUND);
}

/// The same package twice in one batch would make the second entry's merge
/// depend on the first's uncommitted result; the same name in two ecosystems
/// is two different packages and is fine.
#[tokio::test]
async fn a_repeated_package_is_refused_but_a_shared_name_across_ecosystems_is_not() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router_with_auth(tri_ecosystem_config(storage.clone()), AuthState::in_memory());
    let token = token_for(&app, "alice").await;

    let repeated = json!({
        "packages": [
            npm_entry("mixed-pkg", "1.0.0", b"one"),
            npm_entry("mixed-pkg", "2.0.0", b"two"),
        ],
    });
    let response = app
        .clone()
        .oneshot(publish_request("/-/pnpr/v0/publish", &repeated, Some(&token)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let reason = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert!(reason.contains("duplicate"), "{reason}");

    let shared_name = json!({
        "packages": [
            npm_entry("demo", "1.0.0", b"an npm package called demo"),
            cargo_entry("demo", "0.1.0", &crate_archive("demo", "0.1.0")),
        ],
    });
    let response = app
        .oneshot(publish_request("/-/pnpr/v0/publish", &shared_name, Some(&token)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
}

/// The transactions the publish journal still holds. A journal a publish
/// never reached has no directory at all.
fn journal_entries(storage: &std::path::Path) -> Vec<PathBuf> {
    match std::fs::read_dir(storage.join(".pnpr-journal")) {
        Ok(entries) => entries.map(|entry| entry.unwrap().path()).collect(),
        Err(_) => Vec::new(),
    }
}

/// Every staged file left under `root`, by the marker the storage layer puts
/// in a staged file's name.
fn staged_files(root: &std::path::Path) -> Vec<PathBuf> {
    let mut staged = Vec::new();
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            staged.extend(staged_files(&path));
        } else if path.file_name().is_some_and(|name| name.to_string_lossy().contains(".tmp.")) {
            staged.push(path);
        }
    }
    staged
}
