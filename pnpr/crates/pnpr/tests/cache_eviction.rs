//! Integration tests for whole-cache reclamation, orphan pruning,
//! and LRU/TTL eviction across upstream and resolver caches.

mod security_and_scopes;

use pnpm_store_dir::{CafsFileInfo, PackageFilesIndex, StoreDir, StoreIndex};
use pnpr::{
    Config,
    cache_maintenance::{
        CacheGcEffect, CacheGcOptions, CacheGcScope, active_cache_namespaces, collect_cache,
        prune_orphaned_namespaces_startup,
    },
};
use pnpr_storage::layout::package_segments;
use rusqlite::Connection;
use std::{
    collections::HashMap,
    fs::{File, FileTimes},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tempfile::TempDir;

fn test_config(tmp: &TempDir) -> Config {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 7677));
    let config_path = tmp.path().join("config.yaml");
    let yaml = format!(
        r"
storage: {storage}
cache: {cache}
secret: test-secret-key-32-chars-long-12
registries:
  public-upstream:
    type: upstream
    url: https://registry.npmjs.org/
    public: true
  private-upstream:
    type: upstream
    url: https://npm.pkg.github.com/
    access:
      - $all
",
        storage = tmp.path().join("storage").display(),
        cache = tmp.path().join("cache").display(),
    );
    std::fs::write(&config_path, yaml).unwrap();
    Config::from_yaml(&config_path, listen, None).unwrap()
}

fn create_file_with_age(path: &Path, content: &[u8], age: Duration) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
    let mtime = SystemTime::now().checked_sub(age).unwrap_or(SystemTime::UNIX_EPOCH);
    let file = File::options()
        .write(true)
        .open(path)
        .unwrap();
    file.set_times(FileTimes::new().set_modified(mtime).set_accessed(mtime))
        .unwrap();
}

#[tokio::test]
async fn orphaned_namespaces_are_pruned_while_active_remain() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let cache_dir = &config.storage.cache_dir;
    let active = active_cache_namespaces(&config);
    assert_eq!(active.len(), 3, "the two upstreams and the tar variants");

    let active_ns = active.iter().next().unwrap();
    let active_file = cache_dir.join(active_ns).join("packages/pkg1/document.json");
    create_file_with_age(&active_file, b"{}", Duration::from_secs(10));

    let orphan_public = cache_dir.join("~public/stale-hash/packages/pkg2/document.json");
    create_file_with_age(&orphan_public, b"{}", Duration::from_secs(10));

    let orphan_private =
        cache_dir.join("~upstreams/stale-secret-hash/packages/pkg3/blobs/pkg3.tgz");
    create_file_with_age(&orphan_private, b"fake-tgz-data", Duration::from_secs(10));

    let options = CacheGcOptions {
        effect: CacheGcEffect::Apply,
        max_size: None,
        max_age: None,
        scope: CacheGcScope::OrphansOnly,
    };
    let report = collect_cache(&config, &options).await.unwrap();
    assert_eq!(report.namespaces_removed, 2);
    assert_eq!(report.files_evicted, 2);

    assert!(active_file.exists());
    assert!(!orphan_public.exists());
    assert!(!orphan_private.exists());
    assert!(!cache_dir.join("~public/stale-hash").exists());
    assert!(!cache_dir.join("~upstreams/stale-secret-hash").exists());
}

#[tokio::test]
async fn dry_run_reports_without_deleting_orphaned_namespaces() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let cache_dir = &config.storage.cache_dir;

    let orphan = cache_dir.join("~upstreams/orphan-hash/packages/pkg/document.json");
    create_file_with_age(&orphan, b"{}", Duration::from_secs(10));

    let options = CacheGcOptions {
        effect: CacheGcEffect::DryRun,
        max_size: None,
        max_age: None,
        scope: CacheGcScope::OrphansOnly,
    };
    let report = collect_cache(&config, &options).await.unwrap();
    assert_eq!(report.namespaces_removed, 1);
    assert_eq!(report.files_evicted, 1);
    assert!(orphan.exists());
}

