use super::{assert_failure_contains, pacquet_in, project, wheel};
use assert_cmd::prelude::*;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

/// One release an index serves with a PEP 658 metadata file beside its
/// wheel.
struct Release {
    name: &'static str,
    requires: &'static str,
}

impl Release {
    fn metadata(&self) -> String {
        format!("Metadata-Version: 2.4\nName: {}\nVersion: 1.0\n{}\n", self.name, self.requires)
    }

    fn filename(&self) -> String {
        format!("{}-1.0-py3-none-any.whl", self.name)
    }

    fn page(&self, metadata_digest: &str) -> String {
        let archive = wheel(self.name, "1.0", self.requires, &[]);
        json!({
            "meta": {"api-version": "1.0"},
            "name": self.name,
            "files": [{
                "filename": self.filename(),
                "url": format!("/files/{}", self.filename()),
                "hashes": {"sha256": format!("{:x}", Sha256::digest(&archive))},
                "core-metadata": {"sha256": metadata_digest},
            }],
        })
        .to_string()
    }

    fn digest(&self) -> String {
        format!("{:x}", Sha256::digest(self.metadata()))
    }
}

/// Serve `release`: its index page, its metadata file, and its wheel,
/// which is downloaded `wheel_downloads` times, or any number of times
/// when that is `None`.
async fn serve_release(
    server: &mut mockito::ServerGuard,
    release: &Release,
    wheel_downloads: Option<usize>,
) -> Vec<mockito::Mock> {
    let wheel_mock = server
        .mock("GET", format!("/files/{}", release.filename()).as_str())
        .with_body(wheel(release.name, "1.0", release.requires, &[]));
    let wheel_mock = match wheel_downloads {
        Some(downloads) => wheel_mock.expect(downloads),
        None => wheel_mock.expect_at_least(0),
    };
    vec![
        wheel_mock.create_async().await,
        server
            .mock("GET", format!("/simple/{}/", release.name).as_str())
            .with_header("content-type", "application/vnd.pypi.simple.v1+json")
            .with_body(release.page(&release.digest()))
            .create_async()
            .await,
        server
            .mock("GET", format!("/files/{}.metadata", release.filename()).as_str())
            .with_body(release.metadata())
            .expect(1)
            .create_async()
            .await,
    ]
}

const ALPHA: Release = Release { name: "alpha", requires: "Requires-Dist: beta" };
const BETA: Release = Release { name: "beta", requires: "" };

#[tokio::test]
async fn resolution_reads_the_metadata_file_instead_of_the_wheel() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let mut mocks = Vec::new();
    for release in [&ALPHA, &BETA] {
        mocks.extend(serve_release(&mut server, release, Some(0)).await);
    }
    project(root.path(), &server.url(), &["alpha"]);

    pacquet_in(root.path())
        .args(["install", "--lockfile-only"])
        .assert()
        .success();

    let lock = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
    assert!(lock.contains(r#"name = "beta""#), "locks what the metadata file requires: {lock}");
    for mock in mocks {
        mock.assert_async().await;
    }
}

#[tokio::test]
async fn a_metadata_file_that_does_not_match_its_published_digest_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _page = server
        .mock("GET", "/simple/beta/")
        .with_header("content-type", "application/vnd.pypi.simple.v1+json")
        .with_body(BETA.page(&"0".repeat(64)))
        .create_async()
        .await;
    let _metadata = server
        .mock("GET", format!("/files/{}.metadata", BETA.filename()).as_str())
        .with_body(BETA.metadata())
        .create_async()
        .await;
    project(root.path(), &server.url(), &["beta"]);

    assert_failure_contains(
        pacquet_in(root.path()).args(["install", "--lockfile-only"]),
        "does not match the SHA-256 the index published",
    );
}

