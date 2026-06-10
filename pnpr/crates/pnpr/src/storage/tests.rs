use super::*;
use tempfile::TempDir;

fn storage_in(tmp: &TempDir) -> Storage {
    Storage::new(&HostedStoreConfig::Fs, tmp.path().join("storage"), tmp.path().join("cache"))
}

fn pkg(name: &str) -> PackageName {
    PackageName::parse(name).unwrap()
}

fn validators(etag: Option<&str>, last_modified: Option<&str>) -> CacheValidators {
    CacheValidators {
        etag: etag.map(str::to_string),
        last_modified: last_modified.map(str::to_string),
    }
}

/// The cached-store sidecar path `write_cached_packument` maintains.
fn sidecar_path(tmp: &TempDir, name: &str) -> PathBuf {
    tmp.path().join("cache").join(name).join(PACKUMENT_META_FILE)
}

/// Read an entry back as `Stale` so its validators can be inspected. The
/// write is aged past a 1ms TTL with a short sleep.
async fn read_stale_validators(storage: &Storage, name: &PackageName) -> CacheValidators {
    tokio::time::sleep(Duration::from_millis(20)).await;
    match storage.read_cached_packument_entry(name, Duration::from_millis(1)).await.unwrap() {
        Some(CachedPackument::Stale(validators)) => validators,
        other => panic!("expected a stale entry, got {other:?}"),
    }
}

#[tokio::test]
async fn fresh_entry_returns_its_body() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = pkg("foo");
    let body = br#"{"name":"foo"}"#;

    storage.write_cached_packument(&name, body, &validators(Some(r#""abc""#), None)).await.unwrap();

    match storage.read_cached_packument_entry(&name, Duration::from_mins(1)).await.unwrap() {
        Some(CachedPackument::Fresh(bytes)) => assert_eq!(bytes, body),
        other => panic!("expected a fresh entry, got {other:?}"),
    }
}

#[tokio::test]
async fn stale_entry_returns_its_validators() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = pkg("foo");

    storage
        .write_cached_packument(
            &name,
            b"{}",
            &validators(Some(r#""abc""#), Some("Wed, 21 Oct 2015 07:28:00 GMT")),
        )
        .await
        .unwrap();

    let validators = read_stale_validators(&storage, &name).await;
    assert_eq!(validators.etag.as_deref(), Some(r#""abc""#));
    assert_eq!(validators.last_modified.as_deref(), Some("Wed, 21 Oct 2015 07:28:00 GMT"));
}

#[tokio::test]
async fn missing_cached_packument_reads_as_none() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let entry =
        storage.read_cached_packument_entry(&pkg("absent"), Duration::from_mins(1)).await.unwrap();
    assert!(entry.is_none());
}

#[tokio::test]
async fn empty_validators_remove_a_previously_written_sidecar() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = pkg("foo");

    storage.write_cached_packument(&name, b"{}", &validators(Some(r#""v1""#), None)).await.unwrap();
    assert!(sidecar_path(&tmp, "foo").exists(), "validators write a sidecar");

    // A later refresh whose upstream sends no validators must drop the
    // stale sidecar so the next read can't replay an outdated ETag.
    storage.write_cached_packument(&name, b"{}", &CacheValidators::default()).await.unwrap();
    assert!(!sidecar_path(&tmp, "foo").exists(), "empty validators remove the sidecar");

    assert!(read_stale_validators(&storage, &name).await.is_empty());
}

#[tokio::test]
async fn writing_without_validators_is_a_noop_when_no_sidecar_exists() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = pkg("foo");

    // First write already carries no validators: removing the (absent)
    // sidecar must be a benign no-op, not an error.
    storage.write_cached_packument(&name, b"{}", &CacheValidators::default()).await.unwrap();
    assert!(!sidecar_path(&tmp, "foo").exists());

    assert!(read_stale_validators(&storage, &name).await.is_empty());
}

#[tokio::test]
async fn malformed_sidecar_reads_as_empty_validators() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = pkg("foo");
    storage.write_cached_packument(&name, b"{}", &validators(Some(r#""v1""#), None)).await.unwrap();

    // A damaged sidecar must degrade to empty validators (forcing an
    // unconditional refresh) rather than failing the read.
    fs::write(sidecar_path(&tmp, "foo"), b"not json").await.unwrap();

    assert!(read_stale_validators(&storage, &name).await.is_empty());
}

#[tokio::test]
async fn read_cached_packument_returns_bytes_regardless_of_age() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = pkg("foo");
    storage
        .write_cached_packument(&name, br#"{"v":1}"#, &CacheValidators::default())
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(10)).await;
    let bytes = storage.read_cached_packument(&name).await.unwrap();
    assert_eq!(bytes.as_deref(), Some(&br#"{"v":1}"#[..]));
}
