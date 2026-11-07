use crate::{pausing_store::PausingStore, tokens::seeded_auth};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use object_store::{ObjectStoreExt, path::Path};
use pnpr::{Config, HostedStoreConfig, router_with_auth};
use pnpr_config::MetadataBackend;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use pnpr_storage::{HostedDocumentVersion, Storage};
use std::{sync::Arc, time::Duration};
use tempfile::TempDir;
use tower::ServiceExt;

struct Replica {
    app: Router,
    token: String,
    objects: Arc<PausingStore>,
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
        let objects = Arc::new(PausingStore::default());
        config.storage.hosted_backend = HostedStoreConfig::ObjectStore {
            store: Arc::<PausingStore>::clone(&objects),
            prefix: String::new(),
        };
        config.storage.metadata = MetadataBackend::Sqlite(directory.path().join("metadata.sqlite"));
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
        Self { app, token, objects, _directory: directory, _clock: clock }
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
        for _ in 0..100 {
            if self.get(path).await == expected {
                return;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(self.get(path).await, expected);
    }

    async fn start_stalled_refresh(&self, records: &[&str]) {
        assert_eq!(self.get("/npm/example").await, StatusCode::OK);
        for record in records {
            self.objects.pause((*record).to_string());
        }
        tokio::time::advance(Duration::from_secs(10)).await;
        assert_eq!(self.get("/npm/example").await, StatusCode::OK);
        self.objects.started.notified().await;
        tokio::time::advance(Duration::from_secs(4)).await;
        assert_eq!(self.get("/npm/example").await, StatusCode::OK);
        tokio::time::advance(Duration::from_secs(1)).await;
    }
}

#[tokio::test(start_paused = true)]
async fn stalled_roster_denies_stale_membership_and_retries() {
    let replica = Replica::new().await;
    replica.start_stalled_refresh(&[".team-rosters/npm/local.json"]).await;
    replica.expect_status("/npm/example", StatusCode::FORBIDDEN).await;
    tokio::time::advance(Duration::from_secs(1)).await;
    replica.expect_status("/npm/example", StatusCode::OK).await;
    replica.objects
        .put(&Path::from(".team-rosters/npm/local.json"), br#"{"teams":[]}"#.to_vec().into())
        .await
        .unwrap();
    tokio::time::advance(Duration::from_secs(10)).await;
    replica.expect_status("/npm/example", StatusCode::FORBIDDEN).await;
}

#[tokio::test(start_paused = true)]
async fn stalled_rules_deny_stale_access_and_recover() {
    let replica = Replica::new().await;
    replica.start_stalled_refresh(&[".rule-overrides/npm/local.json"]).await;
    replica.expect_status("/npm/example", StatusCode::NOT_FOUND).await;
    tokio::time::advance(Duration::from_secs(1)).await;
    replica.expect_status("/npm/example", StatusCode::OK).await;
}

#[tokio::test(start_paused = true)]
async fn stalled_scim_refuses_credentials_and_recovers() {
    let replica = Replica::new().await;
    replica.start_stalled_refresh(&[".scim-users/directory.json"]).await;
    replica.expect_status("/-/whoami", StatusCode::INTERNAL_SERVER_ERROR).await;
    tokio::time::advance(Duration::from_secs(1)).await;
    replica.expect_status("/-/whoami", StatusCode::OK).await;
}

#[tokio::test(start_paused = true)]
async fn stalled_roster_does_not_delay_the_scim_deadline() {
    let replica = Replica::new().await;
    replica.start_stalled_refresh(&[".team-rosters/npm/local.json", ".scim-users/directory.json"])
        .await;
    replica.expect_status("/-/whoami", StatusCode::INTERNAL_SERVER_ERROR).await;
    tokio::time::advance(Duration::from_secs(1)).await;
    replica.expect_status("/-/whoami", StatusCode::OK).await;
}

#[tokio::test(start_paused = true)]
async fn stalled_rules_do_not_delay_the_scim_deadline() {
    let replica = Replica::new().await;
    replica.start_stalled_refresh(&[
        ".rule-overrides/npm/local.json",
        ".scim-users/directory.json",
    ])
    .await;
    replica.expect_status("/-/whoami", StatusCode::INTERNAL_SERVER_ERROR).await;
    tokio::time::advance(Duration::from_secs(1)).await;
    replica.expect_status("/-/whoami", StatusCode::OK).await;
}

#[tokio::test(start_paused = true)]
async fn the_initial_read_has_a_deadline_and_retries() {
    let replica = Replica::new().await;
    replica.objects.pause(".scim-users/directory.json".to_string());
    let request = replica.get("/-/whoami");
    tokio::pin!(request);
    tokio::select! {
        status = &mut request => panic!("initial authorization completed before the read: {status}"),
        () = replica.objects.started.notified() => {}
    }
    tokio::time::advance(Duration::from_secs(5)).await;
    assert_eq!(request.await, StatusCode::INTERNAL_SERVER_ERROR);
    replica.objects
        .put(
            &Path::from(".scim-users/directory.json"),
            br#"{"users":{"reader":{"active":false}}}"#.to_vec().into(),
        )
        .await
        .unwrap();
    tokio::time::advance(Duration::from_secs(1)).await;
    replica.expect_status("/-/whoami", StatusCode::UNAUTHORIZED).await;
}
