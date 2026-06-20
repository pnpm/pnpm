use std::path::PathBuf;

use pacquet_registry::Package;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::{
    ABBREVIATED_META_DIR, FULL_FILTERED_META_DIR, FULL_META_DIR, encode_pkg_name,
    get_pkg_mirror_path, get_registry_name, load_meta, load_meta_headers, save_meta_indexed,
};

#[test]
fn encode_pkg_name_passes_lowercase_through() {
    assert_eq!(encode_pkg_name("lodash"), "lodash");
    assert_eq!(encode_pkg_name("@scope/foo"), "@scope/foo");
}

#[test]
fn encode_pkg_name_hash_suffix_for_mixed_case() {
    let got = encode_pkg_name("LRUCache");
    assert!(got.starts_with("LRUCache_"), "got: {got}");
    let suffix = got.trim_start_matches("LRUCache_");
    assert_eq!(suffix.len(), 64, "sha256 hex is 64 chars");
    assert!(suffix.chars().all(|ch| ch.is_ascii_hexdigit()));
}

#[test]
fn get_registry_name_default_scheme() {
    let got = get_registry_name("https://registry.npmjs.org/").expect("encode");
    assert_eq!(got, "registry.npmjs.org");
}

#[test]
fn get_registry_name_with_port() {
    let got = get_registry_name("https://npm.example:8443/").expect("encode");
    assert_eq!(got, "npm.example+8443");
}

#[test]
fn get_registry_name_default_port_omitted() {
    let got = get_registry_name("https://npm.example:443/").expect("encode");
    assert_eq!(got, "npm.example");
}

/// Callers (notably the cached fetcher) downgrade to a cache-less
/// fetch on this error instead of failing the install.
#[test]
fn get_registry_name_rejects_malformed_url() {
    let err = get_registry_name("not a url").expect_err("malformed url must error");
    assert!(matches!(err, super::EncodeRegistryError::ParseUrl { .. }), "got: {err:?}");
}

#[test]
fn get_pkg_mirror_path_composes_full_path() {
    let dir = PathBuf::from("/cache");
    let got = get_pkg_mirror_path(&dir, FULL_META_DIR, "https://registry.npmjs.org/", "lodash")
        .expect("compose");
    assert_eq!(got, PathBuf::from("/cache/v11/metadata-full/registry.npmjs.org/lodash.jsonl"));
}

/// Constants match upstream's `core/constants/src/index.ts` slugs.
/// Any drift would silently fork the cache layout from pnpm's.
#[test]
fn constants_match_upstream() {
    assert_eq!(FULL_META_DIR, "v11/metadata-full");
    assert_eq!(FULL_FILTERED_META_DIR, "v11/metadata-full-filtered");
    assert_eq!(ABBREVIATED_META_DIR, "v11/metadata");
}

/// Build a minimal `Package` fixture for the round-trip tests.
fn fixture_package() -> Package {
    let body = serde_json::json!({
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "modified": "2025-01-15T12:00:00.000Z",
        "time": { "1.0.0": "2025-01-10T08:30:00.000Z" },
        "versions": {
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "dist": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": "https://registry/acme-1.0.0.tgz"
                }
            }
        }
    });
    serde_json::from_value(body).expect("deserialize fixture Package")
}

#[test]
fn load_meta_headers_round_trip() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("nested").join("lodash.jsonl");
    let pkg = fixture_package();
    save_meta_indexed(&mirror, &pkg, Some(r#"W/"abc""#)).expect("save");
    let headers = load_meta_headers(&mirror).expect("read headers back");
    assert_eq!(headers.etag.as_deref(), Some(r#"W/"abc""#));
    assert_eq!(headers.modified.as_deref(), Some("2025-01-15T12:00:00.000Z"));
}

#[test]
fn load_meta_round_trip_hydrates_versions_from_spans() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("acme.jsonl");
    let pkg = fixture_package();
    save_meta_indexed(&mirror, &pkg, Some(r#"W/"abc""#)).expect("save");
    let loaded = load_meta(&mirror).expect("read full back");
    assert_eq!(loaded.name, "acme");
    assert_eq!(loaded.etag.as_deref(), Some(r#"W/"abc""#));
    assert_eq!(loaded.published_at("1.0.0"), Some("2025-01-10T08:30:00.000Z"));
    assert_eq!(loaded.dist_tag("latest"), Some("1.0.0"));
    let manifest = loaded.versions.get("1.0.0").expect("hydrate from file span");
    assert_eq!(manifest.dist.tarball, "https://registry/acme-1.0.0.tgz");
}

#[test]
fn load_meta_rejects_truncated_fragments() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("acme.jsonl");
    let pkg = fixture_package();
    save_meta_indexed(&mirror, &pkg, None).expect("save");
    let full = std::fs::read(&mirror).expect("read mirror");
    std::fs::write(&mirror, &full[..full.len() - 10]).expect("truncate");
    assert!(load_meta(&mirror).is_none());
}

/// pnpm and pacquet must share the same on-disk metadata mirror.
#[test]
fn pnpm_ndjson_format_reads_as_cache_hit() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("acme.jsonl");
    let mut pkg = fixture_package();
    pkg.modified = None;
    std::fs::write(
        &mirror,
        format!(
            "{{\"etag\":\"W/abc\",\"modified\":\"2025-01-15T12:00:00.000Z\"}}\n{}",
            serde_json::to_string(&pkg).expect("serialize fixture"),
        ),
    )
    .expect("write pnpm format");
    let headers = load_meta_headers(&mirror).expect("read headers");
    assert_eq!(headers.etag.as_deref(), Some("W/abc"));
    let meta = load_meta(&mirror).expect("read meta");
    assert_eq!(meta.etag.as_deref(), Some("W/abc"));
    assert_eq!(meta.modified.as_deref(), Some("2025-01-15T12:00:00.000Z"));
    assert_eq!(meta.published_at("1.0.0"), Some("2025-01-10T08:30:00.000Z"));
}

#[test]
fn load_helpers_return_none_on_missing_file() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("does-not-exist.jsonl");
    assert!(load_meta_headers(&mirror).is_none());
    assert!(load_meta(&mirror).is_none());
}

#[test]
fn load_helpers_return_none_on_malformed_mirror() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("bad.jsonl");
    std::fs::write(&mirror, "no-newline-only-header").expect("write garbage");
    assert!(load_meta_headers(&mirror).is_none());
    assert!(load_meta(&mirror).is_none());
}

#[test]
fn save_meta_overwrites_existing_mirror() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("acme.jsonl");
    let pkg = fixture_package();

    save_meta_indexed(&mirror, &pkg, Some(r#"W/"old""#)).expect("first save");
    save_meta_indexed(&mirror, &pkg, Some(r#"W/"new""#)).expect("second save");

    let headers = load_meta_headers(&mirror).expect("read headers");
    assert_eq!(headers.etag.as_deref(), Some(r#"W/"new""#));
}