#[tokio::test]
async fn ttl_eviction_removes_expired_files_and_cleans_empty_dirs() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let cache_dir = &config.storage.cache_dir;
    let active = active_cache_namespaces(&config);
    let active_ns = active.iter().next().unwrap();

    let old_file = cache_dir.join(active_ns).join("packages/stale-pkg/document.json");
    create_file_with_age(&old_file, b"{}", Duration::from_hours(240));

    let fresh_file = cache_dir.join(active_ns).join("packages/fresh-pkg/document.json");
    create_file_with_age(&fresh_file, b"{}", Duration::from_mins(1));

    let options = CacheGcOptions {
        effect: CacheGcEffect::Apply,
        max_size: None,
        max_age: Some(Duration::from_hours(120)),
        scope: CacheGcScope::Full,
    };
    let report = collect_cache(&config, &options).await.unwrap();
    assert_eq!(report.files_evicted, 1);
    assert!(!old_file.exists());
    assert!(
        !cache_dir
            .join(active_ns)
            .join("packages/stale-pkg")
            .exists(),
    );
    assert!(fresh_file.exists());
}

#[tokio::test]
async fn lru_eviction_enforces_max_size_cap() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let cache_dir = &config.storage.cache_dir;
    let active = active_cache_namespaces(&config);
    let active_ns = active.iter().next().unwrap();

    let file1 = cache_dir.join(active_ns).join("packages/pkg/blobs/a-1.0.0.tgz");
    let file2 = cache_dir.join(active_ns).join("packages/pkg/blobs/b-1.0.0.tgz");
    let file3 = cache_dir.join(active_ns).join("packages/pkg/blobs/c-1.0.0.tgz");

    let payload = vec![0xAB; 1000];
    create_file_with_age(&file1, &payload, Duration::from_mins(5));
    create_file_with_age(&file2, &payload, Duration::from_secs(200));
    create_file_with_age(&file3, &payload, Duration::from_secs(100));

    let options = CacheGcOptions {
        effect: CacheGcEffect::Apply,
        max_size: Some(1500),
        max_age: None,
        scope: CacheGcScope::Full,
    };
    let report = collect_cache(&config, &options).await.unwrap();
    assert_eq!(report.files_evicted, 2);
    assert_eq!(report.bytes_reclaimed, 2000);

    assert!(!file1.exists());
    assert!(!file2.exists());
    assert!(file3.exists());
}

#[tokio::test]
async fn startup_pruning_cleans_orphaned_namespaces_automatically() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let cache_dir = &config.storage.cache_dir;

    let orphan = cache_dir.join("~upstreams/stale-proc-hash/packages/pkg/document.json");
    create_file_with_age(&orphan, b"{}", Duration::from_secs(100));

    let report = prune_orphaned_namespaces_startup(&config).await.unwrap();
    assert_eq!(report.namespaces_removed, 1);
    assert!(!orphan.exists());
}

#[tokio::test]
async fn ttl_eviction_prunes_empty_package_directories() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let cache_dir = &config.storage.cache_dir;
    let active = active_cache_namespaces(&config);
    let active_ns = active.iter().next().unwrap();

    let scoped_file = cache_dir
        .join(active_ns)
        .join("packages")
        .join(PathBuf::from_iter(package_segments(
            pnpr_storage::Ecosystem::Npm,
            "@my-scope/my-pkg",
        )))
        .join("document.json");
    create_file_with_age(&scoped_file, b"{}", Duration::from_hours(240));

    let options = CacheGcOptions {
        effect: CacheGcEffect::Apply,
        max_size: None,
        max_age: Some(Duration::from_hours(120)),
        scope: CacheGcScope::Full,
    };
    let report = collect_cache(&config, &options).await.unwrap();
    assert_eq!(report.files_evicted, 1);
    assert!(!scoped_file.exists());
    assert!(
        !cache_dir
            .join(active_ns)
            .join("packages")
            .join(PathBuf::from_iter(package_segments(
                pnpr_storage::Ecosystem::Npm,
                "@my-scope/my-pkg",
            )))
            .exists(),
    );
    assert!(
        !cache_dir
            .join(active_ns)
            .join("packages")
            .exists(),
    );
}

