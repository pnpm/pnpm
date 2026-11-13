//! The registry admin API (`/-/pnpr/v0/admin/registries` and
//! `/-/pnpr/v0/admin/registry-revisions`) with `registriesManagedBy: api`:
//! writes, versions, revisions, who may make a change, the changes pnpr
//! refuses, and how replicas follow the stored graph.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use pnpr::{AuthState, Config, router_with_auth};
use pnpr_storage::{
    HostedDocumentVersion, Storage,
    metadata::{NewRegistryGraph, RegistryGraphWrite},
};
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tempfile::TempDir;
use tower::ServiceExt;

const CONFIG: &str = "\
auth:
  admins: [alice]
  registryAdmins: [root]
  users:
    maxUsers: 100
registryEnv: [PATH]
routes:
  allowedPrivateNetworks: ['10.0.0.0/8']
registries:
  local:
    type: hosted
    namespace: ''
    packages:
      '**': {}
  main:
    type: router
    sources: [local]
defaultRegistry: main
";

/// [`CONFIG`] without registries.
const EMPTY_CONFIG: &str = "\
auth:
  admins: [alice]
  registryAdmins: [root]
  users:
    maxUsers: 100
";

const DOCUMENT: &str = "/-/pnpr/v0/admin/registries";
const REVISIONS: &str = "/-/pnpr/v0/admin/registry-revisions";
const ACCOUNTS: &[(&str, &str)] =
    &[("root", "secret123"), ("alice", "secret123"), ("bob", "secret123")];

async fn load_config(dir: &Path) -> Config {
    let yaml = format!("storage: {}\n{CONFIG}", storage_dir(dir).display());
    crate::managed_registries::managed_config(&config_path(dir), &yaml, listen(), public_url())
        .await
}

/// The config [`load_config`] wrote, without storing its graph again.
fn reload_config(dir: &Path) -> Config {
    Config::from_yaml(&config_path(dir), listen(), public_url()).unwrap()
}

/// [`CONFIG`] with the registries in the config file.
fn load_file_config(dir: &Path) -> Config {
    let path = config_path(dir);
    std::fs::write(&path, format!("storage: {}\n{CONFIG}", storage_dir(dir).display())).unwrap();
    let yaml = std::fs::read_to_string(&path).unwrap().replace("registryEnv: [PATH]\n", "");
    std::fs::write(&path, yaml).unwrap();
    Config::from_yaml(&path, listen(), public_url()).unwrap()
}

fn config_path(dir: &Path) -> std::path::PathBuf {
    dir.join("config.yaml")
}

fn storage_dir(dir: &Path) -> std::path::PathBuf {
    let storage = dir.join("storage");
    std::fs::create_dir_all(&storage).unwrap();
    storage
}

fn listen() -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873))
}

fn public_url() -> Option<String> {
    Some("http://example.test".to_string())
}

struct Replica {
    app: axum::Router,
    root: String,
    alice: String,
    bob: String,
}

impl Replica {
    async fn new(config: Config) -> Self {
        let auth = crate::tokens::seeded_auth(&config, ACCOUNTS);
        Self::with_auth(config, auth).await
    }

    async fn with_auth(config: Config, auth: AuthState) -> Self {
        let app = router_with_auth(config, auth);
        let root = login(&app, "root").await;
        let alice = login(&app, "alice").await;
        let bob = login(&app, "bob").await;
        Self { app, root, alice, bob }
    }

    async fn send(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value, Option<String>) {
        send(&self.app, method, path, token, body, None).await
    }

    async fn as_root(&self, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let (status, payload, _) = self.send(method, path, Some(&self.root), body).await;
        (status, payload)
    }
}

async fn send(
    app: &axum::Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
    if_match: Option<&str>,
) -> (StatusCode, Value, Option<String>) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    if let Some(version) = if_match {
        request = request.header("if-match", format!(r#""{version}""#));
    }
    let body = body.map(|body| Body::from(body.to_string())).unwrap_or_else(Body::empty);
    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let etag = response
        .headers()
        .get("etag")
        .map(|etag| {
            etag.to_str()
                .unwrap()
                .trim_matches('"')
                .to_string()
        });
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null), etag)
}

async fn login(app: &axum::Router, username: &str) -> String {
    let body = json!({ "name": username, "password": "secret123", "type": "user", "roles": [] });
    let path = format!("/-/user/org.couchdb.user:{username}");
    let (status, payload, _) = send(app, "PUT", &path, None, Some(body), None).await;
    assert_eq!(status, StatusCode::CREATED, "{payload}");
    payload["token"]
        .as_str()
        .unwrap()
        .to_string()
}

fn upstream(url: &str) -> Value {
    json!({ "type": "upstream", "url": url, "public": true })
}

