//! Integration tests for the publish commit journal: a publish that
//! crashed between sealing and applying is rolled forward on startup,
//! an unsealed one is rolled back, and the journal directory carries
//! no residue after a successful publish.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use pnpr::{Config, MaxUsers, recover_publish_journal, router};
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use tower::ServiceExt;

fn static_config(storage: PathBuf) -> Config {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let mut config = Config::static_serve(listen, storage);
    config.public_url = "http://example.test".to_string();
    config.auth.htpasswd.max_users = MaxUsers::Unlimited;
    config
}

async fn body_json(body: Body) -> Value {
    let bytes = to_bytes(body, usize::MAX).await.expect("read body");
    serde_json::from_slice(&bytes).expect("body parses as JSON")
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
    let payload = body_json(response.into_body()).await;
    payload["token"].as_str().expect("token in response").to_string()
}

fn packument(name: &str, version: &str, tarball: &[u8]) -> Value {
    json!({
        "name": name,
        "dist-tags": { "latest": version },
        "versions": {
            version: {
                "name": name,
                "version": version,
                "dist": {
                    "tarball": format!("http://localhost:4873/{name}/-/{name}-{version}.tgz"),
                    "integrity": sri_sha512(tarball),
                },
            },
        },
    })
}

/// Lay down the on-disk state a publish leaves behind right after
/// sealing its journal transaction and before applying it: the staged
/// tmp tarball in the package directory plus the journal entry. With
/// `sealed`, the `commit` marker is present too.
fn fabricate_crashed_publish(
    storage: &Path,
    name: &str,
    version: &str,
    tarball: &[u8],
    sealed: bool,
) -> PathBuf {
    let pkg_dir = storage.join(name);
    std::fs::create_dir_all(&pkg_dir).unwrap();
    let tmp_path = pkg_dir.join(format!("{name}-{version}.tgz.tmp.999.0"));
    std::fs::write(&tmp_path, tarball).unwrap();

    let txn_dir = storage.join(".pnpr-journal").join("0000000000000001-999-0");
    std::fs::create_dir_all(&txn_dir).unwrap();
    std::fs::write(
        txn_dir.join("packument-0.json"),
        serde_json::to_vec_pretty(&packument(name, version, tarball)).unwrap(),
    )
    .unwrap();
    let manifest = json!({
        "packages": [{
            "name": name,
            "packument_file": "packument-0.json",
            "tarballs": [{
                "filename": format!("{name}-{version}.tgz"),
                "tmp_path": tmp_path,
            }],
        }],
    });
    std::fs::write(txn_dir.join("manifest.json"), serde_json::to_vec_pretty(&manifest).unwrap())
        .unwrap();
    if sealed {
        std::fs::write(txn_dir.join("commit"), b"").unwrap();
    }
    tmp_path
}

#[tokio::test]
async fn recovery_rolls_a_sealed_transaction_forward() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let tarball = b"crashed-tarball-bytes";
    let tmp_path = fabricate_crashed_publish(&storage, "crash-fwd", "1.0.0", tarball, true);

    recover_publish_journal(&static_config(storage.clone())).await.unwrap();

    // The package became fully visible: packument and tarball in
    // place, staged tmp file promoted, journal entry consumed.
    let on_disk: Value = serde_json::from_slice(
        &std::fs::read(storage.join("crash-fwd/package.json")).expect("packument written"),
    )
    .unwrap();
    assert_eq!(on_disk["versions"]["1.0.0"]["version"], "1.0.0");
    assert_eq!(on_disk["dist-tags"]["latest"], "1.0.0");
    assert_eq!(std::fs::read(storage.join("crash-fwd/crash-fwd-1.0.0.tgz")).unwrap(), tarball);
    assert!(!tmp_path.exists(), "staged tmp file should be promoted away");
    assert!(
        std::fs::read_dir(storage.join(".pnpr-journal")).unwrap().next().is_none(),
        "journal should be empty after recovery",
    );

    // And it serves.
    let app = router(static_config(storage));
    let response =
        app.oneshot(Request::get("/crash-fwd").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn recovery_rolls_an_unsealed_transaction_back() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let tmp_path =
        fabricate_crashed_publish(&storage, "crash-back", "1.0.0", b"aborted-bytes", false);

    recover_publish_journal(&static_config(storage.clone())).await.unwrap();

    // Nothing of the aborted publish survives.
    assert!(!storage.join("crash-back/package.json").exists());
    assert!(!storage.join("crash-back/crash-back-1.0.0.tgz").exists());
    assert!(!tmp_path.exists(), "staged tmp file should be deleted");
    assert!(std::fs::read_dir(storage.join(".pnpr-journal")).unwrap().next().is_none());
}