#[tokio::test]
async fn unconfigured_secret_also_prunes_orphaned_private_namespaces() {
    let tmp = TempDir::new().unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 7677));
    let config_path = tmp.path().join("config.yaml");
    let yaml = format!(
        r"
storage: {storage}
cache: {cache}
registries:
  public-upstream:
    type: upstream
    url: https://registry.npmjs.org/
    public: true
  private-upstream:
    type: upstream
    url: https://npm.pkg.github.com/
    access:
      - $all
",
        storage = tmp.path().join("storage").display(),
        cache = tmp.path().join("cache").display(),
    );
    std::fs::write(&config_path, yaml).unwrap();
    let config = Config::from_yaml(&config_path, listen, None).unwrap();
    assert!(!config.has_configured_secret);

    let cache_dir = &config.storage.cache_dir;
    let orphan_public = cache_dir.join("~public/stale-public/packages/pkg/document.json");
    create_file_with_age(&orphan_public, b"{}", Duration::from_secs(10));
    let orphan_private =
        cache_dir.join("~upstreams/prior-ephemeral-digest/packages/pkg/document.json");
    create_file_with_age(&orphan_private, b"{}", Duration::from_secs(10));

    let options = CacheGcOptions {
        effect: CacheGcEffect::Apply,
        max_size: None,
        max_age: None,
        scope: CacheGcScope::OrphansOnly,
    };
    let report = collect_cache(&config, &options).await.unwrap();
    assert_eq!(report.namespaces_removed, 2);
    assert!(!orphan_public.exists());
    assert!(!orphan_private.exists());
}

#[tokio::test]
async fn in_flight_tmp_files_are_preserved_during_eviction() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let cache_dir = &config.storage.cache_dir;
    let active_ns = active_cache_namespaces(&config)
        .into_iter()
        .find(|ns| ns.starts_with("~public"))
        .unwrap();
    let in_flight_tmp = cache_dir.join(format!("{active_ns}/pkg/.tmp.download123"));
    create_file_with_age(&in_flight_tmp, b"partial download", Duration::from_secs(10));
    let expired_file = cache_dir.join(format!("{active_ns}/pkg/old.tgz"));
    create_file_with_age(&expired_file, b"old content", Duration::from_hours(2));

    let options = CacheGcOptions {
        effect: CacheGcEffect::Apply,
        max_size: Some(10),
        max_age: Some(Duration::from_hours(1)),
        scope: CacheGcScope::Full,
    };
    let report = collect_cache(&config, &options).await.unwrap();
    assert_eq!(report.files_evicted, 1);
    assert!(!expired_file.exists());
    assert!(in_flight_tmp.exists());
}

#[tokio::test]
async fn ttl_eviction_covers_the_tar_variants() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let cache_dir = &config.storage.cache_dir;
    let variant = cache_dir.join("~tar-zstd19/left-pad/0123.tar.zst");
    create_file_with_age(&variant, b"variant", Duration::from_hours(240));

    let options = CacheGcOptions {
        effect: CacheGcEffect::Apply,
        max_size: None,
        max_age: Some(Duration::from_hours(24)),
        scope: CacheGcScope::Full,
    };
    let report = collect_cache(&config, &options).await.unwrap();
    assert_eq!(report.files_evicted, 1);
    assert!(!variant.exists(), "an expired variant is evicted");
}

#[tokio::test]
async fn cache_gc_reclaims_stale_s3_staging_files_without_cache_caps() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let staging_base = pnpr_storage::cache_staging_tmp_base(&config.storage.cache_dir);

    let stale_staged = pnpr_storage::unique_tmp_path(&staging_base);
    let payload = vec![0xCD; 2048];
    create_file_with_age(&stale_staged, &payload, Duration::from_mins(2));

    let fresh_staged = pnpr_storage::unique_tmp_path(&staging_base);
    create_file_with_age(&fresh_staged, &payload, Duration::from_secs(5));

    let options = CacheGcOptions {
        effect: CacheGcEffect::Apply,
        max_size: None,
        max_age: None,
        scope: CacheGcScope::Full,
    };
    let report = collect_cache(&config, &options).await.unwrap();
    assert_eq!(report.files_evicted, 1);
    assert_eq!(report.bytes_reclaimed, payload.len() as u64);
    assert!(!stale_staged.exists());
    assert!(fresh_staged.exists());
}

#[tokio::test]
async fn cache_gc_dry_run_counts_stale_staging_files_without_removing_them() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let staging_base = pnpr_storage::cache_staging_tmp_base(&config.storage.cache_dir);

    let stale_staged = pnpr_storage::unique_tmp_path(&staging_base);
    let payload = vec![0xEF; 1024];
    create_file_with_age(&stale_staged, &payload, Duration::from_mins(2));

    let options = CacheGcOptions {
        effect: CacheGcEffect::DryRun,
        max_size: None,
        max_age: None,
        scope: CacheGcScope::Full,
    };
    let report = collect_cache(&config, &options).await.unwrap();
    assert_eq!(report.files_evicted, 1);
    assert_eq!(report.bytes_reclaimed, payload.len() as u64);
    assert!(stale_staged.exists());
}

