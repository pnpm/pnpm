//! How many round trips each npm operation makes to a libSQL primary, such
//! as Turso, where each one costs a network round trip. The primary here is
//! a local mock that counts them; the counts guard against a change that
//! adds a round trip to an operation without meaning to.

use crate::npm::publish_doc;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use pnpr::{Config, MaxUsers, router};
use pnpr_config::{LibsqlSettings, MetadataBackend};
use pnpr_fixtures::MockConfigPresets;
use pnpr_libsql::testing::{Primary, serve_primary};
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::Path,
    sync::Arc,
};
use tempfile::TempDir;
use tower::ServiceExt;

fn libsql_config(storage: &Path, primary: SocketAddr) -> Config {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let mut config = Config::static_serve(listen, storage.to_path_buf());
    config.http.public_url = "http://example.test".to_string();
    config.identity.auth.users.max_users = MaxUsers::Unlimited;
    config.storage.metadata = MetadataBackend::Libsql(LibsqlSettings {
        url: format!("http://{primary}"),
        auth_token: None,
        replica_path: None,
        sync_interval_secs: None,
        max_connections: None,
    });
    config
}

fn request(method: &str, path: &str, token: &str, body: Option<&Value>) -> Request<Body> {
    let body = body.map_or_else(Body::empty, |body| Body::from(serde_json::to_vec(body).unwrap()));
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(body)
        .unwrap()
}

/// The registry under test and the primary its metadata lives in.
struct Registry {
    app: Router,
    primary: Arc<Primary>,
    token: String,
    _server: tokio::task::JoinHandle<()>,
    storage: TempDir,
}

impl Registry {
    async fn start() -> Self {
        let storage = TempDir::new().unwrap();
        let (primary, address, server) = serve_primary(storage.path().join("primary.db")).await;
        let app = router(libsql_config(&storage.path().join("registry"), address));
        let body = json!({ "name": "alice", "password": "secret", "type": "user", "roles": [] });
        let (status, bytes) =
            send_to(&app, request("PUT", "/-/user/org.couchdb.user:alice", "-", Some(&body))).await;
        assert_eq!(status, StatusCode::CREATED);
        let token = serde_json::from_slice::<Value>(&bytes).unwrap()["token"]
            .as_str()
            .unwrap()
            .to_string();
        Self { app, primary, token, _server: server, storage }
    }

