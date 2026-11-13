#![cfg(feature = "backend-libsql")]

use crate::tokens::seeded_auth;
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::State,
    http::{Request, StatusCode},
    response::Response,
};
use pnpr::{Config, router_with_auth};
use pnpr_config::{LibsqlSettings, MetadataBackend};
use pnpr_libsql::testing::serve_primary;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use pnpr_storage::{
    HostedDocumentVersion, Storage,
    metadata::{NewRegistryGraph, NewScimUser},
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tempfile::TempDir;
use tower::ServiceExt;

struct Replica {
    app: Router,
    token: String,
    database: Arc<PausingDatabase>,
    storage: Storage,
    servers: Vec<tokio::task::JoinHandle<()>>,
    _directory: TempDir,
    _clock: ManualClock,
}

enum ManagedRecords {
    Registry,
    RegistryAndScim,
}

struct ManualClock(tokio::task::JoinHandle<()>);

impl ManualClock {
    fn new() -> Self {
        // Keep the runtime ready so database I/O cannot auto-advance the test clock.
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

impl Replica {
    async fn new(records: ManagedRecords) -> Self {
        let clock = ManualClock::new();
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("config.yaml");
        std::fs::write(
            &path,
            format!(
                r"
storage: '{storage}'
secret: 'test-secret-key-32-chars-long-12'
auth:
  registryAdmins: [reader]
{scim}
",
                scim = match records {
                    ManagedRecords::Registry => "",
                    ManagedRecords::RegistryAndScim => {
                        "  scim:\n    token: test-scim-secret-0123456789abcdef0123456789"
                    }
                },
                storage = directory
                    .path()
                    .join("storage")
                    .display(),
            ),
        )
        .unwrap();
        let mut config = Config::from_yaml(&path, "127.0.0.1:0".parse().unwrap(), None).unwrap();
        let (_, primary_address, primary_server) =
            serve_primary(directory.path().join("primary.db")).await;
        let database = Arc::new(PausingDatabase {
            primary: format!("http://{primary_address}"),
            records: Mutex::new(Vec::new()),
            started: tokio::sync::Notify::new(),
            holds: Mutex::new(Vec::new()),
            release: tokio::sync::Notify::new(),
            graph_reads: AtomicUsize::new(0),
        });
        let proxy = Router::new()
            .fallback(proxy_database)
            .with_state(Arc::clone(&database));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let proxy_server = tokio::spawn(async move {
            axum::serve(listener, proxy).await.unwrap();
        });
        config.storage.metadata = MetadataBackend::Libsql(LibsqlSettings {
            url: format!("http://{address}"),
            auth_token: None,
            replica_path: None,
            sync_interval_secs: None,
            max_connections: None,
        });
        crate::managed_registries::store_graph(&config, &graph()).await;
        let storage = Storage::from_config(&config.storage).unwrap();
        let package = CanonicalPackageName::parse("example", Ecosystem::Npm).unwrap();
        storage
            .write_hosted_document_if_current(
                &package,
                br#"{"name":"example","versions":{},"dist-tags":{}}"#,
                Some(&HostedDocumentVersion::Unversioned),
                None,
            )
            .await
            .unwrap();
        let auth = seeded_auth(&config, &[("reader", "password")]);
        let token = auth.tokens.issue("reader").await.unwrap();
        let app = router_with_auth(config, auth);
        Self {
            app,
            token,
            database,
            storage,
            servers: vec![primary_server, proxy_server],
            _directory: directory,
            _clock: clock,
        }
    }

    async fn get(&self, path: &str) -> StatusCode {
        self.app
            .clone()
            .oneshot(
                Request::get(path)
                    .header("authorization", format!("Bearer {}", self.token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
    }

    async fn put(&self, path: &str, body: &serde_json::Value) -> StatusCode {
        self.app
            .clone()
            .oneshot(
                Request::put(path)
                    .header("authorization", format!("Bearer {}", self.token))
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
    }

    async fn expect_status(&self, path: &str, expected: StatusCode) {
        self.wait_status(path, expected, Duration::ZERO).await;
    }

    async fn expect_recovery(&self, path: &str, expected: StatusCode) {
        self.wait_status(path, expected, Duration::from_millis(10)).await;
    }

    async fn wait_status(&self, path: &str, expected: StatusCode, step: Duration) {
        for _ in 0..1000 {
            if self.get(path).await == expected {
                return;
            }
            tokio::task::spawn_blocking(|| std::thread::sleep(Duration::from_millis(1)))
                .await
                .unwrap();
            tokio::time::advance(step).await;
        }
        assert_eq!(self.get(path).await, expected);
    }

    async fn start_stalled_refresh(&self, records: &[&str]) {
        assert_eq!(self.get("/npm/example").await, StatusCode::OK);
        for record in records {
            self.database.pause((*record).to_string());
        }
        tokio::time::advance(Duration::from_secs(10)).await;
        assert_eq!(self.get("/npm/example").await, StatusCode::OK);
        self.database.started.notified().await;
        tokio::time::advance(Duration::from_secs(4)).await;
        assert_eq!(self.get("/npm/example").await, StatusCode::OK);
        tokio::time::advance(Duration::from_secs(1)).await;
    }
}

/// The stored graph: one registry whose packages only the `readers` team,
/// holding `reader`, may read.
fn graph() -> serde_json::Value {
    serde_json::json!({
        "registries": {
            "local": {
                "type": "hosted",
                "namespace": "",
                "teams": { "readers": { "name": "readers", "members": ["reader"] } },
                "packages": { "**": { "access": ["team:readers"] } },
            },
        },
        "defaultRegistry": "local",
    })
}

async fn store_unreadable_graph(storage: &Storage) {
    let current = storage.read_registry_graph_generation().await.unwrap();
    let unreadable = NewRegistryGraph {
        document: "{",
        actor_id: "-",
        restored_from: None,
        vacated: &[],
        bound: &[],
    };
    storage.write_registry_graph(current, unreadable).await.unwrap();
}

async fn restore_graph(storage: &Storage) {
    let current = storage.read_registry_graph_generation().await.unwrap();
    let document = serde_json::to_string(&graph()).unwrap();
    let revision = NewRegistryGraph {
        document: &document,
        actor_id: "-",
        restored_from: Some(1),
        vacated: &[],
        bound: &[],
    };
    storage.write_registry_graph(current, revision).await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn a_stalled_graph_read_keeps_the_last_graph() {
    let replica = Replica::new(ManagedRecords::Registry).await;
    replica.start_stalled_refresh(&["registry_graph_revisions"]).await;
    replica.expect_status("/npm/example", StatusCode::OK).await;
}

#[tokio::test(start_paused = true)]
async fn an_unbuildable_graph_keeps_the_last_graph_until_the_stale_limit() {
    let replica = Replica::new(ManagedRecords::Registry).await;
    replica.expect_status("/npm/example", StatusCode::OK).await;
    store_unreadable_graph(&replica.storage).await;
    tokio::time::advance(Duration::from_secs(10)).await;
    replica.expect_status("/npm/example", StatusCode::OK).await;

    tokio::time::advance(Duration::from_secs(61)).await;
    replica.expect_status("/npm/example", StatusCode::SERVICE_UNAVAILABLE).await;
    assert_eq!(replica.get("/-/ping").await, StatusCode::OK);
    assert_eq!(replica.get("/-/ready").await, StatusCode::SERVICE_UNAVAILABLE);

    restore_graph(&replica.storage).await;
    tokio::time::advance(Duration::from_secs(1)).await;
    replica.expect_recovery("/npm/example", StatusCode::OK).await;
    assert_eq!(replica.get("/-/ready").await, StatusCode::OK);
}

#[tokio::test(start_paused = true)]
async fn stalled_scim_refuses_credentials_and_recovers() {
    let replica = Replica::new(ManagedRecords::RegistryAndScim).await;
    replica.start_stalled_refresh(&["scim_users"]).await;
    replica.expect_status("/-/whoami", StatusCode::INTERNAL_SERVER_ERROR).await;
    tokio::time::advance(Duration::from_secs(1)).await;
    replica.expect_recovery("/-/whoami", StatusCode::OK).await;
}

#[tokio::test(start_paused = true)]
async fn an_idle_replica_reloads_before_answering_past_the_stale_limit() {
    let replica = Replica::new(ManagedRecords::Registry).await;
    replica.expect_status("/npm/example", StatusCode::OK).await;
    tokio::time::advance(Duration::from_secs(61)).await;
    assert_eq!(replica.get("/npm/example").await, StatusCode::OK);
    tokio::time::advance(Duration::from_secs(61)).await;
    assert_eq!(replica.get("/-/ready").await, StatusCode::OK);
}

#[tokio::test(start_paused = true)]
async fn an_idle_replica_whose_reload_fails_answers_503() {
    let replica = Replica::new(ManagedRecords::Registry).await;
    replica.expect_status("/npm/example", StatusCode::OK).await;
    replica.database.pause("registry_graph_revisions".to_string());
    tokio::time::advance(Duration::from_secs(61)).await;
    let request = replica.get("/npm/example");
    tokio::pin!(request);
    tokio::select! {
        status = &mut request => panic!("the request did not wait for the reload: {status}"),
        () = replica.database.started.notified() => {}
    }
    tokio::time::advance(Duration::from_secs(5)).await;
    assert_eq!(request.await, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test(start_paused = true)]
async fn concurrent_requests_on_a_stale_graph_share_one_reload() {
    let replica = Replica::new(ManagedRecords::Registry).await;
    replica.expect_status("/npm/example", StatusCode::OK).await;
    store_unreadable_graph(&replica.storage).await;
    tokio::time::advance(Duration::from_secs(61)).await;
    let reads = replica.database.graph_reads.load(Ordering::SeqCst);

    let requests = (0..5).map(|_| replica.get("/npm/example"));
    let statuses = futures_util::future::join_all(requests).await;
    assert!(
        statuses
            .iter()
            .all(|status| *status == StatusCode::SERVICE_UNAVAILABLE),
        "{statuses:?}",
    );
    let reloads = replica.database.graph_reads.load(Ordering::SeqCst) - reads;
    assert!(reloads <= 2, "{reloads} graph reads for one reload");
}

#[tokio::test(start_paused = true)]
async fn a_stalled_edit_does_not_hold_up_requests_on_a_stale_graph() {
    let replica = Replica::new(ManagedRecords::Registry).await;
    replica.expect_status("/npm/example", StatusCode::OK).await;
    store_unreadable_graph(&replica.storage).await;
    tokio::time::advance(Duration::from_secs(61)).await;
    replica.database.hold("INSERT INTO registry_graph_revisions".to_string());
    let document = graph();
    let edit = replica.put("/-/pnpr/v0/admin/registries", &document);
    tokio::pin!(edit);
    tokio::select! {
        status = &mut edit => panic!("the edit finished while its write was held: {status}"),
        () = replica.database.started.notified() => {}
    }

    tokio::time::advance(Duration::from_secs(1)).await;
    let request = replica.get("/npm/example");
    tokio::pin!(request);
    tokio::select! {
        status = &mut request => panic!("the request did not wait for the lock: {status}"),
        () = tokio::task::yield_now() => {}
    }
    tokio::time::advance(Duration::from_secs(5)).await;
    assert_eq!(request.await, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(replica.get("/-/ping").await, StatusCode::OK);

    replica.database.release.notify_one();
    assert_eq!(edit.await, StatusCode::OK);
    replica.expect_status("/npm/example", StatusCode::OK).await;
}

#[tokio::test(start_paused = true)]
async fn a_stalled_graph_does_not_delay_the_scim_deadline() {
    let replica = Replica::new(ManagedRecords::RegistryAndScim).await;
    replica.start_stalled_refresh(&["registry_graph_revisions", "scim_users"]).await;
    replica.expect_status("/-/whoami", StatusCode::INTERNAL_SERVER_ERROR).await;
    tokio::time::advance(Duration::from_secs(1)).await;
    replica.expect_recovery("/-/whoami", StatusCode::OK).await;
}

#[tokio::test(start_paused = true)]
async fn the_initial_read_has_a_deadline_and_retries() {
    let replica = Replica::new(ManagedRecords::RegistryAndScim).await;
    replica.database.pause("scim_users".to_string());
    let request = replica.get("/-/whoami");
    tokio::pin!(request);
    tokio::select! {
        status = &mut request => panic!("initial authorization completed before the read: {status}"),
        () = replica.database.started.notified() => {}
    }
    tokio::time::advance(Duration::from_secs(5)).await;
    assert_eq!(request.await, StatusCode::INTERNAL_SERVER_ERROR);
    replica.storage
        .insert_scim_user(NewScimUser {
            user_id: "reader".to_string(),
            active: false,
            attributes: serde_json::Map::new(),
        })
        .await
        .unwrap();
    tokio::time::advance(Duration::from_secs(1)).await;
    replica.expect_recovery("/-/whoami", StatusCode::UNAUTHORIZED).await;
}

impl Drop for Replica {
    fn drop(&mut self) {
        for server in &self.servers {
            server.abort();
        }
    }
}

struct PausingDatabase {
    primary: String,
    records: Mutex<Vec<String>>,
    started: tokio::sync::Notify,
    /// Statements held until [`Self::release`] is notified.
    holds: Mutex<Vec<String>>,
    release: tokio::sync::Notify,
    graph_reads: AtomicUsize,
}

impl PausingDatabase {
    fn hold(&self, record: String) {
        self.holds.lock().unwrap().push(record);
    }

    fn take_hold(&self, body: &[u8]) -> bool {
        let body = String::from_utf8_lossy(body);
        let mut holds = self.holds.lock().unwrap();
        let held = holds
            .iter()
            .position(|record| body.contains(record));
        held.map(|index| holds.remove(index))
            .is_some()
    }

    fn pause(&self, record: String) {
        self.records
            .lock()
            .unwrap()
            .push(record);
    }

    fn take_pause(&self, body: &[u8]) -> bool {
        let body = String::from_utf8_lossy(body);
        let mut records = self.records.lock().unwrap();
        if let Some(index) = records
            .iter()
            .position(|record| body.contains(record))
        {
            records.remove(index);
            true
        } else {
            false
        }
    }
}

async fn proxy_database(
    State(database): State<Arc<PausingDatabase>>,
    request: Request<Body>,
) -> Response {
    let (parts, body) = request.into_parts();
    let body = to_bytes(body, usize::MAX).await.unwrap();
    if String::from_utf8_lossy(&body).contains("FROM registry_graph_revisions") {
        database.graph_reads.fetch_add(1, Ordering::SeqCst);
    }
    if database.take_pause(&body) {
        database.started.notify_one();
        tokio::time::sleep(Duration::from_secs(6)).await;
    }
    if database.take_hold(&body) {
        database.started.notify_one();
        database.release.notified().await;
    }
    let response = reqwest::Client::new()
        .request(parts.method, format!("{}{}", database.primary, parts.uri))
        .body(body)
        .send()
        .await
        .unwrap();
    Response::builder()
        .status(response.status())
        .body(Body::from(response.bytes().await.unwrap()))
        .unwrap()
}
