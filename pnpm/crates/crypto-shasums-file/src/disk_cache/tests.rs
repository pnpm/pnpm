use std::path::Path;

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

    assert_eq!(read_cached_shasums(Some(cache_dir.path()), ShasumsTrust::Verified, url), None);
    write_cached_shasums(
        Some(cache_dir.path()),
        ShasumsTrust::Verified,
        url,
        b"abc123  node-v22.11.0-linux-x64.tar.gz\n",
    );
    assert_eq!(
        read_cached_shasums(Some(cache_dir.path()), ShasumsTrust::Verified, url).as_deref(),
        Some("abc123  node-v22.11.0-linux-x64.tar.gz\n"),
    );
}

/// The two trust classes cache into disjoint subtrees: a body written
/// by an unverified fetch must never satisfy a reader that expects a
/// signature-verified body.
#[test]
fn trust_classes_do_not_share_entries() {
    let cache_dir = tempfile::tempdir().expect("create temp cache dir");
    let url = "https://nodejs.org/download/release/v22.11.0/SHASUMS256.txt";
    write_cached_shasums(Some(cache_dir.path()), ShasumsTrust::Unverified, url, b"unverified body");

    assert_eq!(read_cached_shasums(Some(cache_dir.path()), ShasumsTrust::Verified, url), None);
    assert_eq!(
        read_cached_shasums(Some(cache_dir.path()), ShasumsTrust::Unverified, url).as_deref(),
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

    assert_eq!(read_cached_shasums(Some(cache_dir.path()), ShasumsTrust::Verified, url), None);
}
