use std::path::PathBuf;

use pnpm_registry::Package;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use pnpm_network::MetadataCacheScope;

use super::{
    ABBREVIATED_META_DIR, FULL_FILTERED_META_DIR, FULL_META_DIR, encode_pkg_name,
    get_pkg_mirror_path, get_registry_name, load_meta, load_meta_headers, load_meta_with_hold_cap,
    save_meta_indexed, scoped_meta_dir,
};

#[test]
fn scoped_meta_dir_public_is_unchanged() {
    assert_eq!(
        scoped_meta_dir(&MetadataCacheScope::Public, ABBREVIATED_META_DIR),
        ABBREVIATED_META_DIR,
    );
    assert_eq!(scoped_meta_dir(&MetadataCacheScope::Public, FULL_META_DIR), FULL_META_DIR);
}

#[test]
fn scoped_meta_dir_private_namespaces_by_descriptor() {
    let scope = MetadataCacheScope::Private { descriptor_id: "abc123".to_string() };
    assert_eq!(
        scoped_meta_dir(&scope, ABBREVIATED_META_DIR),
        "v11/metadata-private/abc123/metadata",
    );
    assert_eq!(scoped_meta_dir(&scope, FULL_META_DIR), "v11/metadata-private/abc123/metadata-full");
    assert_eq!(
        scoped_meta_dir(&scope, FULL_FILTERED_META_DIR),
        "v11/metadata-private/abc123/metadata-full-filtered",
    );
    // Distinct descriptors never share a directory.
    let other = MetadataCacheScope::Private { descriptor_id: "def456".to_string() };
    assert_ne!(scoped_meta_dir(&scope, FULL_META_DIR), scoped_meta_dir(&other, FULL_META_DIR));
}

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
fn load_meta_survives_mirror_rewrite() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("acme.jsonl");
    let pkg = fixture_package();
    save_meta_indexed(&mirror, &pkg, None).expect("save");
    let loaded = load_meta(&mirror).expect("read full back");
    // The fatter `0.9.0` fragment shifts `1.0.0`'s offset, so a loader
    // that re-read the path instead of the pinned inode parses garbage.
    let newer: Package = serde_json::from_value(serde_json::json!({
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "0.9.0": {
                "name": "acme",
                "version": "0.9.0",
                "deprecated": "superseded by 1.0.0; padding padding padding padding",
                "dist": {
                    "integrity": "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
                    "shasum": "1111111111111111111111111111111111111111",
                    "tarball": "https://registry/acme-0.9.0.tgz"
                }
            },
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "dist": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": "https://registry/acme-1.0.0-rebuilt.tgz"
                }
            }
        }
    }))
    .expect("deserialize rewritten Package");
    save_meta_indexed(&mirror, &newer, None).expect("overwrite");
    let manifest = loaded.versions.get("1.0.0").expect("hydrate after rewrite");
    assert_eq!(manifest.dist.tarball, "https://registry/acme-1.0.0.tgz");
}

#[test]
fn load_meta_past_the_hold_cap_buffers_fragments_instead_of_missing() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("acme.jsonl");
    let pkg = fixture_package();
    save_meta_indexed(&mirror, &pkg, Some(r#"W/"abc""#)).expect("save");
    let loaded = load_meta_with_hold_cap(&mirror, 0).expect("read full back without a handle");
    let manifest = loaded.versions.get("1.0.0").expect("hydrate from buffered fragment");
    assert_eq!(manifest.dist.tarball, "https://registry/acme-1.0.0.tgz");
    assert_eq!(loaded.etag.as_deref(), Some(r#"W/"abc""#));
}

#[test]
fn load_meta_rejects_oversized_declared_record_lengths() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("acme.jsonl");
    std::fs::write(&mirror, "pacquet-meta-v1 128 999999999999\n{}{}").expect("write");
    assert!(load_meta(&mirror).is_none());
}

#[test]
fn load_meta_past_the_hold_cap_ignores_a_sparse_tail() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("acme.jsonl");
    let pkg = fixture_package();
    save_meta_indexed(&mirror, &pkg, None).expect("save");
    let file = std::fs::OpenOptions::new().write(true).open(&mirror).expect("open");
    let size = file.metadata().expect("metadata").len();
    file.set_len(size + 64 * 1024 * 1024).expect("extend sparsely");
    let loaded = load_meta_with_hold_cap(&mirror, 0).expect("read full back without a handle");
    let manifest = loaded.versions.get("1.0.0").expect("hydrate from buffered fragment");
    assert_eq!(manifest.dist.tarball, "https://registry/acme-1.0.0.tgz");
}

#[test]
fn load_meta_past_the_hold_cap_skips_a_sparse_gap_between_spans() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("acme.jsonl");
    let headers = "{}";
    let fragment = r#"{"name":"acme","version":"1.0.0","dist":{"integrity":"sha512-A","shasum":"0","tarball":"https://registry/acme-1.0.0.tgz"}}"#;
    let far_offset: u64 = 512 * 1024 * 1024;
    let index = format!(
        r#"{{"name":"acme","distTags":{{}},"versions":[["1.0.0",0,{}],["2.0.0",{far_offset},16]]}}"#,
        fragment.len(),
    );
    let contents =
        format!("pacquet-meta-v1 {} {}\n{headers}{index}{fragment}", headers.len(), index.len());
    std::fs::write(&mirror, &contents).expect("write");
    let file = std::fs::OpenOptions::new().write(true).open(&mirror).expect("open");
    file.set_len(contents.len() as u64 + far_offset + 16).expect("extend sparsely");
    let loaded = load_meta_with_hold_cap(&mirror, 0).expect("read full back without a handle");
    let manifest = loaded.versions.get("1.0.0").expect("hydrate the near fragment");
    assert_eq!(manifest.dist.tarball, "https://registry/acme-1.0.0.tgz");
    // The far span reads zeroes out of the sparse hole — not JSON, so
    // the version is absent; the gap itself must never be buffered.
    assert!(loaded.versions.get("2.0.0").is_none());
}

#[test]
fn load_meta_treats_an_oversized_fragment_span_as_absent() {
    let dir = TempDir::new().expect("tmp dir");
    let mirror = dir.path().join("acme.jsonl");
    let headers = "{}";
    let fragment = r#"{"name":"acme","version":"1.0.0","dist":{"integrity":"sha512-A","shasum":"0","tarball":"https://registry/acme-1.0.0.tgz"}}"#;
    let index = format!(
        r#"{{"name":"acme","distTags":{{}},"versions":[["1.0.0",0,{}],["9.9.9",{},{}]]}}"#,
        fragment.len(),
        fragment.len(),
        32 * 1024 * 1024,
    );
    let contents =
        format!("pacquet-meta-v1 {} {}\n{headers}{index}{fragment}", headers.len(), index.len());
    std::fs::write(&mirror, &contents).expect("write");
    // A sparse tail makes the file size cover the declared span
    // without paying for the bytes, like a corrupt mirror would.
    let file = std::fs::OpenOptions::new().write(true).open(&mirror).expect("open");
    file.set_len(contents.len() as u64 + 64 * 1024 * 1024).expect("extend sparsely");
    let loaded = load_meta(&mirror).expect("read full back");
    assert!(loaded.versions.get("9.9.9").is_none(), "oversized span must read as absent");
    let manifest = loaded.versions.get("1.0.0").expect("hydrate the in-bounds fragment");
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
