use super::{
    inventory::{cas_digest_of_store_file, is_cache_tmp_file, is_managed_sqlite_file},
    is_abandoned_tmp_age,
    orphans::count_dir_contents,
};
use std::{path::Path, time::Duration};

fn is_abandoned_tmp(cache_dir: &Path, path: &Path, age: Duration) -> bool {
    is_cache_tmp_file(cache_dir, path) && is_abandoned_tmp_age(age)
}

#[test]
fn abandoned_tmp_identifies_stale_tmp_files() {
    let cache = Path::new("/cache");
    assert!(!is_abandoned_tmp(
        cache,
        Path::new("/cache/pkg/foo-1.0.0.tgz"),
        Duration::from_hours(2)
    ));
    assert!(!is_abandoned_tmp(cache, Path::new("/cache/pkg/.tmp.123"), Duration::from_mins(1)));
    assert!(is_abandoned_tmp(cache, Path::new("/cache/pkg/.tmp.123"), Duration::from_secs(4000)));
    assert!(!is_abandoned_tmp(
        cache,
        Path::new("/cache/resolver-store/v11/tmp/_tmp_1_0"),
        Duration::from_mins(5),
    ));
    assert!(is_abandoned_tmp(
        cache,
        Path::new("/cache/resolver-store/v11/tmp/_tmp_1_0"),
        Duration::from_secs(4000),
    ));
    assert!(is_abandoned_tmp(
        cache,
        Path::new("/cache/resolver-store/v11/files/stream_1_0"),
        Duration::from_secs(4000),
    ));
}

#[test]
fn cache_tmp_file_matches_proxy_and_resolver_store_temps() {
    let cache = Path::new("/cache");
    assert!(is_cache_tmp_file(cache, Path::new("/cache/~public/h/pkg/.tmp.99")));
    assert!(is_cache_tmp_file(cache, Path::new("/cache/resolver-store/v11/tmp/_tmp_7_2")));
    assert!(is_cache_tmp_file(cache, Path::new("/cache/resolver-store/v11/files/stream_7_2")));
    assert!(!is_cache_tmp_file(cache, Path::new("/cache/resolver-store/v11/files/ab/cd1234")));
}

#[test]
fn managed_sqlite_family_recognizes_verdicts_and_store_index_sidecars() {
    let cache = Path::new("/cache");
    assert!(is_managed_sqlite_file(
        cache,
        Path::new("/cache/resolver-cache/lockfile-verdicts.sqlite"),
    ));
    assert!(is_managed_sqlite_file(
        cache,
        Path::new("/cache/resolver-cache/lockfile-verdicts.sqlite-wal"),
    ));
    assert!(is_managed_sqlite_file(cache, Path::new("/cache/resolver-store/v11/index.db-shm")));
    assert!(!is_managed_sqlite_file(
        cache,
        Path::new("/cache/resolver-cache/v11/metadata-full/foo.json"),
    ));
}

#[test]
fn cas_digest_extracts_hex_for_regular_and_exec_store_files() {
    let cache = Path::new("/cache");
    assert_eq!(
        cas_digest_of_store_file(cache, Path::new("/cache/resolver-store/v11/files/0a/bcdef01234")),
        Some("0abcdef01234".to_string()),
    );
    assert_eq!(
        cas_digest_of_store_file(
            cache,
            Path::new("/cache/resolver-store/v11/files/ff/112233-exec"),
        ),
        Some("ff112233".to_string()),
    );
    assert_eq!(
        cas_digest_of_store_file(cache, Path::new("/cache/resolver-store/v11/files/stream_1_0")),
        None,
    );
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
