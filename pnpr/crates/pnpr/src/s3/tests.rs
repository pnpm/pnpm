use super::*;
use crate::package_name::PackageName;
use object_store::memory::InMemory;
use std::sync::Arc;
use tempfile::tempdir;

fn store_with_prefix(prefix: &str) -> (S3Store, tempfile::TempDir) {
    let staging = tempdir().expect("tempdir");
    let inner: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    // `S3Store::new` takes the already-normalized prefix, exactly as
    // `config.rs` feeds it from `S3Settings::normalized_prefix`.
    let normalized = S3Settings {
        bucket: "b".to_string(),
        region: None,
        endpoint: None,
        prefix: Some(prefix.to_string()),
        access_key_id: None,
        secret_access_key: None,
        force_path_style: None,
        allow_http: None,
    }
    .normalized_prefix();
    let store = S3Store::new(inner, normalized, staging.path().to_path_buf());
    (store, staging)
}

fn pkg(name: &str) -> PackageName {
    PackageName::parse(name).expect("valid package name")
}

async fn collect(body: Body) -> Vec<u8> {
    axum::body::to_bytes(body, usize::MAX).await.expect("read body").to_vec()
}

/// Stage a tarball through the same path the publish flow uses: write
/// the decoded bytes to the reserved local tmp file, then upload.
/// (Cleaning up the staging file is the `HostedStore` wrapper's job,
/// not the adapter's, so it stays behind here.)
async fn upload(store: &S3Store, name: &PackageName, filename: &str, bytes: &[u8]) {
    let tmp = store.staging_tmp_path(name, filename).await.expect("reserve staging path");
    tokio::fs::write(&tmp, bytes).await.expect("write staging file");
    store.upload_tarball(&tmp, name, filename).await.expect("upload");
}

#[tokio::test]
async fn packument_roundtrips_and_missing_is_none() {
    let (store, _staging) = store_with_prefix("");
    let name = pkg("is-positive");
    assert_eq!(store.read_packument(&name).await.unwrap(), None);
    store.write_packument(&name, br#"{"name":"is-positive"}"#).await.unwrap();
    assert_eq!(
        store.read_packument(&name).await.unwrap().as_deref(),
        Some(&br#"{"name":"is-positive"}"#[..]),
    );
}

#[tokio::test]
async fn tarball_uploads_streams_and_reports_length() {
    let (store, _staging) = store_with_prefix("");
    let name = pkg("is-positive");
    assert!(store.open_tarball(&name, "is-positive-1.0.0.tgz").await.unwrap().is_none());

    let payload = b"a fake tarball payload";
    upload(&store, &name, "is-positive-1.0.0.tgz", payload).await;

    let (body, len) = store.open_tarball(&name, "is-positive-1.0.0.tgz").await.unwrap().unwrap();
    assert_eq!(len, Some(payload.len() as u64));
    assert_eq!(collect(body).await, payload);
}

#[tokio::test]
async fn scoped_keys_and_prefix_are_honored() {
    let (store, _staging) = store_with_prefix("packages");
    let name = pkg("@scope/thing");
    store.write_packument(&name, br#"{"name":"@scope/thing"}"#).await.unwrap();
    upload(&store, &name, "thing-1.0.0.tgz", b"scoped tarball").await;

    let (body, _len) = store.open_tarball(&name, "thing-1.0.0.tgz").await.unwrap().unwrap();
    assert_eq!(collect(body).await, b"scoped tarball");
    assert!(store.read_packument(&name).await.unwrap().is_some());
}

#[tokio::test]
async fn remove_tarball_then_package() {
    let (store, _staging) = store_with_prefix("");
    let name = pkg("is-positive");
    store.write_packument(&name, b"{}").await.unwrap();
    upload(&store, &name, "is-positive-1.0.0.tgz", b"payload").await;

    assert!(store.remove_tarball(&name, "is-positive-1.0.0.tgz").await.unwrap());
    // S3 (and the in-memory store) deletes are idempotent and don't
    // report whether the key existed, so a second delete still succeeds.
    store.remove_tarball(&name, "is-positive-1.0.0.tgz").await.unwrap();
    assert!(store.open_tarball(&name, "is-positive-1.0.0.tgz").await.unwrap().is_none());

    store.remove_package(&name).await.unwrap();
    assert!(store.read_packument(&name).await.unwrap().is_none());
}

#[tokio::test]
async fn lists_hosted_package_names() {
    for prefix in ["", "packages"] {
        let (store, _staging) = store_with_prefix(prefix);
        store.write_packument(&pkg("is-positive"), b"{}").await.unwrap();
        store.write_packument(&pkg("@scope/thing"), b"{}").await.unwrap();
        // A stray tarball-only key must not be mistaken for a package.
        upload(&store, &pkg("is-positive"), "is-positive-1.0.0.tgz", b"x").await;

        let mut names = store.list_package_names().await.unwrap();
        names.sort();
        assert_eq!(names, vec!["@scope/thing".to_string(), "is-positive".to_string()]);
    }
}

#[test]
fn prefix_normalizes() {
    let normalized = |prefix: Option<&str>| {
        S3Settings {
            bucket: "b".to_string(),
            region: None,
            endpoint: None,
            prefix: prefix.map(str::to_string),
            access_key_id: None,
            secret_access_key: None,
            force_path_style: None,
            allow_http: None,
        }
        .normalized_prefix()
    };
    assert_eq!(normalized(None), "");
    assert_eq!(normalized(Some("")), "");
    assert_eq!(normalized(Some("  ")), "");
    assert_eq!(normalized(Some("packages")), "packages/");
    assert_eq!(normalized(Some("/packages/")), "packages/");
}
