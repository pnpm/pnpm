use super::{Config, Storage, sweep_abandoned_archive_temps};
use object_store::memory::InMemory;
use pnpr_config::HostedStoreConfig;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use std::{
    fs::{File, FileTimes},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime},
};
use tempfile::TempDir;

fn s3_config(tmp: &TempDir) -> Config {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 7677));
    let config_path = tmp.path().join("config.yaml");
    let yaml = format!(
        r"
storage: {storage}
cache: {cache}
secret: test-secret-key-32-chars-long-12
registries:
  local:
    type: hosted
",
        storage = tmp.path().join("storage").display(),
        cache = tmp.path().join("cache").display(),
    );
    std::fs::write(&config_path, yaml).unwrap();
    let mut config = Config::from_yaml(&config_path, listen, None).unwrap();
    config.storage.hosted_backend =
        HostedStoreConfig::ObjectStore { store: Arc::new(InMemory::new()), prefix: String::new() };
    config
}

fn set_age(path: &Path, age: Duration) {
    let modified = SystemTime::now().checked_sub(age).unwrap_or(SystemTime::UNIX_EPOCH);
    let file = File::options()
        .write(true)
        .open(path)
        .unwrap();
    file.set_times(FileTimes::new().set_modified(modified))
        .unwrap();
}

#[tokio::test(start_paused = true)]
async fn startup_and_periodic_sweeps_reclaim_s3_staging_files_past_request_timeout() {
    let tmp = TempDir::new().unwrap();
    let config = s3_config(&tmp);
    let storage = Storage::from_config(&config.storage).unwrap();
    let name = CanonicalPackageName::parse("large-pkg", Ecosystem::Npm).unwrap();
    let staging_base = pnpr_storage::cache_staging_tmp_base(&config.storage.cache_dir);
    tokio::fs::create_dir_all(staging_base.parent().unwrap()).await.unwrap();

    let stale_path = pnpr_storage::unique_tmp_path(&staging_base);
    tokio::fs::write(&stale_path, b"stale-staging-payload").await.unwrap();
    set_age(&stale_path, Duration::from_mins(2));

    let fresh_path = pnpr_storage::unique_tmp_path(&staging_base);
    tokio::fs::write(&fresh_path, b"just-crashed-payload").await.unwrap();

    let active_slot = storage.reserve_hosted_blob(&name, "large-pkg-1.0.2.tgz").await.unwrap();
    let active_path = active_slot.tmp_path.clone();
    tokio::fs::write(&active_path, b"in-flight-publish-payload").await.unwrap();
    set_age(&active_path, Duration::from_mins(2));

    sweep_abandoned_archive_temps(&config).await.unwrap();
    assert!(!stale_path.exists(), "startup sweep must remove staging files past request_timeout");
    assert!(fresh_path.exists(), "startup sweep must keep staging files within request_timeout");
    assert!(active_path.exists(), "startup sweep must keep active in-process slots");

    tokio::task::yield_now().await;
    set_age(&fresh_path, Duration::from_mins(2));
    tokio::time::advance(Duration::from_mins(2)).await;
    tokio::time::resume();
    for _ in 0..100 {
        if !fresh_path.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(
        !fresh_path.exists(),
        "periodic sweep must remove staging files once request_timeout elapses",
    );
    assert!(active_path.exists(), "periodic sweep must keep active in-process slots");
    drop(active_slot);
    assert!(!active_path.exists());
}

#[tokio::test(start_paused = true)]
async fn startup_sweep_reclaims_local_hosted_archive_temps_across_namespaces() {
    let tmp = TempDir::new().unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 7677));
    let config_path = tmp.path().join("config.yaml");
    let yaml = format!(
        r"
storage: {storage}
cache: {cache}
secret: test-secret-key-32-chars-long-12
registries:
  acme-reg:
    type: hosted
    namespace: acme
",
        storage = tmp.path().join("storage").display(),
        cache = tmp.path().join("cache").display(),
    );
    std::fs::write(&config_path, yaml).unwrap();
    let config = Config::from_yaml(&config_path, listen, None).unwrap();
    let storage = Storage::from_config(&config.storage).unwrap().for_hosted("acme", Ecosystem::Npm);
    let name = CanonicalPackageName::parse("@acme/pkg", Ecosystem::Npm).unwrap();

    let probe = storage.reserve_hosted_blob(&name, "pkg-1.0.0.tgz").await.unwrap();
    let stale_path = pnpr_storage::unique_tmp_path(&probe.tmp_path.with_file_name("archive"));
    drop(probe);
    tokio::fs::write(&stale_path, b"stale-local-payload").await.unwrap();
    set_age(&stale_path, Duration::from_mins(2));

    sweep_abandoned_archive_temps(&config).await.unwrap();
    assert!(!stale_path.exists());
}

