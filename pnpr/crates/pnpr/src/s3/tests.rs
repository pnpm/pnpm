use super::{Body, ObjectStore, S3Settings, S3Store};
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

async fn write_packument(store: &S3Store, name: &PackageName, bytes: &[u8]) {
    assert!(store.write_packument_if_current(name, bytes, None).await.unwrap());
}

#[tokio::test]
async fn packument_roundtrips_and_missing_is_none() {
    let (store, _staging) = store_with_prefix("");
    let name = pkg("is-positive");
    assert_eq!(store.read_packument(&name).await.unwrap(), None);
    write_packument(&store, &name, br#"{"name":"is-positive"}"#).await;
    assert_eq!(
        store.read_packument(&name).await.unwrap().as_deref(),
        Some(&br#"{"name":"is-positive"}"#[..]),
    );
}

#[tokio::test]
async fn stale_packument_update_is_rejected() {
    let (store, _staging) = store_with_prefix("");
    let name = pkg("racer");
    store.write_packument_if_current(&name, br#"{"name":"racer"}"#, None).await.unwrap();

    let first_read = store.read_packument_for_update(&name).await.unwrap().unwrap();
    let second_read = store.read_packument_for_update(&name).await.unwrap().unwrap();

    let first_written = store
        .write_packument_if_current(
            &name,
            br#"{"name":"racer","versions":{"1.0.0":{"version":"1.0.0"}}}"#,
            Some(&first_read.version),
        )
        .await
        .unwrap();
    assert!(first_written);

    let second_written = store
        .write_packument_if_current(
            &name,
            br#"{"name":"racer","versions":{"2.0.0":{"version":"2.0.0"}}}"#,
            Some(&second_read.version),
        )
        .await
        .unwrap();
    assert!(!second_written);
    assert_eq!(
        store.read_packument(&name).await.unwrap().as_deref(),
        Some(&br#"{"name":"racer","versions":{"1.0.0":{"version":"1.0.0"}}}"#[..]),
    );
}

#[tokio::test]
async fn deleted_packument_update_is_rejected() {
    let (store, _staging) = store_with_prefix("");
    let name = pkg("removed-racer");
    write_packument(&store, &name, br#"{"name":"removed-racer"}"#).await;

    let read = store.read_packument_for_update(&name).await.unwrap().unwrap();
    store.remove_package(&name).await.unwrap();

    let written = store
        .write_packument_if_current(
            &name,
            br#"{"name":"removed-racer","versions":{"1.0.0":{"version":"1.0.0"}}}"#,
            Some(&read.version),
        )
        .await
        .unwrap();
    assert!(!written);
    assert!(store.read_packument(&name).await.unwrap().is_none());
}

#[tokio::test]
async fn concurrent_tarball_finalize_does_not_overwrite() {
    use crate::storage::TarballFinalize;
    let (store, _staging) = store_with_prefix("");
    let name = pkg("racer");
    let file = "racer-1.0.0.tgz";

    let tmp = store.staging_tmp_path(&name, file).await.unwrap();
    tokio::fs::write(&tmp, b"tarball A").await.unwrap();
    assert_eq!(store.upload_tarball(&tmp, &name, file).await.unwrap(), TarballFinalize::Written);

    // Re-promoting byte-identical content is a tolerated no-op, so idempotent
    // journal roll-forward and concurrent identical publishes don't conflict.
    let tmp = store.staging_tmp_path(&name, file).await.unwrap();
    tokio::fs::write(&tmp, b"tarball A").await.unwrap();
    assert_eq!(
        store.upload_tarball(&tmp, &name, file).await.unwrap(),
        TarballFinalize::AlreadyIdentical,
    );

    // Different bytes for the same version's key are rejected without
    // overwriting the first writer's tarball.
    let tmp = store.staging_tmp_path(&name, file).await.unwrap();
    tokio::fs::write(&tmp, b"tarball B").await.unwrap();
    assert_eq!(store.upload_tarball(&tmp, &name, file).await.unwrap(), TarballFinalize::Conflict);

    let (body, _len) = store.open_tarball(&name, file).await.unwrap().unwrap();
    assert_eq!(collect(body).await, b"tarball A");
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
    write_packument(&store, &name, br#"{"name":"@scope/thing"}"#).await;
    upload(&store, &name, "thing-1.0.0.tgz", b"scoped tarball").await;

    let (body, _len) = store.open_tarball(&name, "thing-1.0.0.tgz").await.unwrap().unwrap();
    assert_eq!(collect(body).await, b"scoped tarball");
    assert!(store.read_packument(&name).await.unwrap().is_some());
}

#[tokio::test]
async fn remove_tarball_then_package() {
    let (store, _staging) = store_with_prefix("");
    let name = pkg("is-positive");
    write_packument(&store, &name, b"{}").await;
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
        write_packument(&store, &pkg("is-positive"), b"{}").await;
        write_packument(&store, &pkg("@scope/thing"), b"{}").await;
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