    /// Send `method path` with `body`, and report its status and the
    /// statements of each round trip it made to the primary.
    async fn send(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> (StatusCode, Vec<Vec<String>>) {
        self.round_trips();
        let (status, _) = send_to(&self.app, request(method, path, &self.token, body)).await;
        (status, self.round_trips())
    }

    /// The round trips to the primary since the last call, without the
    /// `COMMIT` that ends a read of the primary: it is sent after the read
    /// answers, and nothing waits for it.
    fn round_trips(&self) -> Vec<Vec<String>> {
        self.primary.round_trips
            .take()
            .into_iter()
            .filter(|statements| statements != &["COMMIT".to_string()])
            .collect()
    }
}

async fn send_to(app: &Router, request: Request<Body>) -> (StatusCode, Vec<u8>) {
    let response = app
        .clone()
        .oneshot(request)
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap().to_vec();
    (status, body)
}

/// Every round trip each operation made, listed with its statements.
fn listing(operations: &[(&str, Vec<Vec<String>>)]) -> String {
    use std::fmt::Write as _;
    let mut listing = String::new();
    for (operation, round_trips) in operations {
        writeln!(listing, "{operation}: {} round trips", round_trips.len()).unwrap();
        for (index, statements) in round_trips.iter().enumerate() {
            writeln!(listing, "  {}: {}", index + 1, statements.join(" | ")).unwrap();
        }
    }
    listing
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn npm_operations_make_few_round_trips_to_a_libsql_primary() {
    let registry = Registry::start().await;
    // The first operation creates the schema.
    let (status, _) = registry.send("GET", "/npm/sql-pkg", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let mut operations = Vec::new();
    let doc = publish_doc("sql-pkg", "1.0.0", b"1.0.0");
    let (status, round_trips) = registry.send("PUT", "/npm/sql-pkg", Some(&doc)).await;
    assert_eq!(status, StatusCode::CREATED);
    operations.push(("the first publish", round_trips));

    let doc = publish_doc("sql-pkg", "1.1.0", b"1.1.0");
    let (status, round_trips) = registry.send("PUT", "/npm/sql-pkg", Some(&doc)).await;
    assert_eq!(status, StatusCode::CREATED);
    operations.push(("a publish of another version", round_trips));

    let (status, round_trips) =
        registry.send("PUT", "/npm/-/package/sql-pkg/dist-tags/stable", Some(&json!("1.0.0")))
            .await;
    assert_eq!(status, StatusCode::CREATED);
    operations.push(("a dist-tag move", round_trips));

    let (status, round_trips) = registry.send("GET", "/npm/sql-pkg", None).await;
    assert_eq!(status, StatusCode::OK);
    operations.push(("a view read that renders a version", round_trips));

    let (status, round_trips) = registry.send("GET", "/npm/sql-pkg", None).await;
    assert_eq!(status, StatusCode::OK);
    operations.push(("a view read the cache answers", round_trips));

    let (status, round_trips) = registry.send("DELETE", "/npm/sql-pkg/-rev/1", None).await;
    assert_eq!(status, StatusCode::CREATED);
    operations.push(("an unpublish", round_trips));

    let counts: Vec<(&str, usize)> = operations
        .iter()
        .map(|(operation, round_trips)| (*operation, round_trips.len()))
        .collect();
    // A publish reads the outcome of an earlier identical request and the
    // current document, each in one round trip, and fetches fragments the
    // cache lacks in one more, before its write: the round trip that takes
    // the write lock with every read the write compares with, and the one
    // that sends every change with the commit. The rows of the blobs a
    // publish stores are read with its document. An unpublish removes the
    // package in its write's two round trips, then reclaims the archives
    // nothing maps any more: two round trips move them to `reclaiming`, and
    // one more records them gone once their keys are removed.
    let expected = vec![
        ("the first publish", 4),
        ("a publish of another version", 5),
        ("a dist-tag move", 2),
        ("a view read that renders a version", 2),
        ("a view read the cache answers", 1),
        ("an unpublish", 6),
    ];
    assert_eq!(counts, expected, "\n{}", listing(&operations));
}

/// Replicas sharing a libSQL primary hold no package lock in common, so a
/// publish refused at its commit removes the incarnation it stored, fenced
/// off first so that no commit still on its way can make it live.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refused_publish_on_libsql_removes_its_archives() {
    let registry = Registry::start().await;
    let batch = json!({ "packages": [publish_doc("first", "1.0.0", b"first")] });
    let committed = keyed(&registry, &batch).await;
    assert_eq!(committed, StatusCode::CREATED);

    // Another request under the same idempotency key is refused at its
    // commit, after its archive is in place.
    let other = json!({ "packages": [publish_doc("second", "1.0.0", b"second")] });
    assert_eq!(keyed(&registry, &other).await, StatusCode::CONFLICT);
    let store = registry.storage.path().join("registry");
    let npm = pnpr_storage::Ecosystem::Npm;
    assert_eq!(
        crate::storage::stored_archives(&store, "", npm).len(),
        1,
        "only the committed archive",
    );
}

/// Send `batch` as a batch publish under the idempotency key `k1`.
async fn keyed(registry: &Registry, batch: &Value) -> StatusCode {
    let mut request = request("PUT", "/-/pnpr/v0/publish", &registry.token, Some(batch));
    request
        .headers_mut()
        .insert("idempotency-key", header::HeaderValue::from_static("k1"));
    send_to(&registry.app, request).await.0
}
