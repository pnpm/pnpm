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

    let stale_slot = std::mem::ManuallyDrop::new(
        storage.reserve_hosted_blob(&name, "large-pkg-1.0.0.tgz").await.unwrap(),
    );
    let stale_path = stale_slot.tmp_path.clone();
    tokio::fs::write(&stale_path, b"stale-staging-payload").await.unwrap();
    set_age(&stale_path, Duration::from_mins(2));

    let fresh_slot = std::mem::ManuallyDrop::new(
        storage.reserve_hosted_blob(&name, "large-pkg-1.0.1.tgz").await.unwrap(),
    );
    let fresh_path = fresh_slot.tmp_path.clone();
    tokio::fs::write(&fresh_path, b"just-crashed-payload").await.unwrap();

    sweep_abandoned_archive_temps(&config).await.unwrap();
    assert!(!stale_path.exists(), "startup sweep must remove staging files past request_timeout");
    assert!(fresh_path.exists(), "startup sweep must keep staging files within request_timeout");

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
}

#[tokio::test(start_paused = true)]
async fn startup_sweep_reclaims_local_hosted_archive_temps_across_orgs() {
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
    org: acme
",
        storage = tmp.path().join("storage").display(),
        cache = tmp.path().join("cache").display(),
    );
    std::fs::write(&config_path, yaml).unwrap();
    let config = Config::from_yaml(&config_path, listen, None).unwrap();
    let storage = Storage::from_config(&config.storage).unwrap().for_hosted("acme", Ecosystem::Npm);
    let name = CanonicalPackageName::parse("@acme/pkg", Ecosystem::Npm).unwrap();

    let stale_slot = std::mem::ManuallyDrop::new(
        storage.reserve_hosted_blob(&name, "pkg-1.0.0.tgz").await.unwrap(),
    );
    let stale_path = stale_slot.tmp_path.clone();
    tokio::fs::write(&stale_path, b"stale-local-payload").await.unwrap();
    set_age(&stale_path, Duration::from_mins(2));

    sweep_abandoned_archive_temps(&config).await.unwrap();
    assert!(!stale_path.exists());
}
