use super::{
    CacheGcEffect,
    inventory::{cas_digest_of_store_file, is_cache_tmp_file, is_managed_sqlite_file},
    is_abandoned_tmp_age,
    orphans::{PrivateScopePolicy, count_dir_contents, prune_orphaned_namespaces},
};
use crate::server::ecosystem::UpstreamCacheSubtree;
use pnpm_fs::OpenDir;
use std::{
    collections::HashSet,
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

#[cfg(windows)]
#[test]
fn windows_backslash_paths_classify_temps_sqlite_and_cas_digests() {
    let cache = Path::new(r"C:\pnpr\cache");
    assert!(is_cache_tmp_file(cache, Path::new(r"C:\pnpr\cache\resolver-store\v11\tmp\_tmp_1_0")));
    let wal = Path::new(r"C:\pnpr\cache\resolver-cache\lockfile-verdicts.sqlite-wal");
    assert!(is_managed_sqlite_file(cache, wal));
    let cas = Path::new(r"C:\pnpr\cache\resolver-store\v11\files\0a\bcdef01234-exec");
    assert_eq!(cas_digest_of_store_file(cache, cas), Some("0abcdef01234".to_string()));
}

fn write_cached_file(cache_dir: &Path, namespace: &str, rel: &str, bytes: &[u8]) -> PathBuf {
    let mut path = cache_dir.to_path_buf();
    path.extend(
        namespace
            .split('/')
            .filter(|segment| !segment.is_empty()),
    );
    path.extend(rel.split('/'));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, bytes).unwrap();
    path
}

fn populate_upstream_ecosystem_subtrees(cache_dir: &Path, namespace: &str) -> Vec<PathBuf> {
    let cargo_artifact = UpstreamCacheSubtree::Artifact(b"sha256-cargo").within(namespace);
    let pypi_artifact = UpstreamCacheSubtree::Artifact(b"sha256-pypi").within(namespace);
    let pypi_metadata =
        UpstreamCacheSubtree::Metadata(b"https://pypi.org/whl\ntrue").within(namespace);
    let oci_manifest = UpstreamCacheSubtree::OciManifest(b"latest").within(namespace);
    let oci_blobs = UpstreamCacheSubtree::OciBlobs.within(namespace);
    vec![
        write_cached_file(cache_dir, namespace, "packages/@/mycrate/document.json", b"{}"),
        write_cached_file(
            cache_dir,
            &cargo_artifact,
            "packages/@/mycrate/blobs/1.0.0.crate",
            b"crate-bytes",
        ),
        write_cached_file(
            cache_dir,
            &pypi_artifact,
            "packages/@/requests/blobs/requests-1.0.0-py3-none-any.whl",
            b"wheel-bytes",
        ),
        write_cached_file(
            cache_dir,
            &pypi_metadata,
            "packages/@/requests/document.json",
            b"Metadata-Version: 2.1\n",
        ),
        write_cached_file(
            cache_dir,
            &oci_manifest,
            "packages/@library/alpine/document.json",
            b"{\"schemaVersion\":2}",
        ),
        write_cached_file(
            cache_dir,
            &oci_blobs,
            "packages/@library/alpine/blobs/sha256-deadbeef",
            b"layer-bytes",
        ),
    ]
}

#[tokio::test]
async fn prune_orphaned_namespaces_keeps_active_ecosystem_subtrees_and_prunes_prefixed_siblings() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_dir = tmp.path();
    let active = HashSet::from(["~public/abc".to_string(), "~upstreams/priv".to_string()]);

    let active_public = populate_upstream_ecosystem_subtrees(cache_dir, "~public/abc");
    let active_private = populate_upstream_ecosystem_subtrees(cache_dir, "~upstreams/priv");
    let removed_public = populate_upstream_ecosystem_subtrees(cache_dir, "~public/zzz");
    let removed_private = populate_upstream_ecosystem_subtrees(cache_dir, "~upstreams/old");

    let prefix_without_dash =
        write_cached_file(cache_dir, "~public/abcdef", "packages/@/pkg/document.json", b"123");
    let prefix_with_dash =
        write_cached_file(cache_dir, "~public/abc-stale", "packages/@/pkg/document.json", b"456");
    let private_prefix_without_dash =
        write_cached_file(cache_dir, "~upstreams/privdef", "packages/@/pkg/document.json", b"789");

    let dry = prune_orphaned_namespaces(
        cache_dir,
        &active,
        PrivateScopePolicy::Keep,
        CacheGcEffect::DryRun,
    )
    .await
    .unwrap();
    assert_eq!(dry.report.namespaces_removed, 5);
    assert_eq!(dry.report.files_evicted, removed_public.len() + removed_private.len() + 3);
    assert!(prefix_without_dash.exists());

    let applied = prune_orphaned_namespaces(
        cache_dir,
        &active,
        PrivateScopePolicy::Keep,
        CacheGcEffect::Apply,
    )
    .await
    .unwrap();
    assert_eq!(applied.report, dry.report);

    for path in active_public.iter().chain(&active_private) {
        eprintln!("ACTIVE PATH: {}", path.display());
        assert!(path.exists());
    }
    for path in removed_public
        .iter()
        .chain(&removed_private)
        .chain([&prefix_without_dash, &prefix_with_dash, &private_prefix_without_dash])
    {
        eprintln!("ORPHAN PATH: {}", path.display());
        assert!(!path.exists());
    }
    assert!(!cache_dir.join("~public/zzz").exists());
    assert!(!cache_dir.join("~upstreams/old").exists());
    assert!(!cache_dir.join("~public/abcdef").exists());
    assert!(!cache_dir.join("~public/abc-stale").exists());
    assert!(!cache_dir.join("~upstreams/privdef").exists());
}