#[tokio::test]
async fn aggressive_cache_eviction_preserves_in_flight_staging_files() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let staging_base = pnpr_storage::cache_staging_tmp_base(&config.storage.cache_dir);

    let in_flight_staged = pnpr_storage::unique_tmp_path(&staging_base);
    create_file_with_age(&in_flight_staged, b"in-flight-publish", Duration::from_secs(5));

    let options = CacheGcOptions {
        effect: CacheGcEffect::Apply,
        max_size: Some(0),
        max_age: Some(Duration::ZERO),
        scope: CacheGcScope::Full,
    };
    let report = collect_cache(&config, &options).await.unwrap();
    assert_eq!(report.files_evicted, 0);
    assert!(in_flight_staged.exists());
}

#[tokio::test]
async fn whole_cache_eviction_covers_resolver_store_resolver_cache_and_verdict_db() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let cache_dir = &config.storage.cache_dir;
    std::fs::create_dir_all(cache_dir).unwrap();

    let layout_marker = cache_dir.join(pnpr_storage::layout::LAYOUT_FILE);
    std::fs::write(&layout_marker, b"{\"layout\":1}").unwrap();
    let secret_marker = cache_dir.join(pnpr_config::CACHE_SECRET_MARKER);
    std::fs::write(&secret_marker, pnpr_config::cache_secret_fingerprint(&config.cache_secret))
        .unwrap();

    let store_dir = StoreDir::new(cache_dir.join("resolver-store"));
    let shard_dir = store_dir.root().join("files/ab");
    let cas_file = shard_dir.join("cd0123456789");
    create_file_with_age(&cas_file, &[0x11; 800], Duration::from_mins(10));

    let store_index = StoreIndex::open_in(&store_dir).unwrap();
    let mut files = HashMap::new();
    files.insert(
        "package.json".to_string(),
        CafsFileInfo {
            digest: "abcd0123456789".to_string(),
            mode: 0o644,
            size: 800,
            checked_at: None,
        },
    );
    store_index
        .set(
            "sha512-demo\tpkg@1.0.0",
            &PackageFilesIndex {
                manifest: None,
                requires_build: None,
                requires_prepare: None,
                algo: "sha512".to_string(),
                files,
                side_effects: None,
                remote_side_effects_quarantine: None,
                tar_integrity: None,
            },
        )
        .unwrap();
    drop(store_index);

    let npm_meta = cache_dir.join("resolver-cache/v11/metadata-full/registry.npmjs.org/pkg.json");
    create_file_with_age(&npm_meta, &[0x22; 800], Duration::from_mins(8));
    let cargo_idx =
        cache_dir.join("resolver-cache/cargo-index/https_3a_2f_2fcrates.io/se/rd/serde");
    create_file_with_age(&cargo_idx, &[0x33; 800], Duration::from_mins(6));
    let py_idx = cache_dir.join("resolver-cache/python-index/https_3a_2f_2fpypi.org/requests.json");
    create_file_with_age(&py_idx, &[0x44; 800], Duration::from_secs(30));

    let verdict_db = cache_dir.join("resolver-cache/lockfile-verdicts.sqlite");
    let conn = Connection::open(&verdict_db).unwrap();
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         CREATE TABLE lockfile_verdicts (
             hash TEXT PRIMARY KEY,
             policy TEXT NOT NULL,
             verified_at_ms INTEGER NOT NULL
         );",
    )
    .unwrap();
    let now_ms = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let expired_ms = now_ms - i64::try_from(Duration::from_hours(48).as_millis()).unwrap();
    conn.execute(
        "INSERT INTO lockfile_verdicts (hash, policy, verified_at_ms) VALUES (?1, ?2, ?3)",
        rusqlite::params!["expired-hash", "{}", expired_ms],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO lockfile_verdicts (hash, policy, verified_at_ms) VALUES (?1, ?2, ?3)",
        rusqlite::params!["fresh-hash", "{}", now_ms],
    )
    .unwrap();
    drop(conn);

    let sqlite_and_markers = std::fs::metadata(&layout_marker).unwrap().len()
        + std::fs::metadata(&secret_marker).unwrap().len()
        + std::fs::metadata(&verdict_db).unwrap().len()
        + std::fs::metadata(store_dir.root().join("index.db")).unwrap().len();
    let options = CacheGcOptions {
        effect: CacheGcEffect::Apply,
        max_size: Some(sqlite_and_markers + 1000),
        max_age: Some(Duration::from_hours(24)),
        scope: CacheGcScope::Full,
    };
    let report = collect_cache(&config, &options).await.unwrap();
    assert_eq!(report.files_evicted, 3);
    assert!(!cas_file.exists(), "oldest resolver-store CAS file must be evicted");
    assert!(!npm_meta.exists(), "older resolver-cache metadata must be evicted");
    assert!(!cargo_idx.exists(), "older cargo-index file must be evicted");
    assert!(py_idx.exists(), "newest resolver-cache file fits within budget");
    assert!(shard_dir.exists(), "resolver-store shard directory stays intact");
    assert!(layout_marker.exists(), "layout.json must never be evicted");
    assert!(secret_marker.exists(), ".cache-secret must never be evicted");

    let reopened_index = StoreIndex::open_in(&store_dir).unwrap();
    assert!(
        reopened_index
            .get("sha512-demo\tpkg@1.0.0")
            .unwrap()
            .is_none(),
        "StoreIndex row referencing an evicted CAS digest must be pruned",
    );

    let conn = Connection::open(&verdict_db).unwrap();
    let expired_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM lockfile_verdicts WHERE hash = 'expired-hash'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let fresh_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM lockfile_verdicts WHERE hash = 'fresh-hash'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(expired_count, 0, "expired lockfile verdict row must be deleted");
    assert_eq!(fresh_count, 1, "fresh lockfile verdict row within budget stays");
}

