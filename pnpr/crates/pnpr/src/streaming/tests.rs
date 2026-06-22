use super::{TarballStreamError, download_verified_to_cache, integrity_checker, parse_integrity};
use crate::{config::HostedStoreConfig, package_name::PackageName, storage::Storage};
use ssri::{Algorithm, Integrity, IntegrityOpts};
use std::{path::Path, sync::Arc, time::Duration};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Notify,
};

#[test]
fn accepts_integrity_with_supported_hash() {
    assert!(parse_integrity("sha512-ZGVhZGJlZWY=").is_ok());
}

#[test]
fn rejects_integrity_without_hashes() {
    for value in ["", " \t\n "] {
        let err = parse_integrity(value).unwrap_err();
        assert!(err.to_string().contains("no supported hashes"));
    }

    let zero_hash = Integrity { hashes: Vec::new() };
    assert!(integrity_checker(&zero_hash).is_err());
}

#[test]
fn rejects_malformed_or_unsupported_integrity() {
    for value in ["not-a-valid-sri", "md5-deadbeef"] {
        assert!(parse_integrity(value).is_err());
    }
}

fn sha512_integrity(bytes: &[u8]) -> String {
    let mut opts = IntegrityOpts::new().algorithm(Algorithm::Sha512);
    opts.input(bytes);
    opts.result().to_string()
}

async fn spawn_stalled_response() -> (String, Arc<Notify>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let release = Arc::new(Notify::new());
    let release_for_server = Arc::clone(&release);
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 4096];
        let _ = socket.read(&mut request).await;
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\n\
                  Content-Length: 1048576\r\n\
                  Content-Type: application/octet-stream\r\n\
                  Connection: close\r\n\
                  \r\n",
            )
            .await
            .unwrap();
        socket.write_all(&vec![0xAA; 64 * 1024]).await.unwrap();
        socket.flush().await.unwrap();
        release_for_server.notified().await;
    });
    (format!("http://{addr}/foo/-/foo-1.0.0.tgz"), release, server)
}

async fn spawn_response(bytes: &'static [u8]) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 4096];
        let _ = socket.read(&mut request).await;
        let headers = format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Length: {}\r\n\
             Content-Type: application/octet-stream\r\n\
             Connection: close\r\n\
             \r\n",
            bytes.len(),
        );
        socket.write_all(headers.as_bytes()).await.unwrap();
        socket.write_all(bytes).await.unwrap();
        socket.flush().await.unwrap();
    });
    format!("http://{addr}/foo/-/foo-1.0.0.tgz")
}

fn tarball_tmp_entries(dir: &Path) -> Vec<String> {
    let mut entries = dir
        .read_dir()
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|entry| {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    name.starts_with("foo-1.0.0.tgz.tmp.").then_some(name)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    entries.sort();
    entries
}

async fn await_nonempty_tarball_tmp(dir: &Path) -> Vec<String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        let entries = tarball_tmp_entries(dir);
        if entries
            .iter()
            .any(|name| std::fs::metadata(dir.join(name)).is_ok_and(|metadata| metadata.len() > 0))
        {
            return entries;
        }
        assert!(std::time::Instant::now() < deadline, "tarball body was not written to tmp");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn cancelling_in_flight_response_body_removes_tmp_file() {
    let expected_bytes = vec![0xAA; 1024 * 1024];
    let integrity = parse_integrity(&sha512_integrity(&expected_bytes)).unwrap();
    let (url, release, server) = spawn_stalled_response().await;
    let response = reqwest::get(url).await.unwrap();

    let tmp = TempDir::new().unwrap();
    let cache = tmp.path().join("cache");
    let storage = Storage::new(&HostedStoreConfig::Fs, tmp.path().join("hosted"), cache.clone());
    let name = PackageName::parse("foo").unwrap();
    let write = storage.open_cached_tarball_tmp(&name, "foo-1.0.0.tgz").await.unwrap();

    let download = tokio::spawn(async move {
        download_verified_to_cache(response, write, &integrity, u64::MAX).await
    });
    let package_dir = cache.join("foo");
    let in_flight = await_nonempty_tarball_tmp(&package_dir).await;
    assert_eq!(in_flight.len(), 1, "expected one in-flight tarball writer");

    download.abort();
    assert!(download.await.unwrap_err().is_cancelled());
    assert!(tarball_tmp_entries(&package_dir).is_empty());
    assert!(!package_dir.join("foo-1.0.0.tgz").exists());

    release.notify_one();
    server.await.unwrap();
}

#[tokio::test]
async fn oversized_response_is_rejected_and_tmp_is_removed() {
    let bytes = b"oversized";
    let integrity = parse_integrity(&sha512_integrity(bytes)).unwrap();
    let response = reqwest::get(spawn_response(bytes).await).await.unwrap();

    let tmp = TempDir::new().unwrap();
    let cache = tmp.path().join("cache");
    let storage = Storage::new(&HostedStoreConfig::Fs, tmp.path().join("hosted"), cache.clone());
    let name = PackageName::parse("foo").unwrap();
    let write = storage.open_cached_tarball_tmp(&name, "foo-1.0.0.tgz").await.unwrap();

    let err = download_verified_to_cache(response, write, &integrity, 3).await.unwrap_err();
    assert!(matches!(err, TarballStreamError::TooLarge { limit: 3, received } if received > 3));

    let package_dir = cache.join("foo");
    assert!(tarball_tmp_entries(&package_dir).is_empty());
    assert!(!package_dir.join("foo-1.0.0.tgz").exists());
}
