use super::{
    AsyncWriteExt, CacheValidators, CachedPackument, CachedTarballIntegrity, Duration, ErrorKind,
    HostedStoreConfig, PACKUMENT_META_FILE, PackageName, PathBuf, RegistryError, Storage,
    TARBALL_INTEGRITY_SUFFIX, TarballWrite, create_tmp_file_with, fs,
};
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

/// The uplink name the cache tests key their validators under.
const UPLINK: &str = "npmjs";

/// Read an entry back as `Stale` and return `UPLINK`'s validators so they
/// can be inspected. The write is aged past a 1ms TTL with a short sleep.
async fn read_stale_validators(storage: &Storage, name: &PackageName) -> CacheValidators {
    tokio::time::sleep(Duration::from_millis(20)).await;
    match storage.read_cached_packument_entry(name, Duration::from_millis(1)).await.unwrap() {
        Some(CachedPackument::Stale(validators)) => validators.get(UPLINK),
        other => panic!("expected a stale entry, got {other:?}"),
    }
}

#[tokio::test]
async fn fresh_entry_returns_its_body() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = pkg("foo");
    let body = br#"{"name":"foo"}"#;

    storage
        .write_cached_packument(&name, body, UPLINK, &validators(Some(r#""abc""#), None))
        .await
        .unwrap();

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
            UPLINK,
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

    storage
        .write_cached_packument(&name, b"{}", UPLINK, &validators(Some(r#""v1""#), None))
        .await
        .unwrap();
    assert!(sidecar_path(&tmp, "foo").exists(), "validators write a sidecar");

    // A later refresh whose upstream sends no validators must drop the
    // stale sidecar so the next read can't replay an outdated ETag.
    storage
        .write_cached_packument(&name, b"{}", UPLINK, &CacheValidators::default())
        .await
        .unwrap();
    assert!(!sidecar_path(&tmp, "foo").exists(), "empty validators remove the sidecar");

    assert!(read_stale_validators(&storage, &name).await.is_empty());
}

#[tokio::test]
async fn writing_a_new_body_scopes_validators_to_its_origin_uplink() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = pkg("foo");

    // The primary fills the cache, then a secondary replaces the shared body.
    storage
        .write_cached_packument(
            &name,
            br#"{"v":"a"}"#,
            "primary",
            &validators(Some(r#""a""#), None),
        )
        .await
        .unwrap();
    storage
        .write_cached_packument(
            &name,
            br#"{"v":"b"}"#,
            "secondary",
            &validators(Some(r#""b""#), None),
        )
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(20)).await;
    let map =
        match storage.read_cached_packument_entry(&name, Duration::from_millis(1)).await.unwrap() {
            Some(CachedPackument::Stale(map)) => map,
            other => panic!("expected a stale entry, got {other:?}"),
        };
    // Only the body's origin keeps validators: the primary's are dropped, so a
    // later refresh sends the primary an unconditional GET and a 304 can only
    // come from the secondary — the uplink that actually wrote the body.
    assert_eq!(map.get("secondary").etag.as_deref(), Some(r#""b""#));
    assert!(map.get("primary").is_empty());
}

#[tokio::test]
async fn writing_without_validators_is_a_noop_when_no_sidecar_exists() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = pkg("foo");

    // First write already carries no validators: removing the (absent)
    // sidecar must be a benign no-op, not an error.
    storage
        .write_cached_packument(&name, b"{}", UPLINK, &CacheValidators::default())
        .await
        .unwrap();
    assert!(!sidecar_path(&tmp, "foo").exists());

    assert!(read_stale_validators(&storage, &name).await.is_empty());
}

#[tokio::test]
async fn malformed_sidecar_reads_as_empty_validators() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = pkg("foo");
    storage
        .write_cached_packument(&name, b"{}", UPLINK, &validators(Some(r#""v1""#), None))
        .await
        .unwrap();

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
        .write_cached_packument(&name, br#"{"v":1}"#, UPLINK, &CacheValidators::default())
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(10)).await;
    let bytes = storage.read_cached_packument(&name).await.unwrap();
    assert_eq!(bytes.as_deref(), Some(&br#"{"v":1}"#[..]));
}

#[tokio::test]
async fn hosted_tarball_under_non_directory_package_path_is_an_error() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = pkg("foo");
    let storage_root = tmp.path().join("storage");
    fs::create_dir_all(&storage_root).await.unwrap();
    fs::write(storage_root.join("foo"), b"not a directory").await.unwrap();

    let Err(err) = storage.open_hosted_tarball(&name, "foo-1.0.0.tgz").await else {
        panic!("expected hosted tarball open to fail");
    };
    match err {
        RegistryError::Io(err) => assert_eq!(err.kind(), ErrorKind::NotADirectory),
        other => panic!("expected I/O error, got {other:?}"),
    }
}

#[tokio::test]
async fn temp_file_creation_retries_existing_candidate_without_overwriting() {
    let tmp = TempDir::new().unwrap();
    let final_path = tmp.path().join("foo-1.0.0.tgz");
    let occupied = tmp.path().join("foo-1.0.0.tgz.tmp.occupied");
    let retry = tmp.path().join("foo-1.0.0.tgz.tmp.retry");
    fs::write(&occupied, b"occupied").await.unwrap();

    let mut first = true;
    let (mut file, path) = create_tmp_file_with(&final_path, |_| {
        if std::mem::replace(&mut first, false) { occupied.clone() } else { retry.clone() }
    })
    .await
    .unwrap();
    assert_eq!(path, retry);

    file.write_all(b"new").await.unwrap();
    file.sync_all().await.unwrap();
    drop(file);

    assert_eq!(fs::read(&occupied).await.unwrap(), b"occupied");
    assert_eq!(fs::read(&retry).await.unwrap(), b"new");
}

#[cfg(unix)]
#[tokio::test]
async fn temp_file_creation_does_not_follow_symlink_candidate() {
    use std::os::unix::fs::symlink;

    let tmp = TempDir::new().unwrap();
    let final_path = tmp.path().join("foo-1.0.0.tgz");
    let victim = tmp.path().join("victim");
    let symlink_path = tmp.path().join("foo-1.0.0.tgz.tmp.symlink");
    let retry = tmp.path().join("foo-1.0.0.tgz.tmp.retry");
    fs::write(&victim, b"victim").await.unwrap();
    symlink(&victim, &symlink_path).unwrap();

    let mut first = true;
    let (mut file, path) = create_tmp_file_with(&final_path, |_| {
        if std::mem::replace(&mut first, false) { symlink_path.clone() } else { retry.clone() }
    })
    .await
    .unwrap();
    assert_eq!(path, retry);

    file.write_all(b"new").await.unwrap();
    file.sync_all().await.unwrap();
    drop(file);

    assert_eq!(fs::read(&victim).await.unwrap(), b"victim");
    assert_eq!(fs::read(&retry).await.unwrap(), b"new");
    assert!(std::fs::symlink_metadata(&symlink_path).unwrap().file_type().is_symlink());
}

#[tokio::test]
async fn failed_tarball_finalize_removes_tmp_file() {
    let tmp = TempDir::new().unwrap();
    let tmp_path = tmp.path().join("foo-1.0.0.tgz.tmp.test");
    let final_path = tmp.path().join("foo-1.0.0.tgz");
    fs::create_dir(&final_path).await.unwrap();
    fs::write(final_path.join("block-rename"), b"occupied").await.unwrap();

    let file = fs::File::create(&tmp_path).await.unwrap();
    let mut write = TarballWrite { file: Some(file), tmp_path: Some(tmp_path.clone()), final_path };
    write.write_all(b"tarball").await.unwrap();

    assert!(write.finalize().await.is_err());
    assert!(!tmp_path.exists(), "failed finalization must remove its temporary file");
}