/// Replaying a sealed transaction merges into the current on-disk
/// packument instead of overwriting it, so versions published between
/// the failed apply and the restart survive the roll-forward.
#[tokio::test]
async fn roll_forward_merges_with_versions_published_after_the_crash() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    fabricate_crashed_publish(&storage, "crash-merge", "1.0.0", b"old-bytes", true);
    // A newer version landed on disk before recovery ran.
    let pkg_dir = storage.join("crash-merge");
    std::fs::write(
        pkg_dir.join("package.json"),
        serde_json::to_vec_pretty(&packument("crash-merge", "2.0.0", b"new-bytes")).unwrap(),
    )
    .unwrap();

    recover_publish_journal(&static_config(storage.clone())).await.unwrap();

    let on_disk: Value =
        serde_json::from_slice(&std::fs::read(pkg_dir.join("package.json")).unwrap()).unwrap();
    assert_eq!(on_disk["versions"]["1.0.0"]["version"], "1.0.0");
    assert_eq!(on_disk["versions"]["2.0.0"]["version"], "2.0.0", "newer version must survive");
}

#[tokio::test]
async fn recovery_is_a_no_op_without_a_journal_directory() {
    let tmp = TempDir::new().unwrap();
    recover_publish_journal(&static_config(tmp.path().to_path_buf())).await.unwrap();
}

#[tokio::test]
async fn successful_batch_publish_leaves_no_journal_residue() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let token = add_user_and_get_token(app.clone(), "alice", "secret").await;

    let tarball = b"journaled-bytes";
    let mut doc = packument("residue-pkg", "1.0.0", tarball);
    doc["_id"] = json!("residue-pkg");
    doc["versions"]["1.0.0"]["dist"]["shasum"] = json!(sha1_hex(tarball));
    doc["_attachments"] = json!({
        "residue-pkg-1.0.0.tgz": {
            "content_type": "application/octet-stream",
            "data": BASE64.encode(tarball),
            "length": tarball.len(),
        },
    });
    let request = Request::put("/-/pnpm/v1/publish")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&json!({ "packages": [doc] })).unwrap()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    assert!(storage.join("residue-pkg/package.json").exists());
    let journal_root = storage.join(".pnpr-journal");
    let leftover: Vec<_> = match std::fs::read_dir(&journal_root) {
        Ok(entries) => entries.map(|entry| entry.unwrap().path()).collect(),
        Err(_) => Vec::new(),
    };
    assert!(leftover.is_empty(), "journal entries must be removed after apply: {leftover:?}");
}

/// Compute the SRI `sha512-...` string the way npm clients send it
/// in `dist.integrity`.
fn sri_sha512(bytes: &[u8]) -> String {
    let mut opts = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha512);
    opts.input(bytes);
    opts.result().to_string()
}

/// Compute the 40-char hex SHA-1 the way npm clients send it in the
/// legacy `dist.shasum` field.
fn sha1_hex(bytes: &[u8]) -> String {
    let mut opts = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha1);
    opts.input(bytes);
    let integrity = opts.result();
    let digest_base64 = &integrity.hashes[0].digest;
    let digest_bytes = BASE64.decode(digest_base64).unwrap();
    digest_bytes.iter().fold(String::with_capacity(40), |mut acc, byte| {
        use std::fmt::Write;
        write!(acc, "{byte:02x}").unwrap();
        acc
    })
}
