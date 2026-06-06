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

#[tokio::test]
async fn cached_packument_round_trips_with_validators() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = pkg("foo");
    let body = br#"{"name":"foo"}"#;

    storage
        .write_cached_packument(
            &name,
            body,
            &validators(Some(r#""abc""#), Some("Wed, 21 Oct 2015 07:28:00 GMT")),
        )
        .await
        .unwrap();

    let entry = storage
        .read_cached_packument_entry(&name, Duration::from_secs(60))
        .await
        .unwrap()
        .expect("entry present");
    assert_eq!(entry.bytes, body);
    assert!(entry.fresh, "a just-written entry is within ttl");
    assert_eq!(entry.validators.etag.as_deref(), Some(r#""abc""#));
    assert_eq!(entry.validators.last_modified.as_deref(), Some("Wed, 21 Oct 2015 07:28:00 GMT"));
}

#[tokio::test]
async fn missing_cached_packument_reads_as_none() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let entry =
        storage.read_cached_packument_entry(&pkg("absent"), Duration::from_secs(60)).await.unwrap();
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

    let entry =
        storage.read_cached_packument_entry(&name, Duration::from_secs(60)).await.unwrap().unwrap();
    assert!(entry.validators.is_empty());
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

    let entry =
        storage.read_cached_packument_entry(&name, Duration::from_secs(60)).await.unwrap().unwrap();
    assert!(entry.validators.is_empty());
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

    let entry =
        storage.read_cached_packument_entry(&name, Duration::from_secs(60)).await.unwrap().unwrap();
    assert!(entry.validators.is_empty());
}

#[tokio::test]
async fn entry_past_ttl_is_stale_but_keeps_bytes_and_validators() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = pkg("foo");
    storage.write_cached_packument(&name, b"{}", &validators(Some(r#""v1""#), None)).await.unwrap();

    tokio::time::sleep(Duration::from_millis(20)).await;
    let entry = storage
        .read_cached_packument_entry(&name, Duration::from_millis(1))
        .await
        .unwrap()
        .unwrap();
    assert!(!entry.fresh, "an entry older than ttl is stale");
    // A stale entry still surfaces its bytes + validators so the caller
    // can revalidate conditionally and fall back to the stale body.
    assert_eq!(entry.bytes, b"{}");
    assert_eq!(entry.validators.etag.as_deref(), Some(r#""v1""#));
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
