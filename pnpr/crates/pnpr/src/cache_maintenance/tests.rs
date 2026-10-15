use super::{count_dir_contents, is_abandoned_tmp};
use std::{path::Path, time::Duration};

#[test]
fn abandoned_tmp_identifies_stale_tmp_files() {
    assert!(!is_abandoned_tmp(Path::new("/cache/pkg/foo-1.0.0.tgz"), Duration::from_hours(2)));
    assert!(!is_abandoned_tmp(Path::new("/cache/pkg/.tmp.123"), Duration::from_mins(1)));
    assert!(is_abandoned_tmp(Path::new("/cache/pkg/.tmp.123"), Duration::from_secs(4000)));
}

#[tokio::test]
async fn count_dir_contents_counts_files_and_bytes() {
    let tmp = tempfile::tempdir().unwrap();
    let file1 = tmp.path().join("a.txt");
    let file2 = tmp.path().join("sub/b.txt");
    tokio::fs::create_dir_all(tmp.path().join("sub")).await.unwrap();
    tokio::fs::write(&file1, b"hello").await.unwrap();
    tokio::fs::write(&file2, b"world!").await.unwrap();

    let (count, bytes) = count_dir_contents(tmp.path()).await.unwrap();
    assert_eq!(count, 2);
    assert_eq!(bytes, 11);
}
