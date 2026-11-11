use super::{
    inventory::{cas_digest_of_store_file, is_cache_tmp_file, is_managed_sqlite_file},
    is_abandoned_tmp_age,
    orphans::count_dir_contents,
};
use pnpm_fs::OpenDir;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

fn is_abandoned_tmp(cache_dir: &Path, path: &Path, age: Duration) -> bool {
    is_cache_tmp_file(cache_dir, path) && is_abandoned_tmp_age(age)
}

fn cache_subpath(cache: &Path, parts: &[&str]) -> PathBuf {
    parts
        .iter()
        .fold(cache.to_path_buf(), |acc, part| acc.join(part))
}

#[test]
fn abandoned_tmp_identifies_stale_tmp_files() {
    let cache = Path::new("/cache");
    assert!(!is_abandoned_tmp(
        cache,
        &cache_subpath(cache, &["pkg", "foo-1.0.0.tgz"]),
        Duration::from_hours(2),
    ));
    assert!(!is_abandoned_tmp(
        cache,
        &cache_subpath(cache, &["pkg", ".tmp.123"]),
        Duration::from_mins(1),
    ));
    assert!(is_abandoned_tmp(
        cache,
        &cache_subpath(cache, &["pkg", ".tmp.123"]),
        Duration::from_secs(4000),
    ));
    assert!(!is_abandoned_tmp(
        cache,
        &cache_subpath(cache, &["resolver-store", "v11", "tmp", "_tmp_1_0"]),
        Duration::from_mins(5),
    ));
    assert!(is_abandoned_tmp(
        cache,
        &cache_subpath(cache, &["resolver-store", "v11", "tmp", "_tmp_1_0"]),
        Duration::from_secs(4000),
    ));
    assert!(is_abandoned_tmp(
        cache,
        &cache_subpath(cache, &["resolver-store", "v11", "files", "stream_1_0"]),
        Duration::from_secs(4000),
    ));
}

#[test]
fn cache_tmp_file_matches_proxy_and_resolver_store_temps() {
    let cache = Path::new("/cache");
    assert!(is_cache_tmp_file(cache, &cache_subpath(cache, &["~public", "h", "pkg", ".tmp.99"])));
    assert!(is_cache_tmp_file(
        cache,
        &pnpr_storage::unique_tmp_path(&cache_subpath(
            cache,
            &["~public", "h", "pkg", "document.json"],
        )),
    ));
    assert!(is_cache_tmp_file(
        cache,
        &cache_subpath(
            cache,
            &["resolver-cache", "v12", "metadata", "registry.npmjs.org", "pkg.jsonl.4321.7.tmp"],
        ),
    ));
    assert!(is_cache_tmp_file(
        cache,
        &cache_subpath(cache, &["resolver-store", "v11", "tmp", "_tmp_7_2"]),
    ));
    assert!(is_cache_tmp_file(
        cache,
        &cache_subpath(cache, &["resolver-store", "v11", "files", "stream_7_2"]),
    ));
    assert!(!is_cache_tmp_file(
        cache,
        &cache_subpath(cache, &["resolver-store", "v10", "tmp", "_tmp_7_2"]),
    ));
    assert!(!is_cache_tmp_file(
        cache,
        &cache_subpath(cache, &["resolver-store", "v11", "files", "ab", "cd1234"]),
    ));
    assert!(!is_cache_tmp_file(
        cache,
        &cache_subpath(
            cache,
            &["resolver-cache", "v12", "metadata", "registry.npmjs.org", "pkg.jsonl"],
        ),
    ));
}

#[test]
fn managed_sqlite_family_recognizes_verdicts_and_active_store_index_only() {
    let cache = Path::new("/cache");
    assert!(is_managed_sqlite_file(
        cache,
        &cache_subpath(cache, &["resolver-cache", "lockfile-verdicts.sqlite"]),
    ));
    assert!(is_managed_sqlite_file(
        cache,
        &cache_subpath(cache, &["resolver-cache", "lockfile-verdicts.sqlite-wal"]),
    ));
    assert!(is_managed_sqlite_file(
        cache,
        &cache_subpath(cache, &["resolver-store", "v11", "index.db-shm"]),
    ));
    assert!(!is_managed_sqlite_file(
        cache,
        &cache_subpath(cache, &["resolver-store", "v10", "index.db"]),
    ));
    assert!(!is_managed_sqlite_file(
        cache,
        &cache_subpath(cache, &["resolver-cache", "v11", "metadata-full", "foo.json"]),
    ));
}

#[test]
fn cas_digest_extracts_hex_for_regular_and_exec_store_files() {
    let cache = Path::new("/cache");
    assert_eq!(
        cas_digest_of_store_file(
            cache,
            &cache_subpath(cache, &["resolver-store", "v11", "files", "0a", "bcdef01234"]),
        ),
        Some("0abcdef01234".to_string()),
    );
    assert_eq!(
        cas_digest_of_store_file(
            cache,
            &cache_subpath(cache, &["resolver-store", "v11", "files", "ff", "112233-exec"]),
        ),
        Some("ff112233".to_string()),
    );
    assert_eq!(
        cas_digest_of_store_file(
            cache,
            &cache_subpath(cache, &["resolver-store", "v10", "files", "0a", "bcdef01234"]),
        ),
        None,
    );
    assert_eq!(
        cas_digest_of_store_file(
            cache,
            &cache_subpath(cache, &["resolver-store", "v11", "files", "stream_1_0"]),
        ),
        None,
    );
}

#[test]
fn count_dir_contents_counts_regular_files_only() {
    let tmp = tempfile::tempdir().unwrap();
    let sub = tmp.path().join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"hello").unwrap();
    std::fs::write(sub.join("b.txt"), b"world!").unwrap();
    #[cfg(unix)]
    {
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("secret.bin");
        std::fs::write(&target, [0u8; 4096]).unwrap();
        std::os::unix::fs::symlink(&target, sub.join("link.bin")).unwrap();
        let (count, bytes) = count_dir_contents(OpenDir::open(tmp.path()).unwrap()).unwrap();
        assert_eq!(count, 2);
        assert_eq!(bytes, 11);
    }
    #[cfg(not(unix))]
    {
        let (count, bytes) = count_dir_contents(OpenDir::open(tmp.path()).unwrap()).unwrap();
        assert_eq!(count, 2);
        assert_eq!(bytes, 11);
    }
}