#[tokio::test]
async fn cache_max_size_accounts_for_non_evictable_shared_artifacts_overhead() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let cache_dir = &config.storage.cache_dir;

    let shared_blob = cache_dir.join("shared-artifacts/blobs/sha256-deadbeef");
    create_file_with_age(&shared_blob, &[0xAA; 1200], Duration::from_hours(5));

    let older_cache = cache_dir.join("resolver-cache/v11/metadata-full/a.json");
    create_file_with_age(&older_cache, &[0xBB; 600], Duration::from_mins(10));
    let newer_cache = cache_dir.join("resolver-cache/v11/metadata-full/b.json");
    create_file_with_age(&newer_cache, &[0xCC; 600], Duration::from_mins(1));

    let options = CacheGcOptions {
        effect: CacheGcEffect::Apply,
        max_size: Some(1900),
        max_age: None,
        scope: CacheGcScope::Full,
    };
    let report = collect_cache(&config, &options).await.unwrap();
    assert_eq!(report.files_evicted, 1);
    assert!(shared_blob.exists(), "shared-artifacts files are managed by artifacts.quota");
    assert!(!older_cache.exists(), "shared-artifacts bytes count toward cache.maxSize");
    assert!(newer_cache.exists());
}

#[tokio::test]
async fn uploads_scratch_dir_is_preserved_and_counted_toward_max_size() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let cache_dir = &config.storage.cache_dir;

    let upload_scratch = cache_dir.join(".uploads/chunk-1");
    create_file_with_age(&upload_scratch, &[0xAA; 1200], Duration::from_hours(3));
    let empty_uploads_sub = cache_dir.join(".uploads/empty-session");
    std::fs::create_dir_all(&empty_uploads_sub).unwrap();

    let older_cache = cache_dir.join("resolver-cache/v11/metadata-full/old.json");
    create_file_with_age(&older_cache, &[0xBB; 600], Duration::from_mins(10));
    let newer_cache = cache_dir.join("resolver-cache/v11/metadata-full/new.json");
    create_file_with_age(&newer_cache, &[0xCC; 600], Duration::from_mins(1));

    let options = CacheGcOptions {
        effect: CacheGcEffect::Apply,
        max_size: Some(1900),
        max_age: Some(Duration::from_hours(2)),
        scope: CacheGcScope::Full,
    };
    let report = collect_cache(&config, &options).await.unwrap();
    assert_eq!(report.files_evicted, 1);
    assert!(upload_scratch.exists(), ".uploads scratch must not be evicted before UPLOAD_MAX_AGE");
    assert!(empty_uploads_sub.exists(), ".uploads dirs must not be pruned by cache eviction");
    assert!(!older_cache.exists());
    assert!(newer_cache.exists());
}