#[tokio::test(start_paused = true)]
async fn shutdown_deadline_starts_when_shutdown_is_requested() {
    let (send_shutdown, shutdown) = tokio::sync::oneshot::channel::<()>();
    let serving = tokio::spawn(super::drain_until_deadline(std::future::pending(), async {
        shutdown.await.unwrap();
    }));
    tokio::time::advance(Duration::from_secs(120)).await;
    assert!(!serving.is_finished());
    send_shutdown.send(()).unwrap();
    tokio::task::yield_now().await;
    tokio::time::advance(
        super::SHUTDOWN_DRAIN_TIMEOUT
            .checked_sub(Duration::from_secs(1))
            .unwrap(),
    )
    .await;
    assert!(!serving.is_finished());
    tokio::time::advance(Duration::from_secs(1)).await;
    serving.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn shutdown_drain_returns_when_requests_finish() {
    let start = tokio::time::Instant::now();
    super::drain_until_deadline(
        async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            Ok(())
        },
        std::future::ready(()),
    )
    .await
    .unwrap();
    assert_eq!(start.elapsed(), Duration::from_secs(1));
}

#[tokio::test(start_paused = true)]
async fn upstream_only_proxy_startup_preserves_staged_archives_in_shared_store() {
    let tmp = TempDir::new().unwrap();
    let hosted_config = s3_config(&tmp);
    let hosted_storage = Storage::from_config(&hosted_config.storage).unwrap();
    hosted_storage.ensure_hosted_layout().await.unwrap();
    let blob_id = pnpr_storage::BlobId::of_bytes(Ecosystem::Npm, b"staged-tarball");
    let key = pnpr_storage::StagedArchiveKey { principal: "alice", blob_id: &blob_id };
    assert!(hosted_storage.stage_archive(&key, b"staged-tarball".to_vec()).await.unwrap());

    let mut proxy_config = hosted_config.clone();
    proxy_config.routing.hosted.clear();
    proxy_config.storage.metadata =
        pnpr_config::MetadataBackend::Sqlite(tmp.path().join("proxy-replica.sqlite"));
    super::load_startup_auth(&proxy_config).await.unwrap();
    tokio::time::advance(Duration::from_hours(2)).await;
    tokio::task::yield_now().await;

    assert_eq!(
        hosted_storage
            .read_staged_archive(&key)
            .await
            .unwrap()
            .as_deref(),
        Some(b"staged-tarball".as_slice()),
    );
}

