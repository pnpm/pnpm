use super::{CacheCommit, integrity_checker, parse_integrity, tee_verified_to_cache};
use crate::{config::HostedStoreConfig, package_name::PackageName, storage::Storage};
use axum::body::to_bytes;
use ssri::{Algorithm, Integrity, IntegrityOpts};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
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

fn recording_commit() -> (CacheCommit, Arc<Mutex<Option<u64>>>) {
    let recorded = Arc::new(Mutex::new(None));
    let recorded_for_cb = Arc::clone(&recorded);
    let commit: CacheCommit = Box::new(move |len| {
        Box::pin(async move {
            *recorded_for_cb.lock().unwrap() = Some(len);
        })
    });
    (commit, recorded)
}

#[tokio::test]
async fn teed_matching_body_is_cached_and_committed() {
    let bytes: &'static [u8] = b"a verified tarball body";
    let integrity = parse_integrity(&sha512_integrity(bytes)).unwrap();
    let response = reqwest::get(spawn_response(bytes).await).await.unwrap();

    let tmp = TempDir::new().unwrap();
    let cache = tmp.path().join("cache");
    let storage = Storage::new(&HostedStoreConfig::Fs, tmp.path().join("hosted"), cache.clone());
    let name = PackageName::parse("foo").unwrap();
    let write = storage.open_cached_tarball_tmp(&name, "foo-1.0.0.tgz").await.unwrap();

    let (commit, committed) = recording_commit();
    let body = tee_verified_to_cache(response, write, integrity, u64::MAX, commit);
    // The stream closes only after the background task finalizes and commits,
    // so a fully drained body means promotion has already completed.
    let served = to_bytes(body, usize::MAX).await.unwrap();
    assert_eq!(served.as_ref(), bytes);

    let package_dir = cache.join("foo");
    let cached_path = package_dir.join("foo-1.0.0.tgz");
    assert_eq!(std::fs::read(&cached_path).unwrap(), bytes);
    assert_eq!(*committed.lock().unwrap(), Some(bytes.len() as u64));
    assert!(tarball_tmp_entries(&package_dir).is_empty());
}

#[tokio::test]
async fn teed_mismatched_body_is_streamed_but_not_cached() {
    let actual: &'static [u8] = b"actual upstream bytes";
    let declared = parse_integrity(&sha512_integrity(b"a different body entirely")).unwrap();
    let response = reqwest::get(spawn_response(actual).await).await.unwrap();

    let tmp = TempDir::new().unwrap();
    let cache = tmp.path().join("cache");
    let storage = Storage::new(&HostedStoreConfig::Fs, tmp.path().join("hosted"), cache.clone());
    let name = PackageName::parse("foo").unwrap();
    let write = storage.open_cached_tarball_tmp(&name, "foo-1.0.0.tgz").await.unwrap();

    let (commit, committed) = recording_commit();
    let body = tee_verified_to_cache(response, write, declared, u64::MAX, commit);
    let served = to_bytes(body, usize::MAX).await.unwrap();
    // The client still receives the bytes; its own SRI check is what rejects them.
    assert_eq!(served.as_ref(), actual);

    let package_dir = cache.join("foo");
    assert!(!package_dir.join("foo-1.0.0.tgz").exists());
    assert!(committed.lock().unwrap().is_none());
    assert!(tarball_tmp_entries(&package_dir).is_empty());
}

#[tokio::test]
async fn teed_oversized_body_is_aborted_and_not_cached() {
    let bytes: &'static [u8] = b"oversized";
    let integrity = parse_integrity(&sha512_integrity(bytes)).unwrap();
    let response = reqwest::get(spawn_response(bytes).await).await.unwrap();

    let tmp = TempDir::new().unwrap();
    let cache = tmp.path().join("cache");
    let storage = Storage::new(&HostedStoreConfig::Fs, tmp.path().join("hosted"), cache.clone());
    let name = PackageName::parse("foo").unwrap();
    let write = storage.open_cached_tarball_tmp(&name, "foo-1.0.0.tgz").await.unwrap();

    let (commit, committed) = recording_commit();
    let body = tee_verified_to_cache(response, write, integrity, 3, commit);
    // The body exceeds the cap, so the relayed stream is aborted (errors) rather
    // than forwarding unbounded data, and nothing is cached.
    assert!(to_bytes(body, usize::MAX).await.is_err());

    let package_dir = cache.join("foo");
    assert!(!package_dir.join("foo-1.0.0.tgz").exists());
    assert!(committed.lock().unwrap().is_none());
    assert!(tarball_tmp_entries(&package_dir).is_empty());
}

#[tokio::test]
async fn client_disconnect_mid_stream_removes_tmp_file() {
    let expected_bytes = vec![0xAA; 1024 * 1024];
    let integrity = parse_integrity(&sha512_integrity(&expected_bytes)).unwrap();
    let (url, release, server) = spawn_stalled_response().await;
    let response = reqwest::get(url).await.unwrap();

    let tmp = TempDir::new().unwrap();
    let cache = tmp.path().join("cache");
    let storage = Storage::new(&HostedStoreConfig::Fs, tmp.path().join("hosted"), cache.clone());
    let name = PackageName::parse("foo").unwrap();
    let write = storage.open_cached_tarball_tmp(&name, "foo-1.0.0.tgz").await.unwrap();

    let (commit, _committed) = recording_commit();
    let body = tee_verified_to_cache(response, write, integrity, u64::MAX, commit);
    let package_dir = cache.join("foo");
    let in_flight = await_nonempty_tarball_tmp(&package_dir).await;
    assert_eq!(in_flight.len(), 1, "expected one in-flight tarball writer");

    // Client goes away mid-body; releasing the stalled upstream then lets the
    // tee task observe the truncated body / gone receiver and abandon the tmp.
    drop(body);
    release.notify_one();
    server.await.unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        if tarball_tmp_entries(&package_dir).is_empty() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "tmp file was not cleaned up after client disconnect",
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(!package_dir.join("foo-1.0.0.tgz").exists());
}
