use super::{
    AsyncWriteExt, ErrorKind, HostedStoreConfig, PackageName, RegistryError, Storage, TarballWrite,
    create_tmp_file_with, fs,
};
use tempfile::TempDir;

fn storage_in(tmp: &TempDir) -> Storage {
    Storage::new(&HostedStoreConfig::Fs, tmp.path().join("storage"), tmp.path().join("cache"))
}

fn pkg(name: &str) -> PackageName {
    PackageName::parse(name).unwrap()
}

#[test]
fn packument_write_conflict_delay_caps_growth() {
    assert_eq!(super::packument_write_conflict_delay(0).as_millis(), 5);
    assert_eq!(super::packument_write_conflict_delay(1).as_millis(), 10);
    assert_eq!(super::packument_write_conflict_delay(6).as_millis(), 250);
    assert_eq!(super::packument_write_conflict_delay(32).as_millis(), 250);
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
