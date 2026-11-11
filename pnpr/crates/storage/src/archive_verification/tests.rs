use super::verified_local_file;
use crate::BlobId;
use pnpr_package_name::Ecosystem;
use std::time::Instant;
use tempfile::TempDir;
use tokio::fs;

#[tokio::test]
async fn unchanged_large_archives_reuse_verification_and_changed_bytes_are_rejected() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("archive");
    let bytes = vec![42; 32 * 1024 * 1024];
    let key = BlobId::of_bytes(Ecosystem::Npm, &bytes).key(1);
    fs::write(&path, &bytes).await.unwrap();
    let cache_key = path.to_string_lossy().into_owned();
    let cold_start = Instant::now();
    let file = fs::File::open(&path).await.unwrap();
    drop(verified_local_file(file, &key, cache_key.clone()).await.unwrap());
    let cold = cold_start.elapsed();
    let warm_start = Instant::now();
    for _ in 0..10 {
        let file = fs::File::open(&path).await.unwrap();
        drop(verified_local_file(file, &key, cache_key.clone()).await.unwrap());
    }
    eprintln!(
        "32 MiB archive verification: cold={cold:?}, warm mean={:?}",
        warm_start.elapsed() / 10,
    );
    fs::write(&path, vec![43; bytes.len()]).await.unwrap();
    let file = fs::File::open(&path).await.unwrap();
    assert!(verified_local_file(file, &key, cache_key).await.is_err());
}
