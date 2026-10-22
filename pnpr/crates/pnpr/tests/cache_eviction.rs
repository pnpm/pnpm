//! Integration tests for upstream proxy cache reclamation, orphan pruning,
//! and LRU eviction.

use pnpr::{
    Config,
    cache_maintenance::{
        CacheGcEffect, CacheGcOptions, CacheGcScope, active_cache_namespaces, collect_cache,
        prune_orphaned_namespaces_startup,
    },
};
use pnpr_storage::layout::encode_segment;
use std::{
    fs::{File, FileTimes},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::Path,
    time::{Duration, SystemTime},
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
        storage = tmp.path().display(),
        cache = tmp.path().display(),
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
    let active = active_cache_namespaces(&config);
    assert_eq!(active.len(), 3, "the two upstreams and the tar variants");

    let active_ns = active.iter().next().unwrap();
    let active_file = tmp
        .path()
        .join(active_ns)
        .join("packages/pkg1/document.json");
    create_file_with_age(&active_file, b"{}", Duration::from_secs(10));

    let orphan_public = tmp.path().join("~public/stale-hash/packages/pkg2/document.json");
    create_file_with_age(&orphan_public, b"{}", Duration::from_secs(10));

    let orphan_private =
        tmp.path().join("~upstreams/stale-secret-hash/packages/pkg3/blobs/pkg3.tgz");
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
    assert!(
        !tmp.path()
            .join("~public/stale-hash")
            .exists(),
    );
    assert!(
        !tmp.path()
            .join("~upstreams/stale-secret-hash")
            .exists(),
    );
}

#[tokio::test]
async fn dry_run_reports_without_deleting_orphaned_namespaces() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);

    let orphan = tmp.path().join("~upstreams/orphan-hash/packages/pkg/document.json");
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
    let active = active_cache_namespaces(&config);
    let active_ns = active.iter().next().unwrap();

    let old_file = tmp
        .path()
        .join(active_ns)
        .join("packages/stale-pkg/document.json");
    create_file_with_age(&old_file, b"{}", Duration::from_hours(240));

    let fresh_file = tmp
        .path()
        .join(active_ns)
        .join("packages/fresh-pkg/document.json");
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
        !tmp.path()
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
    let active = active_cache_namespaces(&config);
    let active_ns = active.iter().next().unwrap();

    let file1 = tmp
        .path()
        .join(active_ns)
        .join("packages/pkg/blobs/a-1.0.0.tgz");
    let file2 = tmp
        .path()
        .join(active_ns)
        .join("packages/pkg/blobs/b-1.0.0.tgz");
    let file3 = tmp
        .path()
        .join(active_ns)
        .join("packages/pkg/blobs/c-1.0.0.tgz");

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

    let orphan = tmp.path().join("~upstreams/stale-proc-hash/packages/pkg/document.json");
    create_file_with_age(&orphan, b"{}", Duration::from_secs(100));

    let report = prune_orphaned_namespaces_startup(&config).await.unwrap();
    assert_eq!(report.namespaces_removed, 1);
    assert!(!orphan.exists());
}

#[tokio::test]
async fn ttl_eviction_prunes_empty_package_directories() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let active = active_cache_namespaces(&config);
    let active_ns = active.iter().next().unwrap();

    let scoped_file = tmp
        .path()
        .join(active_ns)
        .join("packages")
        .join(encode_segment("@my-scope/my-pkg"))
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
        !tmp.path()
            .join(active_ns)
            .join("packages")
            .join(encode_segment("@my-scope/my-pkg"))
            .exists(),
    );
    assert!(
        !tmp.path()
            .join(active_ns)
            .join("packages")
            .exists(),
    );
}

#[tokio::test]
async fn unconfigured_secret_preserves_private_namespaces_from_orphan_pruning() {
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
        storage = tmp.path().display(),
        cache = tmp.path().display(),
    );
    std::fs::write(&config_path, yaml).unwrap();
    let config = Config::from_yaml(&config_path, listen, None).unwrap();
    assert!(!config.has_configured_secret);

    let orphan_public = tmp.path().join("~public/stale-public/packages/pkg/document.json");
    create_file_with_age(&orphan_public, b"{}", Duration::from_secs(10));
    let private_ns =
        tmp.path().join("~upstreams/sibling-process-digest/packages/pkg/document.json");
    create_file_with_age(&private_ns, b"{}", Duration::from_secs(10));

    let options = CacheGcOptions {
        effect: CacheGcEffect::Apply,
        max_size: None,
        max_age: None,
        scope: CacheGcScope::OrphansOnly,
    };
    let report = collect_cache(&config, &options).await.unwrap();
    assert_eq!(report.namespaces_removed, 1);
    assert!(!orphan_public.exists());
    assert!(private_ns.exists());
}

#[tokio::test]
async fn in_flight_tmp_files_are_preserved_during_eviction() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let active_ns = active_cache_namespaces(&config)
        .into_iter()
        .find(|ns| ns.starts_with("~public"))
        .unwrap();
    let in_flight_tmp = tmp
        .path()
        .join(format!("{active_ns}/pkg/.tmp.download123"));
    create_file_with_age(&in_flight_tmp, b"partial download", Duration::from_secs(10));
    let expired_file = tmp
        .path()
        .join(format!("{active_ns}/pkg/old.tgz"));
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
    let variant = tmp.path().join("~tar-zstd19/left-pad/0123.tar.zst");
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
