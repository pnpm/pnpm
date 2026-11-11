use super::{Fingerprint, LOCAL_TIMESTAMP_STABILITY, verified_local_file};
use crate::BlobId;
use pnpr_package_name::Ecosystem;
use std::time::{Duration, Instant, SystemTime};
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
    tokio::time::sleep(LOCAL_TIMESTAMP_STABILITY).await;
    let file = fs::File::open(&path).await.unwrap();
    drop(verified_local_file(file, &key, cache_key.clone()).await.unwrap());
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

#[test]
fn local_verification_cache_requires_stable_timestamps() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("archive");
    std::fs::write(&path, b"original").unwrap();
    let now = SystemTime::UNIX_EPOCH
        .checked_add(Duration::from_secs(10))
        .unwrap();
    let mut fingerprint = Fingerprint::local(&std::fs::metadata(&path).unwrap()).unwrap();
    let Fingerprint::Local { modified, identity, .. } = &mut fingerprint else {
        panic!("local fingerprint")
    };
    *modified = now.checked_sub(LOCAL_TIMESTAMP_STABILITY).unwrap();
    #[cfg(unix)]
    {
        identity.changed = (8, 0);
    }
    #[cfg(not(unix))]
    {
        identity.created = Some(now.checked_sub(LOCAL_TIMESTAMP_STABILITY).unwrap());
    }
    let mut recent_identity = identity.clone();
    #[cfg(unix)]
    {
        recent_identity.changed = (10, 0);
    }
    #[cfg(not(unix))]
    {
        recent_identity.created = Some(now);
    }
    assert!(!recent_identity.has_stable_timestamp(now));
    assert!(fingerprint.has_stable_timestamps(now));
    let Fingerprint::Local { modified, .. } = &mut fingerprint else { unreachable!() };
    *modified = now;
    assert!(!fingerprint.has_stable_timestamps(now));
    let Fingerprint::Local { modified, .. } = &mut fingerprint else { unreachable!() };
    *modified = now.checked_add(LOCAL_TIMESTAMP_STABILITY).unwrap();
    assert!(!fingerprint.has_stable_timestamps(now));
}

#[tokio::test]
async fn future_dated_archives_are_verified_even_if_a_matching_fingerprint_is_cached() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("archive");
    let key = BlobId::of_bytes(Ecosystem::Npm, b"original").key(1);
    fs::write(&path, b"tampered").await.unwrap();
    let modified = SystemTime::now()
        .checked_add(Duration::from_hours(1))
        .unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
    let fingerprint = Fingerprint::local(&std::fs::metadata(&path).unwrap()).unwrap();
    let cache_key = path.to_string_lossy().into_owned();
    super::remember(cache_key.clone(), fingerprint);
    let file = fs::File::open(&path).await.unwrap();
    assert!(verified_local_file(file, &key, cache_key).await.is_err());
}
