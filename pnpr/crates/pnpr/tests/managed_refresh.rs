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
use pnpr_storage::{HostedDocumentVersion, RegistryRecord, Storage};
use std::{
    sync::{Arc, Mutex},
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
    async fn new() -> Self {
        let clock = ManualClock::new();
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("config.yaml");
        std::fs::write(
            &path,
            format!(
                r"
storage: '{}'
auth:
  scim:
    token: test-scim-secret-0123456789abcdef0123456789
registries:
  local:
    type: hosted
    org: ''
    teamsManagedBy: api
    rulesManagedBy: api
    teams:
      readers: {{ name: readers, members: [reader] }}
    packages:
      '**':
        access: [team:readers]
defaultRegistry: local
",
                directory
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

#[tokio::test(start_paused = true)]
async fn stalled_roster_denies_stale_membership_and_retries() {
    let replica = Replica::new().await;
    replica.start_stalled_refresh(&["team-roster"]).await;
    replica.expect_status("/npm/example", StatusCode::FORBIDDEN).await;
    tokio::time::advance(Duration::from_secs(1)).await;
    replica.expect_recovery("/npm/example", StatusCode::OK).await;
    replica.storage
        .create_registry_record(RegistryRecord::TeamRoster, "npm/local", br#"{"teams":[]}"#)
        .await
        .unwrap();
    tokio::time::advance(Duration::from_secs(10)).await;
    replica.expect_status("/npm/example", StatusCode::FORBIDDEN).await;
}

#[tokio::test(start_paused = true)]
async fn stalled_rules_deny_stale_access_and_recover() {
    let replica = Replica::new().await;
    replica.start_stalled_refresh(&["rule-overrides"]).await;
    replica.expect_status("/npm/example", StatusCode::NOT_FOUND).await;
    tokio::time::advance(Duration::from_secs(1)).await;
    replica.expect_recovery("/npm/example", StatusCode::OK).await;
}

#[tokio::test(start_paused = true)]
async fn stalled_scim_refuses_credentials_and_recovers() {
    let replica = Replica::new().await;
    replica.start_stalled_refresh(&["scim-users"]).await;
    replica.expect_status("/-/whoami", StatusCode::INTERNAL_SERVER_ERROR).await;
    tokio::time::advance(Duration::from_secs(1)).await;
    replica.expect_recovery("/-/whoami", StatusCode::OK).await;
}

#[tokio::test(start_paused = true)]
async fn stalled_roster_does_not_delay_the_scim_deadline() {
    let replica = Replica::new().await;
    replica.start_stalled_refresh(&["team-roster", "scim-users"]).await;
    replica.expect_status("/-/whoami", StatusCode::INTERNAL_SERVER_ERROR).await;
    tokio::time::advance(Duration::from_secs(1)).await;
    replica.expect_recovery("/-/whoami", StatusCode::OK).await;
}

#[tokio::test(start_paused = true)]
async fn stalled_rules_do_not_delay_the_scim_deadline() {
    let replica = Replica::new().await;
    replica.start_stalled_refresh(&["rule-overrides", "scim-users"]).await;
    replica.expect_status("/-/whoami", StatusCode::INTERNAL_SERVER_ERROR).await;
    tokio::time::advance(Duration::from_secs(1)).await;
    replica.expect_recovery("/-/whoami", StatusCode::OK).await;
}

#[tokio::test(start_paused = true)]
async fn the_initial_read_has_a_deadline_and_retries() {
    let replica = Replica::new().await;
    replica.database.pause("scim-users".to_string());
    let request = replica.get("/-/whoami");
    tokio::pin!(request);
    tokio::select! {
        status = &mut request => panic!("initial authorization completed before the read: {status}"),
        () = replica.database.started.notified() => {}
    }
    tokio::time::advance(Duration::from_secs(5)).await;
    assert_eq!(request.await, StatusCode::INTERNAL_SERVER_ERROR);
    replica.storage
        .create_registry_record(
            RegistryRecord::ScimUsers,
            pnpr_storage::SCIM_DIRECTORY,
            br#"{"users":{"reader":{"active":false}}}"#,
        )
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
}

impl PausingDatabase {
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
    if database.take_pause(&body) {
        database.started.notify_one();
        tokio::time::sleep(Duration::from_secs(6)).await;
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
