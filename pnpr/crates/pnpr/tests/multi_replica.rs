//! Several pnpr replicas behind one load balancer: separate processes, each
//! with its own local scratch and accounts, sharing one hosted object store
//! and one metadata database. Replicas share a libSQL primary; here one
//! `SQLite` file stands in for it.
//!
//! What holds the deployment together is that every write into that shared
//! store is conditional, so a request can land on any replica. These tests
//! drive two routers over one bucket the way a load balancer would spread
//! requests over two containers.

use crate::{npm, pausing_store};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use npm::publish_doc;
use object_store::{ObjectStore, memory::InMemory};
use pausing_store::PausingStore;
use pnpr::{Config, HostedStoreConfig, MaxUsers};
use pnpr_config::MetadataBackend;
use pnpr_fixtures::MockConfigPresets;
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::Arc,
};
use tempfile::TempDir;
use tower::ServiceExt;

/// One replica: its own storage directory (accounts, cache)
/// over the hosted store every replica shares.
struct Replica {
    app: axum::Router,
    token: String,
    _storage: TempDir,
}

/// The metadata database every replica of a test shares.
struct SharedMetadata(TempDir);

impl SharedMetadata {
    fn new() -> Self {
        Self(TempDir::new().unwrap())
    }
}

impl Replica {
    async fn start(store: &Arc<dyn ObjectStore>, metadata: &SharedMetadata) -> Replica {
        let storage = TempDir::new().unwrap();
        let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
        let mut config = Config::static_serve(listen, storage.path().to_path_buf());
        config.http.public_url = "http://example.test".to_string();
        config.identity.auth.users.max_users = MaxUsers::Unlimited;
        config.storage.hosted_backend =
            HostedStoreConfig::ObjectStore { store: Arc::clone(store), prefix: String::new() };
        config.storage.metadata =
            MetadataBackend::Sqlite(metadata.0.path().join("metadata.sqlite"));
        let auth = crate::tokens::seeded_auth(&config, &[("alice", "secret")]);
        let app = pnpr::router_with_auth(config, auth);
        let token = add_user_and_get_token(app.clone(), "alice", "secret").await;
        Replica { app, token, _storage: storage }
    }

    async fn send(&self, method: &str, path: &str, body: Body) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .header("Authorization", format!("Bearer {}", self.token))
            .body(body)
            .unwrap();
        let response = self.app
            .clone()
            .oneshot(request)
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn publish(&self, name: &str, version: &str, tarball: &[u8]) -> StatusCode {
        let body = publish_doc(name, version, tarball);
        self.send("PUT", &format!("/npm/{name}"), Body::from(serde_json::to_vec(&body).unwrap()))
            .await
            .0
    }

    /// Publish `name@version` in the background, answering its status.
    fn spawn_publish(
        &self,
        name: &str,
        version: &str,
        tarball: &[u8],
    ) -> tokio::task::JoinHandle<StatusCode> {
        let app = self.app.clone();
        let token = self.token.clone();
        let body = publish_doc(name, version, tarball);
        let request = Request::put(format!("/npm/{name}"))
            .header("content-type", "application/json")
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        tokio::spawn(async move {
            app.oneshot(request).await
                .unwrap()
                .status()
        })
    }

    async fn packument(&self, name: &str) -> Value {
        let (status, document) = self.send("GET", &format!("/npm/{name}"), Body::empty()).await;
        assert_eq!(status, StatusCode::OK, "{name} must be served by every replica");
        document
    }