#[tokio::test]
async fn ephemeral_run_marks_cache_and_requires_secret_on_restart() {
    let tmp = TempDir::new().unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0));
    let config_path = tmp.path().join("pnpr.yaml");
    let storage_dir = tmp.path().join("storage");
    let cache_dir = tmp.path().join("cache");
    std::fs::write(
        &config_path,
        format!(
            "storage: {}\ncache: {}\nregistries:\n  private-up:\n    type: upstream\n    url: https://npm.pkg.github.com/\n    access:\n      - $all\n",
            storage_dir.display(),
            cache_dir.display(),
        ),
    )
    .unwrap();

    let first_config = Config::from_yaml(&config_path, listen, None).unwrap();
    assert!(!first_config.has_configured_secret);
    let listener = tokio::net::TcpListener::bind(listen).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut serve_task = tokio::spawn(super::serve_listener(first_config, listener));
    wait_for_http_ping(addr, &mut serve_task).await;
    assert!(cache_dir.join(pnpr_config::CACHE_SECRET_MARKER).exists());
    serve_task.abort();
    let _ = serve_task.await;

    let stale_private =
        cache_dir.join("~upstreams/ephemeral-run-digest/packages/pkg/document.json");
    tokio::fs::create_dir_all(stale_private.parent().unwrap()).await.unwrap();
    tokio::fs::write(&stale_private, b"{}").await.unwrap();

    let restart_without_secret = Config::from_yaml(&config_path, listen, None).unwrap();
    let listener2 = tokio::net::TcpListener::bind(listen).await.unwrap();
    let err =
        super::serve_listener(restart_without_secret, listener2).await.unwrap_err().to_string();
    assert!(err.contains("when the cache directory persists across restarts"), "{err}");

    std::fs::write(
        &config_path,
        format!(
            "storage: {}\ncache: {}\nsecret: configured-secret-for-restart-32\nregistries:\n  private-up:\n    type: upstream\n    url: https://npm.pkg.github.com/\n    access:\n      - $all\n",
            storage_dir.display(),
            cache_dir.display(),
        ),
    )
    .unwrap();
    let configured_restart = Config::from_yaml(&config_path, listen, None).unwrap();
    let listener3 = tokio::net::TcpListener::bind(listen).await.unwrap();
    let addr3 = listener3.local_addr().unwrap();
    let mut serve_task3 = tokio::spawn(super::serve_listener(configured_restart, listener3));
    wait_for_http_ping(addr3, &mut serve_task3).await;
    serve_task3.abort();
    let _ = serve_task3.await;
    assert!(!stale_private.exists(), "startup with configured secret prunes prior ephemeral dirs");
}

#[tokio::test]
async fn default_osv_dir_in_unmarked_hosted_store_passes_layout_check() {
    let tmp = TempDir::new().unwrap();
    let storage_dir = tmp.path().join("storage");
    let osv_path = pnpr_config::default_osv_path(&storage_dir);
    tokio::fs::create_dir_all(osv_path.parent().unwrap()).await.unwrap();
    tokio::fs::write(&osv_path, b"placeholder").await.unwrap();

    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 7677));
    let mut config = Config::from_default_yaml(tmp.path(), listen, None);
    config.storage.hosted_dir = storage_dir.clone();
    config.storage.cache_dir = tmp.path().join("cache");
    config.storage.metadata =
        pnpr_config::MetadataBackend::Sqlite(pnpr_config::default_metadata_path(&storage_dir));

    super::prepare_storage(&config).await.unwrap();
    assert!(storage_dir.join(pnpr_storage::layout::LAYOUT_FILE).exists());
    assert!(osv_path.exists());
    let _router = crate::try_router(config).unwrap();
}

#[tokio::test]
async fn non_dot_cache_subdir_inside_fresh_hosted_storage_starts_cleanly() {
    let tmp = TempDir::new().unwrap();
    let storage_dir = tmp.path().join("cold-mock-storage");
    let cache_dir = storage_dir.join("cache");

    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0));
    let mut config = Config::from_default_yaml(tmp.path(), listen, None);
    config.storage.hosted_dir = storage_dir.clone();
    config.storage.cache_dir = cache_dir.clone();
    config.storage.metadata =
        pnpr_config::MetadataBackend::Sqlite(pnpr_config::default_metadata_path(&storage_dir));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut serve_task = tokio::spawn(super::serve_listener(config, listener));
    wait_for_http_ping(addr, &mut serve_task).await;
    serve_task.abort();
    let _ = serve_task.await;
    assert!(storage_dir.join(pnpr_storage::layout::LAYOUT_FILE).exists());
    assert!(cache_dir.join(pnpr_config::CACHE_SECRET_MARKER).exists());
}

