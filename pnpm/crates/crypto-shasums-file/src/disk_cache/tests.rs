use std::{path::Path, time::Duration};

use pretty_assertions::assert_eq;

use super::{ShasumsTrust, read_cached_shasums, shasums_cache_path, write_cached_shasums};

#[test]
fn maps_a_shasums_url_under_the_cache_dir() {
    let path = shasums_cache_path(
        Path::new("/cache"),
        ShasumsTrust::Verified,
        "https://nodejs.org/download/release/v22.11.0/SHASUMS256.txt",
    )
    .expect("mappable URL");
    assert_eq!(
        path,
        Path::new(
            "/cache/v11/runtime-shasums/verified/nodejs.org/download/release/v22.11.0/SHASUMS256.txt",
        ),
    );
}

#[test]
fn encodes_hosts_ports_and_unusual_segments() {
    let path = shasums_cache_path(
        Path::new("/cache"),
        ShasumsTrust::Unverified,
        "http://Mirror.Example.com:8443/Node%20Dist/v22.11.0/SHASUMS256.txt",
    )
    .expect("mappable URL");
    assert_eq!(
        path,
        Path::new(
            "/cache/v11/runtime-shasums/unverified/mirror.example.com+8443/Node%2520Dist/v22.11.0/SHASUMS256.txt",
        ),
    );
}

#[test]
fn rejects_urls_the_mapping_cannot_represent() {
    let not_representable = [
        "ftp://nodejs.org/v22.11.0/SHASUMS256.txt",
        "https://nodejs.org/v22.11.0/SHASUMS256.txt?token=1",
        "https://nodejs.org/v22.11.0/SHASUMS256.txt#fragment",
        "https://user@nodejs.org/v22.11.0/SHASUMS256.txt",
        "https://nodejs.org/v22.11.0/../SHASUMS256.txt",
        "https://nodejs.org//SHASUMS256.txt",
        "https://nodejs.org",
        "https:///v22.11.0/SHASUMS256.txt",
    ];
    for url in not_representable {
        assert_eq!(
            shasums_cache_path(Path::new("/cache"), ShasumsTrust::Verified, url),
            None,
            "url={url:?}",
        );
    }
}

#[test]
fn round_trips_a_cached_body() {
    let cache_dir = tempfile::tempdir().expect("create temp cache dir");
    let url = "https://nodejs.org/download/release/v22.11.0/SHASUMS256.txt";

    assert_eq!(
        read_cached_shasums(Some(cache_dir.path()), ShasumsTrust::Verified, url, None),
        None,
    );
    write_cached_shasums(
        Some(cache_dir.path()),
        ShasumsTrust::Verified,
        url,
        b"abc123  node-v22.11.0-linux-x64.tar.gz\n",
    );
    assert_eq!(
        read_cached_shasums(Some(cache_dir.path()), ShasumsTrust::Verified, url, None).as_deref(),
        Some("abc123  node-v22.11.0-linux-x64.tar.gz\n"),
    );
}

/// A reader that asks for a body no older than an age gets the entry
/// only while it is younger than that, so a URL naming the newest
/// release is refetched once its list may have moved on.
#[test]
fn an_entry_older_than_the_caller_allows_reads_as_a_miss() {
    let cache_dir = tempfile::tempdir().expect("create temp cache dir");
    let url =
        "https://github.com/astral-sh/python-build-standalone/releases/latest/download/SHA256SUMS";
    let body =
        "abc123  cpython-3.13.15+20260901-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz\n";
    write_cached_shasums(Some(cache_dir.path()), ShasumsTrust::Unverified, url, body.as_bytes());

    let read = |max_age| {
        read_cached_shasums(Some(cache_dir.path()), ShasumsTrust::Unverified, url, max_age)
    };
    assert_eq!(read(Some(Duration::from_hours(1))).as_deref(), Some(body));
    assert_eq!(read(None).as_deref(), Some(body));
    // Every entry is at least this old.
    assert_eq!(read(Some(Duration::ZERO)), None);
}

/// The two trust classes cache into disjoint subtrees: a body written
/// by an unverified fetch must never satisfy a reader that expects a
/// signature-verified body.
#[test]
fn trust_classes_do_not_share_entries() {
    let cache_dir = tempfile::tempdir().expect("create temp cache dir");
    let url = "https://nodejs.org/download/release/v22.11.0/SHASUMS256.txt";
    write_cached_shasums(Some(cache_dir.path()), ShasumsTrust::Unverified, url, b"unverified body");

    assert_eq!(
        read_cached_shasums(Some(cache_dir.path()), ShasumsTrust::Verified, url, None),
        None,
    );
    assert_eq!(
        read_cached_shasums(Some(cache_dir.path()), ShasumsTrust::Unverified, url, None).as_deref(),
        Some("unverified body"),
    );
}

/// An empty file can only come from a torn write; it must read as a
/// miss so the next resolve refetches instead of resolving zero assets.
#[test]
fn treats_an_empty_cache_file_as_a_miss() {
    let cache_dir = tempfile::tempdir().expect("create temp cache dir");
    let url = "https://nodejs.org/download/release/v22.11.0/SHASUMS256.txt";
    write_cached_shasums(Some(cache_dir.path()), ShasumsTrust::Verified, url, b"");

    assert_eq!(
        read_cached_shasums(Some(cache_dir.path()), ShasumsTrust::Verified, url, None),
        None,
    );
}