    async fn stage(&self, name: &str, version: &str, tarball: &[u8]) -> String {
        let body = publish_doc(name, version, tarball);
        let (status, payload) = self.send(
            "POST",
            &format!("/npm/-/stage/package/{name}"),
            Body::from(serde_json::to_vec(&body).unwrap()),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        payload["stageId"]
            .as_str()
            .expect("stageId in response")
            .to_string()
    }

    async fn approve(&self, stage_id: &str) -> StatusCode {
        self.send("POST", &format!("/npm/-/stage/{stage_id}/approve"), Body::empty())
            .await
            .0
    }
}

/// Two replicas publishing versions of one package at once both land: the one
/// that commits second rebases its versions and tags onto what the first
/// committed, instead of writing over it or being refused.
#[tokio::test]
async fn concurrent_publishes_of_one_package_keep_both_versions() {
    let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let metadata = SharedMetadata::new();
    let first = Replica::start(&store, &metadata).await;
    let second = Replica::start(&store, &metadata).await;

    let publish_one = first.publish("shared-pkg", "1.0.0", b"one");
    let publish_two = second.publish("shared-pkg", "2.0.0", b"two");
    let (left, right) = tokio::join!(publish_one, publish_two);
    assert_eq!((left, right), (StatusCode::CREATED, StatusCode::CREATED));
    assert_both_versions(&[&first, &second], "shared-pkg").await;
}

/// The race made certain: one replica has read the packument and is
/// uploading its tarball when another replica publishes, so its commit finds
/// another generation and rebases.
#[tokio::test]
async fn a_publish_that_loses_the_race_rebases_onto_the_winner() {
    let objects = Arc::new(PausingStore::default());
    let store: Arc<dyn ObjectStore> = Arc::clone(&objects) as Arc<dyn ObjectStore>;
    let metadata = SharedMetadata::new();
    let slow = Replica::start(&store, &metadata).await;
    let fast = Replica::start(&store, &metadata).await;
    assert_eq!(fast.publish("raced-pkg", "1.0.0", b"one").await, StatusCode::CREATED);

    objects.pause_write(tarball_key("raced-pkg", "1.1.0", b"slow"));
    let slow_publish = slow.spawn_publish("raced-pkg", "1.1.0", b"slow");
    tokio::time::timeout(std::time::Duration::from_secs(10), objects.started.notified())
        .await
        .expect("the slow publish reaches its tarball upload");
    assert_eq!(fast.publish("raced-pkg", "2.0.0", b"two").await, StatusCode::CREATED);
    objects.resume.notify_one();

    assert_eq!(slow_publish.await.unwrap(), StatusCode::CREATED);
    for replica in [&slow, &fast] {
        let packument = replica.packument("raced-pkg").await;
        let versions = packument["versions"].as_object().expect("versions");
        for version in ["1.0.0", "1.1.0", "2.0.0"] {
            assert!(versions.contains_key(version), "{version} is missing: {versions:?}");
        }
    }
}

/// A publish that loses the race for a package another user created meanwhile
/// is not rebased: that user owns the package now.
#[tokio::test]
async fn a_first_publish_that_loses_the_race_to_another_user_is_refused() {
    let objects = Arc::new(PausingStore::default());
    let store: Arc<dyn ObjectStore> = Arc::clone(&objects) as Arc<dyn ObjectStore>;
    let metadata = SharedMetadata::new();
    let slow = Replica::start(&store, &metadata).await;
    let mut fast = Replica::start(&store, &metadata).await;
    fast.token = add_user_and_get_token(fast.app.clone(), "bob", "secret").await;

    objects.pause_write(tarball_key("claimed-pkg", "1.0.0", b"slow"));
    let slow_publish = slow.spawn_publish("claimed-pkg", "1.0.0", b"slow");
    tokio::time::timeout(std::time::Duration::from_secs(10), objects.started.notified())
        .await
        .expect("the slow publish reaches its tarball upload");
    assert_eq!(fast.publish("claimed-pkg", "2.0.0", b"two").await, StatusCode::CREATED);
    objects.resume.notify_one();

    assert_eq!(slow_publish.await.unwrap(), StatusCode::CONFLICT);
    let packument = slow.packument("claimed-pkg").await;
    assert_eq!(
        packument["maintainers"]
            .as_array()
            .unwrap()
            .len(),
        1,
    );
    let id = crate::tokens::account_id(&fast.app, "bob").await.unwrap();
    assert_eq!(packument["maintainers"][0]["userId"], id);
    assert_eq!(packument["maintainers"][0]["name"], "bob");
    assert!(packument["versions"].get("1.0.0").is_none());
}

/// The key the first incarnation of the tarball `publish_doc` builds for
/// `name@version` from `payload` is stored under.
fn tarball_key(name: &str, version: &str, payload: &[u8]) -> String {
    let tarball = crate::npm::publishable_tgz(name, version, payload);
    pnpr_storage::BlobId::of_bytes(pnpr_storage::Ecosystem::Npm, &tarball).key(1)
}

async fn assert_both_versions(replicas: &[&Replica], name: &str) {
    for replica in replicas {
        let packument = replica.packument(name).await;
        let versions = packument["versions"].as_object().expect("versions");
        assert!(versions.contains_key("1.0.0"), "1.0.0 is missing: {versions:?}");
        assert!(versions.contains_key("2.0.0"), "2.0.0 is missing: {versions:?}");
    }
}

/// A staged record is shared state, not the state of the replica that took it.
#[tokio::test]
async fn a_stage_created_on_one_replica_is_approved_on_another() {
    let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let metadata = SharedMetadata::new();
    let first = Replica::start(&store, &metadata).await;
    let second = Replica::start(&store, &metadata).await;

    let stage_id = first.stage("staged-pkg", "1.0.0", b"the tarball").await;
    assert_eq!(second.approve(&stage_id).await, StatusCode::CREATED);
    assert!(first.packument("staged-pkg").await["versions"]["1.0.0"].is_object());
    assert_eq!(
        first.approve(&stage_id).await,
        StatusCode::NOT_FOUND,
        "the stage is spent on every replica",
    );
}

/// Approving one stage on two replicas at once publishes it once: the held
/// document is replayed by whichever approval claims the record.
#[tokio::test]
async fn one_stage_approved_on_two_replicas_publishes_once() {
    let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let metadata = SharedMetadata::new();
    let first = Replica::start(&store, &metadata).await;
    let second = Replica::start(&store, &metadata).await;

    let stage_id = first.stage("staged-pkg", "1.0.0", b"the tarball").await;
    let approve_here = first.approve(&stage_id);
    let approve_there = second.approve(&stage_id);
    let (left, right) = tokio::join!(approve_here, approve_there);
    let (approved, refused) =
        if left == StatusCode::CREATED { (left, right) } else { (right, left) };
    assert_eq!(approved, StatusCode::CREATED, "one approval must publish: {left}, {right}");
    assert!(
        // Whether the loser saw the winner's claim or the record it had
        // already consumed depends on how far the winner got.
        refused == StatusCode::CONFLICT || refused == StatusCode::NOT_FOUND,
        "the other must be refused, not failed: {refused}",
    );

    let packument = first.packument("staged-pkg").await;
    assert_eq!(
        packument["versions"]
            .as_object()
            .expect("versions")
            .len(),
        1,
    );
    let (status, listing) = second.send("GET", "/npm/-/stage", Body::empty()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing["total"], 0, "the stage is spent");
}

/// A rejection reaching one replica takes the stage back from an approval
/// under way on another, as long as it lands before that approval commits.
///
/// The approving replica is paused after it has the held publish in hand,
/// while it stages it: reading the tarball's integrity record, the one
/// `document.json` the bucket still holds, before the commit, which is where
/// a rejection can still be observed.
#[tokio::test]
async fn a_rejection_stops_an_approval_that_has_not_committed() {
    let objects = Arc::new(PausingStore::default());
    let store: Arc<dyn ObjectStore> = Arc::clone(&objects) as Arc<dyn ObjectStore>;
    let metadata = SharedMetadata::new();
    let approving = Replica::start(&store, &metadata).await;
    let rejecting = Replica::start(&store, &metadata).await;

    let stage_id = approving.stage("staged-pkg", "1.0.0", b"the tarball").await;
    objects.pause("document.json".to_string());
    let approval = tokio::spawn({
        let app = approving.app.clone();
        let token = approving.token.clone();
        let path = format!("/npm/-/stage/{stage_id}/approve");
        async move {
            let request = Request::post(&path)
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap();
            app.oneshot(request).await
                .unwrap()
                .status()
        }
    });

    tokio::time::timeout(std::time::Duration::from_secs(10), objects.started.notified())
        .await
        .expect("the approval reaches the paused read");
    let (rejected, _) =
        rejecting.send("DELETE", &format!("/npm/-/stage/{stage_id}"), Body::empty()).await;
    assert_eq!(rejected, StatusCode::NO_CONTENT);
    objects.resume.notify_one();

    assert_ne!(approval.await.unwrap(), StatusCode::CREATED, "the rejection stands");
    let (status, _) = approving.send("GET", "/npm/staged-pkg", Body::empty()).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "a rejected publish must not be served");
}