async fn wait_for_http_ping(
    addr: SocketAddr,
    serve_task: &mut tokio::task::JoinHandle<pnpr_error::Result<()>>,
) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        if serve_task.is_finished() {
            let res = serve_task.await.expect("serve_listener task panicked");
            panic!("serve_listener exited before answering /-/ping: {res:?}");
        }
        let step = deadline
            .saturating_duration_since(tokio::time::Instant::now())
            .min(Duration::from_millis(50));
        if tokio::time::timeout(step, try_http_ping(addr)).await == Ok(true) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    if serve_task.is_finished() {
        let res = serve_task.await.expect("serve_listener task panicked");
        panic!("serve_listener exited before answering /-/ping: {res:?}");
    }
    panic!("timed out waiting for HTTP 200 on /-/ping at {addr}");
}

async fn try_http_ping(addr: SocketAddr) -> bool {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    let Ok(mut stream) = tokio::net::TcpStream::connect(addr).await else {
        return false;
    };
    if stream
        .write_all(b"GET /-/ping HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .is_err()
    {
        return false;
    }
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.is_ok() && buf.starts_with(b"HTTP/1.1 200")
}

#[tokio::test]
async fn concurrent_ephemeral_cache_claims_admit_only_one_secret() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().join("shared-ephemeral-cache");
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0));

    let mut cfg_a = Config::from_default_yaml(tmp.path(), listen, None);
    cfg_a.storage.cache_dir = cache_dir.clone();
    let mut cfg_b = Config::from_default_yaml(tmp.path(), listen, None);
    cfg_b.storage.cache_dir = cache_dir;

    let claim_a = super::claim_ephemeral_cache_dir(&cfg_a);
    let claim_b = super::claim_ephemeral_cache_dir(&cfg_b);
    let (res_a, res_b) = tokio::join!(claim_a, claim_b);
    assert_eq!(
        usize::from(res_a.is_ok()) + usize::from(res_b.is_ok()),
        1,
        "exactly one distinct ephemeral secret may claim an unmarked cache dir",
    );
}

#[tokio::test]
async fn artifacts_only_startup_prunes_private_scopes_on_secret_rotation() {
    let tmp = TempDir::new().unwrap();
    let storage_dir = tmp.path().join("storage");
    let cache_dir = tmp.path().join("cache");
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(
        cache_dir.join(pnpr_config::CACHE_SECRET_MARKER),
        pnpr_config::cache_secret_fingerprint(b"old-secret-key-32-bytes-00000001"),
    )
    .unwrap();
    let stale_private = cache_dir.join("resolver-cache/v12/metadata-private/old-scope/pkg.jsonl");
    std::fs::create_dir_all(stale_private.parent().unwrap()).unwrap();
    std::fs::write(&stale_private, b"{}").unwrap();

    let config_path = tmp.path().join("artifacts.yaml");
    std::fs::write(
        &config_path,
        format!(
            "storage: {}\ncache: {}\nsecret: new-secret-key-32-bytes-00000002\nresolver:\n  enabled: false\nartifacts:\n  enabled: true\n",
            storage_dir.display(),
            cache_dir.display(),
        ),
    )
    .unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0));
    let config = Config::from_yaml(&config_path, listen, None).unwrap();
    assert!(!config.features.registry.enabled && !config.features.resolver.enabled);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut serve_task = tokio::spawn(super::serve_listener(config.clone(), listener));
    wait_for_http_ping(addr, &mut serve_task).await;
    serve_task.abort();
    let _ = serve_task.await;

    assert!(!stale_private.exists(), "artifacts-only startup must prune rotated private scopes");
    let stored = std::fs::read_to_string(cache_dir.join(pnpr_config::CACHE_SECRET_MARKER)).unwrap();
    assert_eq!(stored.trim(), pnpr_config::cache_secret_fingerprint(&config.cache_secret));
}