#[tokio::test]
async fn a_document_change_is_versioned_and_an_unchanged_one_stores_nothing() {
    let dir = TempDir::new().unwrap();
    let replica = Replica::new(load_config(dir.path()).await).await;

    let (status, document, version) =
        replica.send("GET", DOCUMENT, Some(&replica.root), None).await;
    assert_eq!((status, version.as_deref()), (StatusCode::OK, Some("1")));
    assert_eq!(document["defaultRegistry"], json!("main"));

    let (status, unchanged) = replica.as_root("PUT", DOCUMENT, Some(document.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unchanged["version"], json!("1"));
    assert_eq!(unchanged["changes"], json!({ "added": [], "removed": [], "changed": [] }));

    let mut next = document;
    next["registries"]["extra"] = json!({ "type": "hosted", "namespace": "extra" });
    let (status, changed) = replica.as_root("PUT", DOCUMENT, Some(next)).await;
    assert_eq!(status, StatusCode::OK, "{changed}");
    assert_eq!(changed["version"], json!("2"));
    assert_eq!(changed["changes"]["added"], json!(["extra"]));
    let (status, entry) = replica.as_root("GET", &format!("{DOCUMENT}/extra"), None).await;
    assert_eq!(
        (status, entry),
        (StatusCode::OK, json!({ "type": "hosted", "namespace": "extra" })),
    );
}

#[tokio::test]
async fn a_hosted_registry_added_at_runtime_survives_a_restart() {
    let dir = TempDir::new().unwrap();
    let yaml = format!("storage: {}\n{EMPTY_CONFIG}", storage_dir(dir.path()).display());
    let config = crate::managed_registries::managed_config(
        &config_path(dir.path()),
        &yaml,
        listen(),
        public_url(),
    )
    .await;
    pnpr::prepare_storage(&config).await.unwrap();
    let replica = Replica::new(config.clone()).await;
    let graph = json!({
        "registries": {
            "local": { "type": "hosted", "namespace": "", "packages": { "**": { "publish": "$authenticated" } } },
        },
        "defaultRegistry": "local",
    });
    assert_eq!(replica.as_root("PUT", DOCUMENT, Some(graph)).await.0, StatusCode::OK);
    let publish = crate::npm::publish_doc("runtime-pkg", "1.0.0", b"tarball-contents");
    let (status, body, _) =
        replica.send("PUT", "/npm/runtime-pkg", Some(&replica.bob), Some(publish)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    pnpr::prepare_storage(&config).await.unwrap();
}

#[tokio::test]
async fn a_dry_run_reports_the_changes_without_storing_them() {
    let dir = TempDir::new().unwrap();
    let replica = Replica::new(load_config(dir.path()).await).await;
    let entry = json!({ "type": "hosted", "namespace": "extra" });

    let path = format!("{DOCUMENT}/extra?dryRun=true");
    let (status, report) = replica.as_root("PUT", &path, Some(entry)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(report["version"], json!("1"));
    assert_eq!(report["changes"]["added"], json!(["extra"]));
    assert_eq!(
        replica.as_root("GET", &format!("{DOCUMENT}/extra"), None).await.0,
        StatusCode::NOT_FOUND,
    );
}

#[tokio::test]
async fn a_change_based_on_a_stale_version_is_refused() {
    let dir = TempDir::new().unwrap();
    let replica = Replica::new(load_config(dir.path()).await).await;
    let token = Some(replica.root.as_str());
    let path = format!("{DOCUMENT}/extra");
    let entry = || Some(json!({ "type": "hosted", "namespace": "extra" }));

    let (status, _, version) = send(&replica.app, "PUT", &path, token, entry(), Some("1")).await;
    assert_eq!((status, version.as_deref()), (StatusCode::OK, Some("2")));
    let (status, error, _) = send(&replica.app, "DELETE", &path, token, None, Some("1")).await;
    assert_eq!(status, StatusCode::PRECONDITION_FAILED);
    assert_eq!(error["code"], "ERR_PNPR_PRECONDITION_FAILED");
    assert_eq!(replica.as_root("GET", &path, None).await.0, StatusCode::OK);
}

#[tokio::test]
async fn revisions_are_listed_newest_first_and_a_restore_stores_a_new_one() {
    let dir = TempDir::new().unwrap();
    let replica = Replica::new(load_config(dir.path()).await).await;
    let extra = json!({ "type": "hosted", "namespace": "extra" });
    assert_eq!(
        replica.as_root("PUT", &format!("{DOCUMENT}/extra"), Some(extra)).await.0,
        StatusCode::OK,
    );
    assert_eq!(
        replica.as_root("DELETE", &format!("{DOCUMENT}/extra"), None).await.0,
        StatusCode::OK,
    );

    let (status, original) = replica.as_root("GET", &format!("{REVISIONS}/2"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(original["registries"]["extra"].is_object(), "{original}");
    let (status, restored) = replica.as_root("POST", &format!("{REVISIONS}/2/restore"), None).await;
    assert_eq!((status, &restored["version"]), (StatusCode::OK, &json!("4")));
    let (_, current, _) = replica.send("GET", DOCUMENT, Some(&replica.root), None).await;
    assert_eq!(current, original);

    let (_, listing) = replica.as_root("GET", &format!("{REVISIONS}?limit=2"), None).await;
    let versions: Vec<&Value> = listing["revisions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|revision| &revision["version"])
        .collect();
    assert_eq!(versions, [&json!("4"), &json!("3")]);
    assert_eq!(listing["revisions"][0]["restoredFrom"], json!("2"));
    assert_eq!(listing["revisions"][0]["actorId"], json!("root"));
    let (_, older) = replica.as_root("GET", &format!("{REVISIONS}?before=3"), None).await;
    let versions: Vec<&Value> = older["revisions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|revision| &revision["version"])
        .collect();
    assert_eq!(versions, [&json!("2"), &json!("1")]);
    assert_eq!(
        replica.as_root("GET", &format!("{REVISIONS}/99"), None).await.0,
        StatusCode::NOT_FOUND,
    );
}

#[tokio::test]
async fn admins_read_and_only_registry_admins_change_the_structure() {
    let dir = TempDir::new().unwrap();
    let replica = Replica::new(load_config(dir.path()).await).await;
    let extra = || Some(json!({ "type": "hosted", "namespace": "extra" }));
    let path = format!("{DOCUMENT}/extra");

    assert_eq!(replica.send("GET", DOCUMENT, Some(&replica.alice), None).await.0, StatusCode::OK);
    assert_eq!(replica.send("GET", REVISIONS, Some(&replica.alice), None).await.0, StatusCode::OK);
    for (method, path) in [
        ("PUT", path.as_str()),
        ("DELETE", "/-/pnpr/v0/admin/registries/local"),
        ("POST", "/-/pnpr/v0/admin/registry-revisions/1/restore"),
    ] {
        let (status, _, _) = replica.send(method, path, Some(&replica.alice), extra()).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}");
    }
    let rules = json!({ "access": ["$authenticated"] });
    let rules_path = "/-/pnpr/v0/admin/rules/npm/local";
    assert_eq!(
        replica.send("PUT", rules_path, Some(&replica.alice), Some(rules)).await.0,
        StatusCode::OK,
    );

    assert_eq!(
        replica.send("GET", DOCUMENT, Some(&replica.bob), None).await.0,
        StatusCode::FORBIDDEN,
    );
    assert_eq!(replica.send("GET", DOCUMENT, None, None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(replica.send("PUT", &path, Some(&replica.root), extra()).await.0, StatusCode::OK);
}

#[tokio::test]
async fn registry_admins_read_teams() {
    let dir = TempDir::new().unwrap();
    let replica = Replica::new(load_config(dir.path()).await).await;
    for path in ["/npm/-/org/acme/team", "/npm/~local/-/org/acme/team"] {
        let (status, teams, _) = replica.send("GET", path, Some(&replica.root), None).await;
        assert_eq!((status, teams), (StatusCode::OK, json!([])), "{path}");
        let (status, _, _) = replica.send("GET", path, Some(&replica.bob), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn the_api_refuses_registries_the_config_file_declares() {
    let dir = TempDir::new().unwrap();
    let replica = Replica::new(load_file_config(dir.path())).await;
    for (method, path) in [
        ("GET", DOCUMENT),
        ("PUT", "/-/pnpr/v0/admin/registries/extra"),
        ("GET", REVISIONS),
        ("POST", "/-/pnpr/v0/admin/registry-revisions/1/restore"),
    ] {
        let body = Some(json!({ "type": "hosted" }));
        let (status, error, _) = replica.send(method, path, Some(&replica.root), body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}");
        assert_eq!(error["code"], "ERR_PNPR_REGISTRIES_CONFIG_MANAGED", "{method} {path}");
    }
}

#[tokio::test]
async fn a_hosted_registry_that_holds_packages_keeps_its_name_and_storage() {
    let dir = TempDir::new().unwrap();
    let config = load_config(dir.path()).await;
    let storage = Storage::from_config(&config.storage).unwrap();
    let replica = Replica::new(config).await;
    let empty = json!({ "type": "hosted", "namespace": "empty" });
    assert_eq!(
        replica.as_root("PUT", &format!("{DOCUMENT}/empty"), Some(empty)).await.0,
        StatusCode::OK,
    );
    assert_eq!(
        replica.as_root("DELETE", &format!("{DOCUMENT}/empty"), None).await.0,
        StatusCode::OK,
    );

    let package = pnpr_package_name::CanonicalPackageName::parse(
        "example",
        pnpr_package_name::Ecosystem::Npm,
    )
    .unwrap();
    storage
        .write_hosted_document_if_current(
            &package,
            br#"{"name":"example","versions":{},"dist-tags":{}}"#,
            Some(&HostedDocumentVersion::Unversioned),
            None,
        )
        .await
        .unwrap();
    let moved = json!({ "type": "hosted", "namespace": "elsewhere", "packages": { "**": {} } });
    let (_, document, _) = replica.send("GET", DOCUMENT, Some(&replica.root), None).await;
    let mut renamed = document.clone();
    let local = renamed["registries"]
        .as_object_mut()
        .unwrap()
        .remove("local")
        .unwrap();
    renamed["registries"]["renamed"] = local;
    renamed["registries"]["main"]["sources"] = json!(["renamed"]);
    for (method, path, body) in [
        ("PUT", format!("{DOCUMENT}/local"), Some(moved)),
        ("PUT", DOCUMENT.to_string(), Some(renamed)),
    ] {
        let (status, error) = replica.as_root(method, &path, body).await;
        assert_eq!(status, StatusCode::CONFLICT, "{method} {path}: {error}");
        assert!(
            error["error"]
                .as_str()
                .unwrap()
                .contains("holds packages"),
            "{error}",
        );
    }
    let (_, current, _) = replica.send("GET", DOCUMENT, Some(&replica.root), None).await;
    assert_eq!(current, document);
}

#[tokio::test]
async fn stored_upstream_credentials_must_be_listed_references() {
    let dir = TempDir::new().unwrap();
    let replica = Replica::new(load_config(dir.path()).await).await;
    let private = |auth: Value, headers: Value| {
        json!({
            "type": "upstream", "url": "https://npm.example.com/", "access": "$authenticated",
            "auth": auth, "headers": headers,
        })
    };
    let refused = [
        private(json!({ "type": "bearer", "token": "literal-secret" }), json!({})),
        private(json!({ "type": "bearer", "token": "${PNPR_UNLISTED}" }), json!({})),
        private(json!({ "type": "bearer", "tokenEnv": "PNPR_UNLISTED" }), json!({})),
        private(
            json!({ "type": "bearer", "tokenEnv": "PATH" }),
            json!({ "x-key": "${PNPR_UNLISTED}" }),
        ),
        private(
            json!({ "type": "bearer", "tokenEnv": "PATH" }),
            json!({ "Authorization": "Bearer literal-secret" }),
        ),
        private(json!({ "type": "bearer", "tokenEnv": "PATH" }), json!({ "x-key": "key ${PATH}" })),
    ];
    for entry in refused {
        let (status, error) =
            replica.as_root("PUT", &format!("{DOCUMENT}/corp"), Some(entry.clone())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{entry}: {error}");
    }
    let listed =
        private(json!({ "type": "bearer", "token": "${PATH}" }), json!({ "x-key": "${PATH}" }));
    let (status, error) = replica.as_root("PUT", &format!("{DOCUMENT}/corp"), Some(listed)).await;
    assert_eq!(status, StatusCode::OK, "{error}");
    let (_, stored) = replica.as_root("GET", &format!("{DOCUMENT}/corp"), None).await;
    assert_eq!(stored["auth"]["token"], json!("${PATH}"));
    assert_eq!(stored["headers"]["x-key"], json!("${PATH}"));
}

#[tokio::test]
async fn an_upstream_url_on_a_private_address_is_refused() {
    let dir = TempDir::new().unwrap();
    let replica = Replica::new(load_config(dir.path()).await).await;
    for url in [
        "http://127.0.0.1:9/",
        "http://[::1]/",
        "http://169.254.169.254/",
        "http://localhost/",
        "http://registry.localhost/",
    ] {
        let (status, error) =
            replica.as_root("PUT", &format!("{DOCUMENT}/up"), Some(upstream(url))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{url}: {error}");
    }
    for url in ["http://10.1.2.3/", "https://registry.npmjs.org/"] {
        let (status, error) =
            replica.as_root("PUT", &format!("{DOCUMENT}/up"), Some(upstream(url))).await;
        assert_eq!(status, StatusCode::OK, "{url}: {error}");
    }
}

#[tokio::test]
async fn an_upstream_url_must_be_an_http_url() {
    let dir = TempDir::new().unwrap();
    let replica = Replica::new(load_config(dir.path()).await).await;
    for url in ["not a url", "ftp://registry.example/", "file:///etc/", "https:"] {
        let path = format!("{DOCUMENT}/up");
        let (status, error) = replica.as_root("PUT", &path, Some(upstream(url))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{url}: {error}");
    }
}

#[tokio::test]
async fn a_stored_public_route_must_parse() {
    let dir = TempDir::new().unwrap();
    let replica = Replica::new(load_config(dir.path()).await).await;
    let (_, mut document, _) = replica.send("GET", DOCUMENT, Some(&replica.root), None).await;
    document["routes"] = json!({ "public": [{ "registry": "not a registry url" }] });
    let (status, error) = replica.as_root("PUT", DOCUMENT, Some(document)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
}

#[tokio::test]
async fn a_stored_public_route_on_the_own_host_must_be_at_its_public_origin() {
    let dir = TempDir::new().unwrap();
    let replica = Replica::new(load_config(dir.path()).await).await;
    let (_, document, _) = replica.send("GET", DOCUMENT, Some(&replica.root), None).await;
    let cases = [
        ("http://example.test:9000/", StatusCode::BAD_REQUEST),
        ("http://127.0.0.1:9000/", StatusCode::BAD_REQUEST),
        ("http://10.0.0.5:9000/", StatusCode::OK),
        ("http://example.test/", StatusCode::OK),
    ];
    for (registry, expected) in cases {
        let mut document = document.clone();
        document["routes"] = json!({ "public": [{ "registry": registry }] });
        let (status, body) = replica.as_root("PUT", DOCUMENT, Some(document)).await;
        assert_eq!(status, expected, "{registry}: {body}");
    }
}

/// A server that counts the requests it answers.
async fn counting_server() -> (SocketAddr, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    let app = axum::Router::new()
        .fallback(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            async { (StatusCode::OK, r#"{"name":"example","versions":{},"dist-tags":{}}"#) }
        });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (address, hits, server)
}

#[tokio::test]
async fn a_stored_upstream_never_reaches_a_refused_address() {
    let dir = TempDir::new().unwrap();
    let config = load_config(dir.path()).await;
    let (address, hits, server) = counting_server().await;
    let port = address.port();
    let graph = json!({
        "registries": {
            "literal": upstream(&format!("http://127.0.0.1:{port}/")),
            "named": upstream(&format!("http://localhost:{port}/")),
        },
    });
    let storage = Storage::from_config(&config.storage).unwrap();
    let document = graph.to_string();
    let revision = NewRegistryGraph {
        document: &document,
        actor_id: "-",
        restored_from: None,
        vacated: &[],
        bound: &[],
    };
    let written = storage
        .write_registry_graph(Some(1), revision)
        .await
        .unwrap();
    assert!(matches!(written, RegistryGraphWrite::Written(2)), "{written:?}");
    let replica = Replica::new(config).await;

    for registry in ["literal", "named"] {
        let path = format!("/npm/~{registry}/example");
        let (status, _, _) = replica.send("GET", &path, Some(&replica.bob), None).await;
        assert_ne!(status, StatusCode::OK, "{path}");
    }
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    server.abort();
}

#[tokio::test]
async fn the_own_public_host_is_no_exception_for_a_stored_upstream() {
    let (address, hits, server) = counting_server().await;
    let port = address.port();
    for host in ["127.0.0.1", "localhost"] {
        let dir = TempDir::new().unwrap();
        let own_url = format!("http://{host}:{port}");
        let yaml = format!("storage: {}\n{CONFIG}", storage_dir(dir.path()).display());
        let config = crate::managed_registries::managed_config(
            &config_path(dir.path()),
            &yaml,
            listen(),
            Some(own_url.clone()),
        )
        .await;
        let storage = Storage::from_config(&config.storage).unwrap();
        let document =
            json!({ "registries": { "up": upstream(&format!("{own_url}/")) } }).to_string();
        let revision = NewRegistryGraph {
            document: &document,
            actor_id: "-",
            restored_from: None,
            vacated: &[],
            bound: &[],
        };
        storage
            .write_registry_graph(Some(1), revision)
            .await
            .unwrap();
        let replica = Replica::new(config).await;

        let (status, _, _) =
            replica.send("GET", "/npm/~up/example", Some(&replica.bob), None).await;
        assert_ne!(status, StatusCode::OK, "{host}");
        let rewrite = Some(upstream(&format!("{own_url}/other/")));
        let (status, error) = replica.as_root("PUT", &format!("{DOCUMENT}/up"), rewrite).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{host}: {error}");
    }
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    server.abort();
}

#[tokio::test]
async fn a_stored_upstream_at_the_own_public_origin_is_refused_on_an_allowed_network() {
    let (address, hits, server) = counting_server().await;
    let port = address.port();
    let allowed = "['10.0.0.0/8', '127.0.0.0/8', '::1/128']";
    for host in ["127.0.0.1", "localhost"] {
        let dir = TempDir::new().unwrap();
        let own_url = format!("http://{host}:{port}");
        let yaml = format!("storage: {}\n{CONFIG}", storage_dir(dir.path()).display()).replace(
            "['10.0.0.0/8']",
            allowed,
        );
        let config = crate::managed_registries::managed_config(
            &config_path(dir.path()),
            &yaml,
            listen(),
            Some(own_url.clone()),
        )
        .await;
        let storage = Storage::from_config(&config.storage).unwrap();
        let document =
            json!({ "registries": { "up": upstream(&format!("{own_url}/")) } }).to_string();
        let revision = NewRegistryGraph {
            document: &document,
            actor_id: "-",
            restored_from: None,
            vacated: &[],
            bound: &[],
        };
        storage
            .write_registry_graph(Some(1), revision)
            .await
            .unwrap();
        let replica = Replica::new(config).await;

        let (status, _, _) =
            replica.send("GET", "/npm/~up/example", Some(&replica.bob), None).await;
        assert_ne!(status, StatusCode::OK, "{host}");
        let rewrite = Some(upstream(&format!("{own_url}/other/")));
        let (status, error) = replica.as_root("PUT", &format!("{DOCUMENT}/up"), rewrite).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{host}: {error}");
        let elsewhere = Some(upstream("http://127.0.0.2:1/"));
        let (status, body) = replica.as_root("PUT", &format!("{DOCUMENT}/up"), elsewhere).await;
        assert_eq!(status, StatusCode::OK, "{host}: an allowed address at another origin {body}");
    }
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    server.abort();
}

#[tokio::test]
async fn a_private_public_host_is_refused_for_a_stored_upstream() {
    let dir = TempDir::new().unwrap();
    let yaml = format!("storage: {}\n{CONFIG}", storage_dir(dir.path()).display());
    let own_url = "http://192.168.7.7:4873".to_string();
    let config = crate::managed_registries::managed_config(
        &config_path(dir.path()),
        &yaml,
        listen(),
        Some(own_url),
    )
    .await;
    let replica = Replica::new(config).await;
    let entry = Some(upstream("http://192.168.7.7:4873/"));
    let (status, error) = replica.as_root("PUT", &format!("{DOCUMENT}/up"), entry).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
}

#[tokio::test]
async fn offline_cache_gc_keeps_the_caches_of_stored_upstreams() {
    let dir = TempDir::new().unwrap();
    let (address, hits, server) = counting_server().await;
    let yaml = format!(
        "storage: {storage}\nsecret: test-secret-key-32-chars-long-12\nauth:\n  registryAdmins: [root]\n  \
         users:\n    maxUsers: 100\nregistryEnv: [PATH]\nroutes:\n  allowedPrivateNetworks: ['127.0.0.0/8']\n\
         registries:\n  up:\n    type: upstream\n    url: http://127.0.0.1:{port}/\n    access: $all\n    \
         headers:\n      x-key: '${{PATH}}'\n",
        storage = storage_dir(dir.path()).display(),
        port = address.port(),
    );
    let config = crate::managed_registries::managed_config(
        &config_path(dir.path()),
        &yaml,
        listen(),
        public_url(),
    )
    .await;
    let replica = Replica::new(config.clone()).await;
    let (status, body, _) = replica.send("GET", "/npm/~up/example", Some(&replica.bob), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(hits.load(Ordering::SeqCst) > 0);
    let cached = walk(&config.storage.cache_dir);
    assert!(
        cached
            .iter()
            .any(|path| path.contains("example")),
        "{cached:?}",
    );

    let gc = pnpr::Command::CacheGc {
        dry_run: false,
        max_size: None,
        max_age: None,
        orphans_only: true,
    };
    pnpr::run_command(&config, gc).await.unwrap();
    let kept = walk(&config.storage.cache_dir);
    for path in cached
        .iter()
        .filter(|path| path.contains("example"))
    {
        assert!(kept.contains(path), "cache-gc removed {path}");
    }
    server.abort();
}

#[tokio::test]
async fn a_file_upstream_on_loopback_is_reached() {
    let dir = TempDir::new().unwrap();
    let (address, hits, server) = counting_server().await;
    let yaml = format!(
        "storage: {}\nregistries:\n  literal:\n    type: upstream\n    public: true\n    url: http://127.0.0.1:{}/\n",
        storage_dir(dir.path()).display(),
        address.port(),
    );
    std::fs::write(config_path(dir.path()), yaml).unwrap();
    let replica = Replica::new(reload_config(dir.path())).await;
    let (status, body, _) =
        replica.send("GET", "/npm/~literal/example", Some(&replica.bob), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(hits.load(Ordering::SeqCst) > 0);
    server.abort();
}

#[tokio::test]
async fn concurrent_writers_on_two_replicas_both_land() {
    let dir = TempDir::new().unwrap();
    let config = load_config(dir.path()).await;
    let auth = crate::tokens::seeded_auth(&config, ACCOUNTS);
    let first = Replica::with_auth(config, auth.clone()).await;
    let second = Replica::with_auth(reload_config(dir.path()), auth).await;

    let entry = |namespace: &str| Some(json!({ "type": "hosted", "namespace": namespace }));
    let [one_path, two_path, three_path, four_path] =
        ["one", "two", "three", "four"].map(|name| format!("{DOCUMENT}/{name}"));
    let first_write = first.as_root("PUT", &one_path, entry("one"));
    let second_write = second.as_root("PUT", &two_path, entry("two"));
    let (one, two) = tokio::join!(first_write, second_write);
    assert_eq!((one.0, two.0), (StatusCode::OK, StatusCode::OK), "{one:?} {two:?}");
    let (_, document, version) = first.send("GET", DOCUMENT, Some(&first.root), None).await;
    assert_eq!(version.as_deref(), Some("3"));
    assert!(document["registries"]["one"].is_object() && document["registries"]["two"].is_object());

    let token = Some(first.root.as_str());
    let first_write = send(&first.app, "PUT", &three_path, token, entry("three"), Some("3"));
    let second_write = send(&second.app, "PUT", &four_path, token, entry("four"), Some("3"));
    let (three, four) = tokio::join!(first_write, second_write);
    let mut statuses = [three.0, four.0];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::OK, StatusCode::PRECONDITION_FAILED]);
}

#[tokio::test]
async fn a_replica_that_still_serves_a_removed_hosted_registry_cannot_publish_into_it() {
    let dir = TempDir::new().unwrap();
    let config = load_config(dir.path()).await;
    let auth = crate::tokens::seeded_auth(&config, ACCOUNTS);
    let writer = Replica::with_auth(config, auth.clone()).await;
    let path = format!("{DOCUMENT}/doomed");
    let doomed = json!({
        "type": "hosted",
        "namespace": "doomed",
        "packages": { "**": { "publish": "$authenticated" } },
    });
    assert_eq!(writer.as_root("PUT", &path, Some(doomed.clone())).await.0, StatusCode::OK);
    let stale = Replica::with_auth(reload_config(dir.path()), auth).await;
    assert_eq!(writer.as_root("DELETE", &path, None).await.0, StatusCode::OK);

    let publish = |name: &str| Some(crate::npm::publish_doc(name, "1.0.0", b"tarball-contents"));
    let target = "/npm/~doomed/fenced-pkg";
    let (status, error, _) =
        stale.send("PUT", target, Some(&stale.bob), publish("fenced-pkg")).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{error}");
    assert_eq!(error["code"], "ERR_PNPR_REGISTRIES_UNAVAILABLE");

    assert_eq!(writer.as_root("PUT", &path, Some(doomed)).await.0, StatusCode::OK);
    let (status, body, _) =
        stale.send("PUT", target, Some(&stale.bob), publish("fenced-pkg")).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

async fn directory_lists(replica: &Replica, name: &str) -> bool {
    let (_, directory, _) =
        replica.send("GET", "/-/pnpr/v0/registries", Some(&replica.bob), None).await;
    directory["registries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["name"] == name)
}

#[tokio::test]
async fn a_write_that_changes_nothing_serves_the_version_it_answers() {
    let dir = TempDir::new().unwrap();
    let config = load_config(dir.path()).await;
    let auth = crate::tokens::seeded_auth(&config, ACCOUNTS);
    let writer = Replica::with_auth(config, auth.clone()).await;
    let behind = Replica::with_auth(reload_config(dir.path()), auth).await;
    let extra = json!({ "type": "hosted", "namespace": "extra" });
    assert_eq!(
        writer.as_root("PUT", &format!("{DOCUMENT}/extra"), Some(extra)).await.0,
        StatusCode::OK,
    );
    assert!(!directory_lists(&behind, "extra").await);

    let (_, document, _) = behind.send("GET", DOCUMENT, Some(&behind.root), None).await;
    let (status, outcome) = behind.as_root("PUT", DOCUMENT, Some(document)).await;
    assert_eq!((status, &outcome["version"]), (StatusCode::OK, &json!("2")));
    assert!(directory_lists(&behind, "extra").await);
}

#[tokio::test]
async fn rules_change_on_a_registry_another_replica_just_added() {
    let dir = TempDir::new().unwrap();
    let config = load_config(dir.path()).await;
    let auth = crate::tokens::seeded_auth(&config, ACCOUNTS);
    let writer = Replica::with_auth(config, auth.clone()).await;
    let behind = Replica::with_auth(reload_config(dir.path()), auth).await;
    let fresh = json!({ "type": "hosted", "namespace": "fresh", "packages": { "@fresh/*": {} } });
    assert_eq!(
        writer.as_root("PUT", &format!("{DOCUMENT}/fresh"), Some(fresh)).await.0,
        StatusCode::OK,
    );

    let rules = "/-/pnpr/v0/admin/rules/npm/fresh";
    let (status, before) = behind.as_root("GET", rules, None).await;
    assert_eq!(status, StatusCode::OK, "{before}");
    let change = json!({ "packages": { "@fresh/*": { "access": ["$authenticated"] } } });
    let (status, after) = behind.as_root("PUT", rules, Some(change)).await;
    assert_eq!(status, StatusCode::OK, "{after}");
    assert_eq!(after["packages"]["@fresh/*"]["access"], json!(["$authenticated"]));
    assert_eq!(after["version"], json!("3"));
}

/// Keeps the runtime busy so database I/O cannot auto-advance the paused
/// test clock.
struct ManualClock(tokio::task::JoinHandle<()>);

impl ManualClock {
    fn new() -> Self {
        Self(tokio::spawn(async {
            loop {
                tokio::task::yield_now().await;
            }
        }))
    }
}

impl Drop for ManualClock {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[tokio::test(start_paused = true)]
async fn another_replica_follows_a_change_within_the_reload_interval() {
    let _clock = ManualClock::new();
    let dir = TempDir::new().unwrap();
    let config = load_config(dir.path()).await;
    let auth = crate::tokens::seeded_auth(&config, ACCOUNTS);
    let writer = Replica::with_auth(config, auth.clone()).await;
    let reader = Replica::with_auth(reload_config(dir.path()), auth).await;
    let directory = |replica: &Replica| {
        let token = replica.bob.clone();
        let app = replica.app.clone();
        async move {
            let (_, directory, _) =
                send(&app, "GET", "/-/pnpr/v0/registries", Some(&token), None, None).await;
            directory["registries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["name"] == "extra")
        }
    };
    assert!(!directory(&reader).await);

    let extra = json!({ "type": "hosted", "namespace": "extra" });
    assert_eq!(
        writer.as_root("PUT", &format!("{DOCUMENT}/extra"), Some(extra)).await.0,
        StatusCode::OK,
    );
    assert!(directory(&writer).await);
    assert!(!directory(&reader).await);

    tokio::time::advance(Duration::from_secs(10)).await;
    for _ in 0..1000 {
        if directory(&reader).await {
            return;
        }
        tokio::task::spawn_blocking(|| std::thread::sleep(Duration::from_millis(1)))
            .await
            .unwrap();
        tokio::time::advance(Duration::from_millis(10)).await;
    }
    panic!("the reader never loaded the new graph");
}

#[tokio::test]
async fn a_registry_admin_repairs_a_graph_the_replica_cannot_serve() {
    let dir = TempDir::new().unwrap();
    let config = load_config(dir.path()).await;
    let storage = Storage::from_config(&config.storage).unwrap();
    let unreadable = NewRegistryGraph {
        document: "{",
        actor_id: "-",
        restored_from: None,
        vacated: &[],
        bound: &[],
    };
    storage
        .write_registry_graph(Some(1), unreadable)
        .await
        .unwrap();
    let replica = Replica::new(config).await;

    let (status, error, _) = replica.send("GET", "/npm/example", Some(&replica.bob), None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(error["code"], "ERR_PNPR_REGISTRIES_UNAVAILABLE");
    let (status, _, _) = replica.send("GET", "/-/pnpr/v0/sign-in", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, restored) = replica.as_root("POST", &format!("{REVISIONS}/1/restore"), None).await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    let (status, _, _) = replica.send("GET", "/npm/example", Some(&replica.bob), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_exported_graph_imports_as_no_change() {
    let dir = TempDir::new().unwrap();
    let config = load_config(dir.path()).await;
    let exported = dir.path().join("registries.yaml");
    let export = pnpr::RegistriesSubcommand::Export { out: Some(exported.clone()) };
    pnpr::run_command(&config, pnpr::Command::Registries { action: export })
        .await
        .unwrap();
    let yaml = std::fs::read_to_string(&exported).unwrap();
    assert!(yaml.contains("defaultRegistry"), "{yaml}");

    let import = |file: &Path, dry_run: bool| pnpr::Command::Registries {
        action: pnpr::RegistriesSubcommand::Import {
            file: file.to_path_buf(),
            dry_run,
            check: false,
        },
    };
    pnpr::run_command(&config, import(&exported, false)).await.unwrap();
    let storage = Storage::from_config(&config.storage).unwrap();
    assert_eq!(storage.read_registry_graph_generation().await.unwrap(), Some(1));

    let changed = dir.path().join("changed.yaml");
    std::fs::write(&changed, yaml.replace("main", "primary")).unwrap();
    pnpr::run_command(&config, import(&changed, true)).await.unwrap();
    assert_eq!(storage.read_registry_graph_generation().await.unwrap(), Some(1));
    pnpr::run_command(&config, import(&changed, false)).await.unwrap();
    assert_eq!(storage.read_registry_graph_generation().await.unwrap(), Some(2));

    let bad_setting = dir.path().join("bad-setting.yaml");
    std::fs::write(
        &bad_setting,
        "registries:\n  up:\n    type: upstream\n    url: https://registry.npmjs.org/\n    public: true\n    timeout: nonsense\n",
    )
    .unwrap();
    assert!(pnpr::run_command(&config, import(&bad_setting, true)).await.is_err());
    assert!(pnpr::run_command(&config, import(&bad_setting, false)).await.is_err());

    let invalid = dir.path().join("invalid.yaml");
    std::fs::write(
        &invalid,
        "registries:\n  up:\n    type: upstream\n    url: http://127.0.0.1/\n    public: true\n",
    )
    .unwrap();
    assert!(pnpr::run_command(&config, import(&invalid, false)).await.is_err());
    assert_eq!(storage.read_registry_graph_generation().await.unwrap(), Some(2));
}

#[tokio::test]
async fn offline_commands_read_the_stored_registries() {
    let dir = TempDir::new().unwrap();
    let config = load_config(dir.path()).await;
    pnpr::prepare_storage(&config).await.unwrap();
    let storage = Storage::from_config(&config.storage).unwrap();
    let package = pnpr_package_name::CanonicalPackageName::parse(
        "example",
        pnpr_package_name::Ecosystem::Npm,
    )
    .unwrap();
    storage
        .write_hosted_document_if_current(
            &package,
            br#"{"name":"example","versions":{},"dist-tags":{}}"#,
            Some(&HostedDocumentVersion::Unversioned),
            None,
        )
        .await
        .unwrap();
    assert!(config.routing.hosted.is_empty());

    let out = dir.path().join("export");
    pnpr::run_command(&config, pnpr::Command::Export { out: out.clone() }).await.unwrap();
    let exported: Vec<_> = walk(&out);
    assert!(
        exported
            .iter()
            .any(|path| path.contains("example")),
        "{exported:?}",
    );
    let gc = pnpr::Command::Gc {
        registry: vec!["local".to_string()],
        dry_run: true,
        min_age: Duration::ZERO,
    };
    pnpr::run_command(&config, gc).await.unwrap();
}

fn walk(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(&next)
            .into_iter()
            .flatten()
            .flatten()
        {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                found.push(path.display().to_string());
            }
        }
    }
    found
}

#[tokio::test]
async fn a_server_whose_stored_graph_does_not_load_starts_and_is_repaired() {
    let dir = TempDir::new().unwrap();
    let mut config = load_config(dir.path()).await;
    drop(crate::tokens::seeded_auth(&config, ACCOUNTS));
    let accounts = config.storage.hosted_dir.join("test-accounts.sqlite");
    config.identity.backend = pnpr_config::BackendConfig::Sqlite(accounts);
    let storage = Storage::from_config(&config.storage).unwrap();
    let unreadable = NewRegistryGraph {
        document: "{",
        actor_id: "-",
        restored_from: None,
        vacated: &[],
        bound: &[],
    };
    storage
        .write_registry_graph(Some(1), unreadable)
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(pnpr::serve_listener(config, listener));
    let client = reqwest::Client::new();
    for _ in 0..500 {
        assert!(!server.is_finished(), "the server stopped: {:?}", server.await);
        if client
            .get(format!("{base}/-/ping"))
            .send()
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let login = json!({ "name": "root", "password": "secret123" });
    let response = client
        .put(format!("{base}/-/user/org.couchdb.user:root"))
        .json(&login)
        .send()
        .await
        .unwrap();
    let token = response.json::<Value>().await.unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string();
    let get_example = || {
        client
            .get(format!("{base}/npm/example"))
            .bearer_auth(&token)
            .send()
    };
    assert_eq!(get_example().await.unwrap().status(), StatusCode::SERVICE_UNAVAILABLE);
    let restore = client
        .post(format!("{base}{REVISIONS}/1/restore"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(restore.status(), StatusCode::OK);
    assert_eq!(get_example().await.unwrap().status(), StatusCode::NOT_FOUND);
    server.abort();
}

#[tokio::test]
async fn offline_paths_refuse_a_public_upstream_with_a_credential() {
    let dir = TempDir::new().unwrap();
    let mut config = load_config(dir.path()).await;
    let file = dir.path().join("public-with-auth.yaml");
    std::fs::write(
        &file,
        "registries:\n  up:\n    type: upstream\n    url: https://registry.npmjs.org/\n    public: true\n    \
         auth: { type: bearer, tokenEnv: PATH }\n",
    )
    .unwrap();
    for dry_run in [true, false] {
        let import =
            pnpr::RegistriesSubcommand::Import { file: file.clone(), dry_run, check: false };
        let result = pnpr::run_command(&config, pnpr::Command::Registries { action: import }).await;
        assert!(result.is_err(), "import with dry_run {dry_run} accepted it");
    }

    let graph = json!({
        "registries": {
            "up": {
                "type": "upstream", "url": "https://registry.npmjs.org/", "public": true,
                "auth": { "type": "bearer", "tokenEnv": "PATH" },
            },
        },
    });
    crate::managed_registries::store_graph(&config, &graph).await;
    let checked = pnpr::config_check::check_config_and_stored_registries(
        &mut config,
        &pnpr::ConfigSource::Bundled,
    )
    .await;
    let error = checked.expect_err("config check accepted it").to_string();
    assert!(error.contains("public"), "{error}");
}

#[tokio::test]
async fn config_check_accepts_a_stored_upstream_whose_credential_is_unset_here() {
    let dir = TempDir::new().unwrap();
    let yaml = format!(
        "storage: {}\nregistryEnv: [PNPR_TEST_UNSET_TOKEN]\nregistries:\n  corp:\n    type: upstream\n    \
         url: https://npm.corp.example/\n    access: $authenticated\n    \
         auth: {{ type: bearer, tokenEnv: PNPR_TEST_UNSET_TOKEN }}\n",
        storage_dir(dir.path()).display(),
    );
    let mut config = crate::managed_registries::managed_config(
        &config_path(dir.path()),
        &yaml,
        listen(),
        public_url(),
    )
    .await;
    let report = pnpr::config_check::check_config_and_stored_registries(
        &mut config,
        &pnpr::ConfigSource::Bundled,
    )
    .await
    .unwrap();
    let report = pnpr::config_check::format_report(&report);
    assert!(report.contains("corp"), "{report}");
}

#[tokio::test]
async fn config_check_fails_on_an_oidc_group_grant_the_stored_registries_lack() {
    let dir = TempDir::new().unwrap();
    let yaml = format!(
        "storage: {}\nauth:\n  oidc:\n    - name: company\n      issuer: https://issuer.example\n      \
         audience: pnpr\n      login:\n        groups:\n          teams:\n            \
         - group: eng\n              registry: local\n              teamId: platform\n\
         registries:\n  local:\n    type: hosted\n    namespace: ''\n    packages:\n      '**': {{}}\n",
        storage_dir(dir.path()).display(),
    );
    let mut config = crate::managed_registries::managed_config(
        &config_path(dir.path()),
        &yaml,
        listen(),
        public_url(),
    )
    .await;
    let error = pnpr::config_check::check_config_and_stored_registries(
        &mut config,
        &pnpr::ConfigSource::Bundled,
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("platform"), "{error}");
}

#[tokio::test]
async fn a_public_route_is_checked_over_both_schemes() {
    let dir = TempDir::new().unwrap();
    let yaml = format!("storage: {}\n{CONFIG}", storage_dir(dir.path()).display());
    let config = crate::managed_registries::managed_config(
        &config_path(dir.path()),
        &yaml,
        listen(),
        Some("https://example.test".to_string()),
    )
    .await;
    let replica = Replica::new(config).await;
    let (_, document, _) = replica.send("GET", DOCUMENT, Some(&replica.root), None).await;
    for (registry, expected) in [
        ("http://example.test/", StatusCode::OK),
        ("https://example.test/", StatusCode::OK),
        ("http://example.test:9000/", StatusCode::BAD_REQUEST),
        ("https://127.0.0.1/", StatusCode::BAD_REQUEST),
    ] {
        let mut document = document.clone();
        document["routes"] = json!({ "public": [{ "registry": registry }] });
        let (status, body) = replica.as_root("PUT", DOCUMENT, Some(document)).await;
        assert_eq!(status, expected, "{registry}: {body}");
    }
}

#[tokio::test]
async fn a_public_ip_public_url_refuses_stored_destinations_at_its_other_ports() {
    let dir = TempDir::new().unwrap();
    let yaml = format!("storage: {}\n{CONFIG}", storage_dir(dir.path()).display());
    let config = crate::managed_registries::managed_config(
        &config_path(dir.path()),
        &yaml,
        listen(),
        Some("http://104.16.0.5:4873".to_string()),
    )
    .await;
    let replica = Replica::new(config).await;
    let entry = Some(upstream("http://104.16.0.5:9000/"));
    let (status, error) = replica.as_root("PUT", &format!("{DOCUMENT}/up"), entry).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    let (_, mut document, _) = replica.send("GET", DOCUMENT, Some(&replica.root), None).await;
    document["routes"] = json!({ "public": [{ "registry": "http://104.16.0.5:9000/" }] });
    let (status, error) = replica.as_root("PUT", DOCUMENT, Some(document)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
}

#[tokio::test]
async fn a_stored_upstream_at_another_port_of_the_own_host_is_never_contacted() {
    let (address, hits, server) = counting_server().await;
    let dir = TempDir::new().unwrap();
    let yaml = format!("storage: {}\n{CONFIG}", storage_dir(dir.path()).display()).replace(
        "['10.0.0.0/8']",
        "['10.0.0.0/8', '127.0.0.0/8']",
    );
    let config = crate::managed_registries::managed_config(
        &config_path(dir.path()),
        &yaml,
        listen(),
        Some("http://127.0.0.1:4873".to_string()),
    )
    .await;
    let other_port = format!("http://127.0.0.1:{}/", address.port());
    let graph = json!({ "registries": { "up": upstream(&other_port) } });
    crate::managed_registries::store_graph(&config, &graph).await;
    let replica = Replica::new(config).await;
    let (status, _, _) = replica.send("GET", "/npm/~up/example", Some(&replica.bob), None).await;
    assert_ne!(status, StatusCode::OK);
    let rewrite = Some(upstream(&format!("{other_port}other/")));
    let (status, error) = replica.as_root("PUT", &format!("{DOCUMENT}/up"), rewrite).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    server.abort();
}

#[tokio::test]
async fn another_spelling_of_the_own_host_is_refused_for_a_stored_upstream() {
    let spellings = [
        ("http://104.16.0.5:4873", "http://[::ffff:104.16.0.5]:9000/"),
        ("http://[::ffff:104.16.0.5]:4873", "http://104.16.0.5:9000/"),
        ("http://example.test", "http://example.test.:9000/"),
        ("http://example.test.", "http://example.test:9000/"),
    ];
    for (own, other_port) in spellings {
        let dir = TempDir::new().unwrap();
        let yaml = format!("storage: {}\n{CONFIG}", storage_dir(dir.path()).display());
        let config = crate::managed_registries::managed_config(
            &config_path(dir.path()),
            &yaml,
            listen(),
            Some(own.to_string()),
        )
        .await;
        let replica = Replica::new(config).await;
        let entry = Some(upstream(other_port));
        let (status, error) = replica.as_root("PUT", &format!("{DOCUMENT}/up"), entry).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{own}: {other_port}: {error}");
    }
}

#[tokio::test]
async fn credentials_in_a_stored_url_are_refused_by_the_api_and_import() {
    let dir = TempDir::new().unwrap();
    let config = load_config(dir.path()).await;
    let replica = Replica::new(config.clone()).await;
    let entry = Some(upstream("https://user:pass@npm.corp.example/"));
    let (status, error) = replica.as_root("PUT", &format!("{DOCUMENT}/up"), entry).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    let (_, mut document, _) = replica.send("GET", DOCUMENT, Some(&replica.root), None).await;
    document["routes"] = json!({ "public": [{ "registry": "https://user:pass@mirror.example/" }] });
    let (status, error) = replica.as_root("PUT", DOCUMENT, Some(document)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");

    let file = dir.path().join("with-credentials.yaml");
    std::fs::write(
        &file,
        "registries:\n  up:\n    type: upstream\n    url: https://user:pass@npm.corp.example/\n    \
         access: $authenticated\n",
    )
    .unwrap();
    for dry_run in [true, false] {
        let import =
            pnpr::RegistriesSubcommand::Import { file: file.clone(), dry_run, check: false };
        let result = pnpr::run_command(&config, pnpr::Command::Registries { action: import }).await;
        assert!(result.is_err(), "import with dry_run {dry_run} accepted it");
    }
}
