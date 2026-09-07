//! Several pnpr replicas behind one load balancer: separate processes, each
//! with its own local scratch and accounts, sharing one hosted object store.
//!
//! What holds the deployment together is that every write into that shared
//! store is conditional, so a request can land on any replica. These tests
//! drive two routers over one bucket the way a load balancer would spread
//! requests over two containers.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use object_store::{ObjectStore, memory::InMemory};
use pnpr::{Config, HostedStoreConfig, MaxUsers, router};
use serde_json::{Value, json};
use std::{
    fmt::Write,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::Arc,
};
use tempfile::TempDir;
use tower::ServiceExt;

/// One replica: its own storage directory (accounts, cache, publish journal)
/// over the hosted store every replica shares.
struct Replica {
    app: axum::Router,
    token: String,
    _storage: TempDir,
}

impl Replica {
    async fn start(store: &Arc<dyn ObjectStore>) -> Replica {
        let storage = TempDir::new().unwrap();
        let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
        let mut config = Config::static_serve(listen, storage.path().to_path_buf());
        config.public_url = "http://example.test".to_string();
        config.auth.htpasswd.max_users = MaxUsers::Unlimited;
        config.hosted_store =
            HostedStoreConfig::ObjectStore { store: Arc::clone(store), prefix: String::new() };
        let app = router(config);
        // Accounts are per-replica state, so every replica registers the
        // publisher and issues it a token of its own.
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
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn publish(&self, name: &str, version: &str, tarball: &[u8]) -> StatusCode {
        let body = publish_doc(name, version, tarball);
        self.send("PUT", &format!("/{name}"), Body::from(serde_json::to_vec(&body).unwrap()))
            .await
            .0
    }

    async fn packument(&self, name: &str) -> Value {
        let (status, document) = self.send("GET", &format!("/{name}"), Body::empty()).await;
        assert_eq!(status, StatusCode::OK, "{name} must be served by every replica");
        document
    }

    async fn stage(&self, name: &str, version: &str, tarball: &[u8]) -> String {
        let body = publish_doc(name, version, tarball);
        let (status, payload) = self
            .send(
                "POST",
                &format!("/-/stage/package/{name}"),
                Body::from(serde_json::to_vec(&body).unwrap()),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        payload["stageId"].as_str().expect("stageId in response").to_string()
    }

    async fn approve(&self, stage_id: &str) -> StatusCode {
        self.send("POST", &format!("/-/stage/{stage_id}/approve"), Body::empty()).await.0
    }
}

/// Two replicas publishing different versions of one package at the same time
/// keep both: the conditional document write makes the loser re-read and merge
/// rather than write over the version it never saw.
#[tokio::test]
async fn concurrent_publishes_of_one_package_keep_both_versions() {
    let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let first = Replica::start(&store).await;
    let second = Replica::start(&store).await;

    let publish_one = first.publish("shared-pkg", "1.0.0", b"one");
    let publish_two = second.publish("shared-pkg", "2.0.0", b"two");
    let (left, right) = tokio::join!(publish_one, publish_two);
    assert_eq!(left, StatusCode::CREATED);
    assert_eq!(right, StatusCode::CREATED);

    for replica in [&first, &second] {
        let packument = replica.packument("shared-pkg").await;
        let versions = packument["versions"].as_object().expect("versions");
        assert!(versions.contains_key("1.0.0"), "1.0.0 is missing: {versions:?}");
        assert!(versions.contains_key("2.0.0"), "2.0.0 is missing: {versions:?}");
    }
}

/// A staged publish is shared state: it can be created on one replica and
/// approved on another, and it is spent once it has been.
#[tokio::test]
async fn a_stage_created_on_one_replica_is_approved_on_another() {
    let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let first = Replica::start(&store).await;
    let second = Replica::start(&store).await;

    let stage_id = first.stage("staged-pkg", "1.0.0", b"the tarball").await;
    assert_eq!(second.approve(&stage_id).await, StatusCode::CREATED);
    assert!(first.packument("staged-pkg").await["versions"]["1.0.0"].is_object());
    assert_eq!(
        first.approve(&stage_id).await,
        StatusCode::NOT_FOUND,
        "the stage is spent on every replica",
    );
}

/// Approving the same stage on two replicas at once publishes it once. The
/// approval claims the record with a conditional write, so the second replica
/// finds a record that is no longer the one it read.
#[tokio::test]
async fn one_stage_approved_on_two_replicas_publishes_once() {
    let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let first = Replica::start(&store).await;
    let second = Replica::start(&store).await;

    let stage_id = first.stage("staged-pkg", "1.0.0", b"the tarball").await;
    let approve_here = first.approve(&stage_id);
    let approve_there = second.approve(&stage_id);
    let (left, right) = tokio::join!(approve_here, approve_there);
    let approved =
        [left, right].into_iter().filter(|status| *status == StatusCode::CREATED).count();
    assert_eq!(approved, 1, "exactly one approval may publish: {left}, {right}");

    let packument = first.packument("staged-pkg").await;
    assert_eq!(packument["versions"].as_object().expect("versions").len(), 1);
    let (status, listing) = second.send("GET", "/-/stage", Body::empty()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing["total"], 0, "the stage is spent");
}

/// A dist-tag moved through one replica is what every replica serves.
#[tokio::test]
async fn a_dist_tag_written_on_one_replica_is_served_by_the_other() {
    let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let first = Replica::start(&store).await;
    let second = Replica::start(&store).await;

    assert_eq!(first.publish("tagged-pkg", "1.0.0", b"one").await, StatusCode::CREATED);
    assert_eq!(second.publish("tagged-pkg", "2.0.0", b"two").await, StatusCode::CREATED);
    let (status, _) =
        first.send("PUT", "/-/package/tagged-pkg/dist-tags/next", Body::from(r#""2.0.0""#)).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, tags) = second.send("GET", "/-/package/tagged-pkg/dist-tags", Body::empty()).await;
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
    payload["token"].as_str().expect("token in response").to_string()
}

fn publish_doc(name: &str, version: &str, tarball: &[u8]) -> Value {
    let filename = format!("{name}-{version}.tgz");
    json!({
        "_id": name,
        "name": name,
        "description": "test",
        "dist-tags": { "latest": version },
        "versions": {
            version: {
                "name": name,
                "version": version,
                "dist": {
                    "tarball": format!("http://localhost:4873/{name}/-/{filename}"),
                    "shasum": sha1_hex(tarball),
                    "integrity": sri_sha512(tarball),
                }
            }
        },
        "_attachments": {
            filename: {
                "content_type": "application/octet-stream",
                "data": BASE64.encode(tarball),
                "length": tarball.len()
            }
        }
    })
}

fn sri_sha512(bytes: &[u8]) -> String {
    let mut opts = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha512);
    opts.input(bytes);
    opts.result().to_string()
}

fn sha1_hex(bytes: &[u8]) -> String {
    let mut opts = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha1);
    opts.input(bytes);
    let integrity = opts.result();
    let digest_bytes = BASE64.decode(&integrity.hashes[0].digest).unwrap();
    digest_bytes.iter().fold(String::with_capacity(40), |mut acc, byte| {
        write!(acc, "{byte:02x}").unwrap();
        acc
    })
}