#[tokio::test]
async fn prepare_storage_purges_rotated_private_scopes_for_embedded_routers() {
    let tmp = TempDir::new().unwrap();
    let storage_dir = tmp.path().join("storage");
    let cache_dir = tmp.path().join("cache");
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(
        cache_dir.join(pnpr_config::CACHE_SECRET_MARKER),
        pnpr_config::cache_secret_fingerprint(b"old-secret-key-32-bytes-00000001"),
    )
    .unwrap();
    let stale_private = cache_dir.join("resolver-cache/v12/metadata-private/old-scope/pkg.jsonl");
    std::fs::create_dir_all(stale_private.parent().unwrap()).unwrap();
    std::fs::write(&stale_private, b"{}").unwrap();
    let stale_upstream = cache_dir.join("~upstreams/old-upstream-ns/packages/pkg/document.json");
    std::fs::create_dir_all(stale_upstream.parent().unwrap()).unwrap();
    std::fs::write(&stale_upstream, b"{}").unwrap();

    let config_path = tmp.path().join("embedded.yaml");
    std::fs::write(
        &config_path,
        format!(
            "storage: {}\ncache: {}\nsecret: new-secret-key-32-bytes-00000002\n",
            storage_dir.display(),
            cache_dir.display(),
        ),
    )
    .unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0));
    let config = Config::from_yaml(&config_path, listen, None).unwrap();

    super::prepare_storage(&config).await.unwrap();
    assert!(!stale_private.exists(), "prepare_storage must purge rotated private resolver scopes");
    assert!(!stale_upstream.exists(), "prepare_storage must purge rotated private upstream dirs");
    let stored = std::fs::read_to_string(cache_dir.join(pnpr_config::CACHE_SECRET_MARKER)).unwrap();
    assert_eq!(stored.trim(), pnpr_config::cache_secret_fingerprint(&config.cache_secret));
}

#[tokio::test]
async fn concurrent_same_config_ephemeral_claims_both_succeed() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().join("same-secret-ephemeral-cache");
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0));
    let mut cfg = Config::from_default_yaml(tmp.path(), listen, None);
    cfg.storage.cache_dir = cache_dir;

    let claim_a = super::claim_ephemeral_cache_dir(&cfg);
    let claim_b = super::claim_ephemeral_cache_dir(&cfg);
    let (res_a, res_b) = tokio::join!(claim_a, claim_b);
    res_a.unwrap();
    res_b.unwrap();
}

#[tokio::test]
async fn concurrent_same_config_claims_reject_dirty_cache_and_prepare_validates_paths() {
    let tmp = TempDir::new().unwrap();
    let dirty_cache = tmp.path().join("dirty-ephemeral-cache");
    std::fs::create_dir_all(dirty_cache.join("~upstreams/stale")).unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0));
    let mut cfg = Config::from_default_yaml(tmp.path(), listen, None);
    cfg.storage.cache_dir = dirty_cache.clone();

    let claim_a = super::claim_ephemeral_cache_dir(&cfg);
    let claim_b = super::claim_ephemeral_cache_dir(&cfg);
    let (res_a, res_b) = tokio::join!(claim_a, claim_b);
    assert!(res_a.is_err(), "first claimant must reject pre-existing cache entries");
    assert!(res_b.is_err(), "second claimant must not accept a pending marker on dirty cache");
    assert!(!dirty_cache.join(pnpr_config::CACHE_SECRET_MARKER).exists());

    let shared_dir = tmp.path().join("shared-dir");
    let hosted_entry = shared_dir.join("~hosted-entry/file.txt");
    std::fs::create_dir_all(hosted_entry.parent().unwrap()).unwrap();
    std::fs::write(&hosted_entry, b"precious").unwrap();

    let mut invalid_cfg = Config::from_default_yaml(tmp.path(), listen, None);
    invalid_cfg.has_configured_secret = true;
    invalid_cfg.storage.hosted_dir = shared_dir.clone();
    invalid_cfg.storage.cache_dir = shared_dir;
    assert!(super::prepare_storage(&invalid_cfg).await.is_err());
    assert!(
        hosted_entry.exists(),
        "prepare_storage must validate cache paths before resetting cache layout",
    );

    let mut shared_no_secret = Config::from_default_yaml(tmp.path(), listen, None);
    shared_no_secret.storage.cache_dir = tmp.path().join("unclaimed-shared-cache");
    shared_no_secret.storage.hosted_backend =
        pnpr_config::HostedStoreConfig::S3(pnpr_config::S3Settings {
            bucket: "test-bucket".to_string(),
            region: Some("us-east-1".to_string()),
            prefix: None,
            endpoint: Some("http://127.0.0.1:9000".to_string()),
            access_key_id: Some("key".to_string()),
            secret_access_key: Some("secret".to_string()),
            allow_http: Some(true),
            force_path_style: Some(true),
        });
    let err = super::prepare_storage(&shared_no_secret).await.unwrap_err().to_string();
    assert!(err.contains("when using shared storage or database backends"), "{err}");
    assert!(!shared_no_secret.storage.cache_dir.exists());
}