/// A dist-tag moved through one replica is what every replica serves.
#[tokio::test]
async fn a_dist_tag_written_on_one_replica_is_served_by_the_other() {
    let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let metadata = SharedMetadata::new();
    let first = Replica::start(&store, &metadata).await;
    let second = Replica::start(&store, &metadata).await;

    assert_eq!(first.publish("tagged-pkg", "1.0.0", b"one").await, StatusCode::CREATED);
    assert_eq!(second.publish("tagged-pkg", "2.0.0", b"two").await, StatusCode::CREATED);
    let (status, _) =
        first.send("PUT", "/npm/-/package/tagged-pkg/dist-tags/next", Body::from(r#""2.0.0""#))
            .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, tags) =
        second.send("GET", "/npm/-/package/tagged-pkg/dist-tags", Body::empty()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(tags["next"], "2.0.0");
}

async fn add_user_and_get_token(app: axum::Router, username: &str, password: &str) -> String {
    let path = format!("/-/user/org.couchdb.user:{username}");
    let body = json!({
        "_id": format!("org.couchdb.user:{username}"),
        "name": username,
        "password": password,
        "email": "foo@bar.net",
        "type": "user",
        "roles": [],
    });
    let request = Request::put(&path)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: Value = serde_json::from_slice(&bytes).unwrap();
    payload["token"]
        .as_str()
        .expect("token in response")
        .to_string()
}
