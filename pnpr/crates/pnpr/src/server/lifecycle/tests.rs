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
    let serve_task = tokio::spawn(super::serve_listener(first_config, listener));

    for _ in 0..200 {
        if cache_dir.join(pnpr_storage::layout::LAYOUT_FILE).exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(cache_dir.join(pnpr_config::CACHE_SECRET_MARKER).exists());

    let stale_private =
        cache_dir.join("~upstreams/ephemeral-run-digest/packages/pkg/document.json");
    tokio::fs::create_dir_all(stale_private.parent().unwrap()).await.unwrap();
    tokio::fs::write(&stale_private, b"{}").await.unwrap();

    serve_task.abort();
    let _ = serve_task.await;

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
    let serve_task3 = tokio::spawn(super::serve_listener(configured_restart, listener3));
    for _ in 0..200 {
        if !stale_private.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
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
    let serve_task = tokio::spawn(super::serve_listener(config, listener));
    for _ in 0..200 {
        if cache_dir.join(pnpr_config::CACHE_SECRET_MARKER).exists()
            && tokio::net::TcpStream::connect(addr).await.is_ok()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    serve_task.abort();
    let _ = serve_task.await;
    assert!(storage_dir.join(pnpr_storage::layout::LAYOUT_FILE).exists());
    assert!(cache_dir.join(pnpr_config::CACHE_SECRET_MARKER).exists());
}