#[tokio::test]
async fn serving_startup_defers_pending_claim_while_rejecting_dirty_or_oversized_markers() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().join("pending-serving-cache");
    std::fs::create_dir_all(&cache_dir).unwrap();
    let marker = cache_dir.join(pnpr_config::CACHE_SECRET_MARKER);
    std::fs::write(&marker, "pending").unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0));
    let mut cfg = Config::from_default_yaml(tmp.path(), listen, None);
    cfg.storage.cache_dir = cache_dir.clone();

    assert!(
        crate::config_validation::validate_startup_config(&mut cfg).is_err(),
        "non-serving validation must still reject a stale pending marker",
    );
    crate::config_validation::validate_serving_startup_config(&mut cfg).unwrap();

    let fingerprint = pnpr_config::cache_secret_fingerprint(&cfg.cache_secret);
    let claim_cfg = cfg.clone();
    let claim_task =
        tokio::spawn(async move { super::claim_ephemeral_cache_dir(&claim_cfg).await });
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    assert!(!claim_task.is_finished(), "claimant must wait while marker is still pending");
    pnpm_fs::OpenDir::open(&cache_dir)
        .unwrap()
        .write_child_file(
            std::ffi::OsStr::new(pnpr_config::CACHE_SECRET_MARKER),
            fingerprint.as_bytes(),
        )
        .unwrap();
    claim_task.await.unwrap().unwrap();

    let orphan_tmp_cache = tmp.path().join("unmarked-orphan-tmp-cache");
    std::fs::create_dir_all(&orphan_tmp_cache).unwrap();
    std::fs::write(orphan_tmp_cache.join(".cache-secret.tmp.42.0.0123456789abcdef"), b"pending")
        .unwrap();
    let mut orphan_cfg = Config::from_default_yaml(tmp.path(), listen, None);
    orphan_cfg.storage.cache_dir = orphan_tmp_cache;
    crate::config_validation::validate_startup_config(&mut orphan_cfg).unwrap();
    super::claim_ephemeral_cache_dir(&orphan_cfg).await.unwrap();

    let dirty_pending_cache = tmp.path().join("dirty-pending-cache");
    std::fs::create_dir_all(dirty_pending_cache.join("~upstreams/npm")).unwrap();
    std::fs::write(dirty_pending_cache.join(pnpr_config::CACHE_SECRET_MARKER), "pending").unwrap();
    let uncreated_hosted = tmp.path().join("uncreated-hosted");
    let mut dirty_cfg = Config::from_default_yaml(tmp.path(), listen, None);
    dirty_cfg.storage.cache_dir = dirty_pending_cache;
    dirty_cfg.storage.hosted_dir = uncreated_hosted.clone();
    assert!(crate::config_validation::validate_serving_startup_config(&mut dirty_cfg).is_err());
    assert!(super::prepare_storage(&dirty_cfg).await.is_err());
    assert!(
        !uncreated_hosted.exists(),
        "prepare_storage must reject a dirty pending cache before touching hosted storage",
    );

    let oversized_cache = tmp.path().join("oversized-marker-cache");
    std::fs::create_dir_all(&oversized_cache).unwrap();
    std::fs::write(oversized_cache.join(pnpr_config::CACHE_SECRET_MARKER), vec![b'a'; 1024])
        .unwrap();
    let mut oversized_cfg = Config::from_default_yaml(tmp.path(), listen, None);
    oversized_cfg.storage.cache_dir = oversized_cache;
    assert!(crate::config_validation::validate_startup_config(&mut oversized_cfg).is_err());
    assert!(super::claim_ephemeral_cache_dir(&oversized_cfg).await.is_err());
}