/// An index that lists a metadata file it does not serve leaves the
/// metadata to be read out of the wheel.
#[tokio::test]
async fn a_metadata_file_the_index_does_not_serve_is_read_from_the_wheel() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _page = server
        .mock("GET", "/simple/beta/")
        .with_header("content-type", "application/vnd.pypi.simple.v1+json")
        .with_body(BETA.page(&BETA.digest()))
        .create_async()
        .await;
    let _metadata = server
        .mock("GET", format!("/files/{}.metadata", BETA.filename()).as_str())
        .with_status(404)
        .create_async()
        .await;
    let wheel_download = server
        .mock("GET", format!("/files/{}", BETA.filename()).as_str())
        .with_body(wheel("beta", "1.0", "", &[]))
        .expect_at_least(1)
        .create_async()
        .await;
    project(root.path(), &server.url(), &["beta"]);

    pacquet_in(root.path())
        .args(["install", "--lockfile-only"])
        .assert()
        .success();
    wheel_download.assert_async().await;
}

/// An offline resolution reads the metadata files an earlier run cached.
/// Resolving downloaded no wheel, so there is none to read them from.
#[tokio::test]
async fn an_offline_resolution_reads_cached_metadata_files() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let mut mocks = Vec::new();
    for release in [&ALPHA, &BETA] {
        mocks.extend(serve_release(&mut server, release, Some(0)).await);
    }
    project(root.path(), &server.url(), &["alpha"]);
    pacquet_in(root.path())
        .args(["install", "--lockfile-only"])
        .assert()
        .success();
    for mock in mocks {
        mock.assert_async().await;
    }
    drop(server);
    let lock = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
    fs::remove_file(root.path().join("pylock.toml")).unwrap();

    pacquet_in(root.path())
        .args(["install", "--lockfile-only", "--offline"])
        .assert()
        .success();
    assert_eq!(fs::read_to_string(root.path().join("pylock.toml")).unwrap(), lock);
}

/// Every index page and metadata file a resolution can name up front is
/// in flight together: each mock answers only once all of its siblings
/// were requested, so reading them one at a time times out.
#[tokio::test]
async fn index_pages_and_metadata_files_are_read_concurrently() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let releases = ["alpha", "beta", "gamma"].map(|name| Release { name, requires: "" });
    let pages = Arc::new((Mutex::new(0), Condvar::new()));
    let metadata_files = Arc::new((Mutex::new(0), Condvar::new()));
    let mut mocks = Vec::new();
    for release in &releases {
        let page = release
            .page(&release.digest())
            .into_bytes();
        let pages = Arc::clone(&pages);
        mocks.push(
            server
                .mock("GET", format!("/simple/{}/", release.name).as_str())
                .with_header("content-type", "application/vnd.pypi.simple.v1+json")
                .with_chunked_body(move |writer| answer_together(&pages, writer, &page))
                .expect(1)
                .create_async()
                .await,
        );
        let metadata = release.metadata().into_bytes();
        let metadata_files = Arc::clone(&metadata_files);
        mocks.push(
            server
                .mock("GET", format!("/files/{}.metadata", release.filename()).as_str())
                .with_chunked_body(move |writer| {
                    answer_together(&metadata_files, writer, &metadata)
                })
                .expect(1)
                .create_async()
                .await,
        );
    }
    project(root.path(), &server.url(), &["alpha", "beta", "gamma"]);

    pacquet_in(root.path())
        .args(["install", "--lockfile-only"])
        .assert()
        .success();
    for mock in mocks {
        mock.assert_async().await;
    }
}

/// Count this request in and answer once all three were made.
fn answer_together(
    rendezvous: &(Mutex<usize>, Condvar),
    writer: &mut dyn Write,
    body: &[u8],
) -> std::io::Result<()> {
    let (arrivals, wake) = rendezvous;
    let mut arrivals = arrivals.lock().unwrap();
    *arrivals += 1;
    wake.notify_all();
    let (arrivals, timeout) = wake
        .wait_timeout_while(arrivals, Duration::from_secs(10), |arrivals| *arrivals < 3)
        .unwrap();
    drop(arrivals);
    if timeout.timed_out() {
        return Err(std::io::Error::other("the sibling requests were never made"));
    }
    writer.write_all(body)
}
